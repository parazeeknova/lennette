import { spawn, spawnSync } from "node:child_process";
import { developmentEnv } from "./dev-env.ts";

const root = new URL("../", import.meta.url);
const env = developmentEnv(new URL("apps/api/", root), root);
if (env.DODO_API_URL !== "https://test.dodopayments.com/") {
  throw new Error("Local webhook forwarding requires Dodo Test Mode.");
}
if (!env.DODO_API_KEY) {
  throw new Error("Add DODO_API_KEY to root .env.local before starting forwarding.");
}
const endpoint = new URL("/v1/billing/webhook", env.API_PUBLIC_URL).href;
let child;
let timer;
let stopping = false;
let failures = 0;

function connect() {
  const started = Date.now();
  let permissionError = false;
  child = spawn("dodo", ["wh", "listen", endpoint], {
    env: {
      ...env,
      DODO_PAYMENTS_API_KEY: env.DODO_API_KEY,
      DODO_PAYMENTS_ENVIRONMENT: "test_mode",
    },
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.on("data", (chunk) => process.stdout.write(chunk));
  child.stderr.on("data", (chunk) => {
    if (
      /INSUFFICIENT_PERMISSION|Please login|401.*Unauthorized|403.*permission/iu.test(
        chunk.toString(),
      )
    )
      permissionError = true;
    process.stderr.write(chunk);
  });
  child.on("error", (error) => {
    stopping = true;
    console.error(
      error.code === "ENOENT"
        ? "Dodo CLI was not found. Install it with: bun install -g dodopayments-cli"
        : "Could not start the Dodo CLI.",
    );
    process.exitCode = 1;
  });
  child.on("close", () => {
    if (stopping) return;
    if (permissionError) {
      console.error(
        "Webhook credentials need attention. Check DODO_API_KEY and webhook permissions in Test Mode.",
      );
      process.exitCode = 1;
      return;
    }
    if (Date.now() - started >= 60_000) failures = 0;
    const delay =
      Math.min(30_000, 1_000 * 2 ** Math.min(failures++, 5)) + Math.floor(Math.random() * 250);
    console.log(`Webhook forwarder disconnected. Reconnecting in ${(delay / 1000).toFixed(1)}s…`);
    timer = setTimeout(connect, delay);
  });
}

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    stopping = true;
    clearTimeout(timer);
    if (child?.pid && child.exitCode === null) {
      if (process.platform === "win32") {
        spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
          stdio: "ignore",
          windowsHide: true,
        });
      } else {
        child.kill(signal);
      }
    }
    process.exit(signal === "SIGINT" ? 130 : 143);
  });
}
console.log(`Forwarding Dodo Test Mode webhooks to ${endpoint}`);
connect();
