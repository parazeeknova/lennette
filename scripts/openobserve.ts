import { randomBytes } from "node:crypto";

// A healthy HTTP listener does not mean OpenObserve's ingester is initialized.
// Send a real, synthetic span before allowing the local API to start exporting.
export async function waitForLocalIngestion(env = process.env, request = fetch, { now = Date.now, sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)) } = {}) {
  const authorization = `Basic ${Buffer.from(`${env.OPENOBSERVE_USER_EMAIL ?? "dev@lennette.local"}:${env.OPENOBSERVE_USER_PASSWORD ?? "LocalOnly-Observe123!"}`).toString("base64")}`;
  const deadline = now() + 90_000;
  let failure = "collector unavailable";
  while (now() < deadline) {
    const start = BigInt(now()) * 1_000_000n;
    try {
      const response = await request("http://127.0.0.1:5080/api/default/v1/traces", {
        method: "POST",
        headers: {
          Authorization: authorization,
          "Content-Type": "application/json",
          "stream-name": "lennette_api",
        },
        signal: AbortSignal.timeout(Math.min(30_000, deadline - now())),
        body: JSON.stringify({ resourceSpans: [{
          resource: { attributes: [{ key: "service.name", value: { stringValue: "lennette-dev-readiness" } }] },
          scopeSpans: [{ scope: { name: "lennette-dev" }, spans: [{
            traceId: randomBytes(16).toString("hex"), spanId: randomBytes(8).toString("hex"),
            name: "local_ingestion_ready", kind: 1,
            startTimeUnixNano: start.toString(), endTimeUnixNano: (start + 1_000_000n).toString(),
          }] }],
        }] }),
      });
      if (response.status === 401 || response.status === 403) {
        await response.body?.cancel();
        throw new Error("OpenObserve ingestion authentication failed; check OPENOBSERVE_USER_EMAIL and OPENOBSERVE_USER_PASSWORD against the persisted local instance.");
      }
      if (response.ok) {
        const body = await response.json();
        if (Number(body.partialSuccess?.rejectedSpans ?? body.partial_success?.rejected_spans ?? 0) === 0) return;
        failure = "collector rejected the readiness span";
      } else {
        failure = `HTTP ${response.status}`;
        await response.body?.cancel();
      }
    } catch (error) {
      if (error instanceof Error && error.message.startsWith("OpenObserve ingestion authentication failed")) throw error;
      // Avoid printing URLs with credentials or collector response bodies.
      failure = "ingestion request failed or timed out";
    }
    await sleep(1000);
  }
  throw new Error(`OpenObserve ingestion did not become ready (${failure}); inspect its container logs.`);
}
