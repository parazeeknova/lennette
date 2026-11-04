import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const root = fileURLToPath(new URL("../", import.meta.url));
const lease = fileURLToPath(new URL(`../target/dev-${randomUUID()}.lease`, import.meta.url));
mkdirSync(new URL("../target/", import.meta.url), { recursive: true });
const children = new Set();
let stopping = false;
let heartbeat;
let infraStarted = false;
function run(args) {
  const child = spawn(process.execPath, args, {
    cwd: root,
    env: { ...process.env, LENNETTE_DEV_LEASE: lease },
    stdio: ["ignore", "inherit", "inherit"],
  });
  children.add(child);
  child.on("exit", () => children.delete(child));
  return child;
}
function done(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code) => resolve(code));
  });
}
async function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  clearInterval(heartbeat);
  rmSync(lease, { force: true });
  console.log("\nClosing Lennette development processes…");
  // Native apps observe lease removal, flush capture and shut down before escalation.
  for (const child of children) if (process.platform !== "win32") child.kill("SIGTERM");
  const deadline = Date.now() + 20_000;
  while (children.size && Date.now() < deadline)
    await new Promise((resolve) => setTimeout(resolve, 100));
  for (const child of children) {
    if (process.platform === "win32")
      spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
        stdio: "ignore",
        windowsHide: true,
      });
    else child.kill("SIGKILL");
  }
  if (infraStarted) {
    const infra = spawn(process.execPath, ["scripts/infra.mjs", "down"], {
      cwd: root,
      stdio: "inherit",
    });
    await done(infra);
  }
  process.exitCode = code;
  process.stdin.pause();
}
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => void stop(0));
// Some attached terminals forward Ctrl+C as input rather than a console event.
process.stdin.on("data", (chunk) => {
  if (chunk.includes(3)) void stop(0);
});
try {
  for (const port of [8080, 8090]) {
    await new Promise((resolve, reject) => {
      const probe = createServer();
      probe.once("error", () =>
        reject(
          new Error(
            `Port ${port} is already in use. Close the previous dev session before starting another.`,
          ),
        ),
      );
      probe.listen(port, () => probe.close(resolve));
    });
  }
  writeFileSync(lease, "running");
  heartbeat = setInterval(() => writeFileSync(lease, "running"), 1000);
  infraStarted = true;
  const setup = run(["scripts/dev-setup.mjs"]);
  const setupCode = await done(setup);
  if (setupCode !== 0) throw new Error("Development setup failed.");
  if (!stopping) {
    const apps = ["api", "web", "desktop"].map((app) =>
      run(["run", "--cwd", `apps/${app}`, "dev"]),
    );
    for (const child of apps)
      child.once("exit", (code) => {
        if (!stopping) void stop(code ?? 1);
      });
  }
} catch (error) {
  console.error(error.message);
  await stop(1);
}
