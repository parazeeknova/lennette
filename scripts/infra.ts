import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { waitForLocalIngestion } from "./openobserve.ts";

const mode = process.argv[2];
if (mode !== "up" && mode !== "down") throw new Error("Expected up or down");
const result = spawnSync(
  "docker",
  [
    "compose",
    "-f",
    "infra/compose.yaml",
    ...(mode === "up" ? ["up", "-d", "--wait", "--wait-timeout", "300"] : ["down"]),
  ],
  {
    cwd: fileURLToPath(new URL("../", import.meta.url)),
    stdio: "inherit",
  },
);
if (result.error) {
  console.error("Could not run Docker. Start Docker Desktop and retry.");
  process.exit(1);
}
if (result.status !== 0) process.exit(result.status ?? 1);
if (mode === "up") {
  let healthy = false;
  for (let attempt = 0; attempt < 30; attempt++) {
    try {
      healthy = (
        await fetch("http://127.0.0.1:5080/healthz", {
          signal: AbortSignal.timeout(1500),
        })
      ).ok;
    } catch {
      healthy = false;
    }
    if (healthy) break;
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  if (!healthy) throw new Error("OpenObserve did not become ready; inspect its container logs.");
  console.log("Waiting for OpenObserve trace ingestion to warm up…");
  await waitForLocalIngestion();
  console.log("PostgreSQL, Redis, ClamAV and OpenObserve are ready.");
}
