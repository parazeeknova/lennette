import { describe, expect, mock, test } from "bun:test";
import { deploy } from "./deploy-api";

const image = `ghcr.io/parazeeknova/lennette-api@sha256:${"a".repeat(64)}`;
const env = {
  DOKPLOY_URL: "https://dokploy.example",
  DOKPLOY_API_KEY: "private-test-key",
  DOKPLOY_APPLICATION_ID: "test-app",
  API_IMAGE: image,
  API_HEALTH_URL: "https://api.example/health/ready",
  DEPLOYMENT_TITLE: "test release",
};
const json = (value: unknown) => Response.json(value);

describe("production deployment gates", () => {
  test("rejects mutable images and HTTP before sending credentials", async () => {
    const transport = mock(async () => json({}));
    await expect(
      deploy({ ...env, API_IMAGE: "ghcr.io/parazeeknova/lennette-api:latest" }, transport),
    ).rejects.toThrow("digest");
    await expect(
      deploy({ ...env, DOKPLOY_URL: "http://dokploy.example" }, transport),
    ).rejects.toThrow("HTTPS");
    expect(transport).not.toHaveBeenCalled();
  });

  test("failed deployment restores the previous desired image", async () => {
    const requests: { url: string; body: any }[] = [];
    const transport = async (url: string, options?: RequestInit) => {
      requests.push({ url, body: options?.body ? JSON.parse(String(options.body)) : null });
      if (url.includes("application.one"))
        return json({ dockerImage: "previous-image", appName: "lennette" });
      if (url.includes("deployment.all"))
        return json([{ title: env.DEPLOYMENT_TITLE, status: "error" }]);
      return json({});
    };
    await expect(deploy(env, transport)).rejects.toThrow("deployment failed");
    expect(requests.at(-1)?.body.dockerImage).toBe("previous-image");
    expect(requests.some((request) => request.url === env.API_HEALTH_URL)).toBe(false);
  });

  test("old deployment and healthy rolled-back image cannot count as success", async () => {
    let deployments = 0;
    let containers = 0;
    let healthCalls = 0;
    const sleep = mock(async () => {});
    const transport = async (url: string) => {
      if (url.includes("application.one"))
        return json({ dockerImage: "previous-image", appName: "lennette" });
      if (url.includes("deployment.all"))
        return json([
          { title: ++deployments === 1 ? "old release" : env.DEPLOYMENT_TITLE, status: "done" },
        ]);
      if (url.includes("docker.getContainers"))
        return json([
          {
            name: "lennette.1.task",
            containerId: "test-container",
            state: "running",
            image: "ghcr.io/parazeeknova/lennette-api",
          },
        ]);
      if (url.includes("docker.getConfig"))
        return json({
          Config: { Image: ++containers === 1 ? "previous-image" : image },
          State: { Health: { Status: "healthy" } },
        });
      if (url === env.API_HEALTH_URL) {
        healthCalls++;
        return new Response(null, { status: 204 });
      }
      return json({});
    };
    await deploy(env, transport, sleep);
    expect(sleep).toHaveBeenCalledTimes(2);
    expect(healthCalls).toBe(1);
  });

  test("errors never expose API response credentials", async () => {
    const transport = async () => new Response("private response credentials", { status: 403 });
    await expect(deploy(env, transport)).rejects.toThrow("HTTP 403");
    await expect(deploy(env, transport)).rejects.not.toThrow("private response credentials");
  });
});
