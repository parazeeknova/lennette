/** Deploy a published digest and verify the running image and public readiness. */
type Environment = Record<string, string | undefined>;
type Transport = (url: string, options?: RequestInit) => Promise<Response>;
type Application = { dockerImage?: string; appName: string; serverId?: string };
type Deployment = { title: string; status: string };
type Container = { name: string; state: string; image: string };

export async function deploy(
  env: Environment,
  transport: Transport = fetch,
  sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
) {
  const required = [
    "DOKPLOY_URL",
    "DOKPLOY_API_KEY",
    "DOKPLOY_APPLICATION_ID",
    "API_IMAGE",
    "API_HEALTH_URL",
    "DEPLOYMENT_TITLE",
  ];
  const missing = required.filter((name) => !env[name]);
  if (missing.length) throw new Error(`Missing deployment settings: ${missing.join(", ")}`);
  const base = env.DOKPLOY_URL!.replace(/\/$/, "");
  const health = env.API_HEALTH_URL!;
  if ([base, health].some((url) => new URL(url).protocol !== "https:")) {
    throw new Error("Deployment and health endpoints must use HTTPS");
  }
  const image = env.API_IMAGE!;
  if (!/^ghcr\.io\/parazeeknova\/lennette-api@sha256:[0-9a-f]{64}$/.test(image)) {
    throw new Error("API_IMAGE must be the published Lennette GHCR image digest");
  }
  const appId = env.DOKPLOY_APPLICATION_ID!;
  const title = env.DEPLOYMENT_TITLE!;
  async function api<T>(path: string, payload?: unknown): Promise<T> {
    const response = await transport(`${base}/api/${path}`, {
      method: payload ? "POST" : "GET",
      headers: { "x-api-key": env.DOKPLOY_API_KEY!, "Content-Type": "application/json" },
      body: payload ? JSON.stringify(payload) : undefined,
      redirect: "error",
      signal: AbortSignal.timeout(30_000),
    });
    // application.one returns credentials. Never print API response bodies.
    if (!response.ok)
      throw new Error(`Dokploy ${path.split("?")[0]} returned HTTP ${response.status}`);
    return response.json() as Promise<T>;
  }
  const query = new URLSearchParams({ applicationId: appId });
  const current = await api<Application>(`application.one?${query}`);
  await api("application.update", { applicationId: appId, dockerImage: image });
  try {
    await api("application.deploy", { applicationId: appId, title, description: image });
    let complete = false;
    for (let attempt = 0; attempt < 90; attempt++) {
      const records = await api<Deployment[]>(`deployment.all?${query}`);
      const record = records.find((item) => item.title === title);
      if (record?.status === "error")
        throw new Error("Dokploy deployment failed; inspect its deployment log");
      if (record?.status === "done") {
        complete = true;
        break;
      }
      await sleep(10_000);
    }
    if (!complete) throw new Error("Dokploy deployment did not complete within 15 minutes");
    const server = current.serverId ? `?serverId=${encodeURIComponent(current.serverId)}` : "";
    for (let attempt = 0; attempt < 30; attempt++) {
      const containers = await api<Container[]>(`docker.getContainers${server}`);
      const running = containers.some(
        (container) =>
          container.name.startsWith(`${current.appName}.`) &&
          container.state === "running" &&
          container.image === image,
      );
      if (running) {
        try {
          const response = await transport(health, {
            redirect: "error",
            signal: AbortSignal.timeout(10_000),
          });
          if (response.status === 204) {
            console.log(
              `Deployed ${image}; running digest verified; production readiness HTTP 204`,
            );
            return;
          }
        } catch {
          /* Retry readiness while the service or certificate starts. */
        }
      }
      await sleep(10_000);
    }
    throw new Error("Published image did not become ready within 5 minutes");
  } catch (error) {
    // Swarm rolls back the running service; restore Dokploy's desired image too.
    if (current.dockerImage && current.dockerImage !== image) {
      try {
        await api("application.update", { applicationId: appId, dockerImage: current.dockerImage });
      } catch {
        console.error(
          "Could not restore Dokploy's previous desired image; inspect the application.",
        );
      }
    }
    throw error;
  }
}

if (import.meta.main) {
  try {
    await deploy(process.env);
  } catch (error) {
    console.error(`Deployment failed: ${error instanceof Error ? error.message : "unknown error"}`);
    process.exitCode = 1;
  }
}
