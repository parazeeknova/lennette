import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseEnv } from "node:util";

function readEnv(path, optional = false) {
  try {
    return parseEnv(readFileSync(path, "utf8"));
  } catch (error) {
    if (optional && error.code === "ENOENT") return {};
    throw error;
  }
}

export function developmentEnv(
  appDirectory,
  rootDirectory,
  inherited = process.env
) {
  const local = readEnv(new URL(".env.local", rootDirectory), true);
  const env = {
    ...readEnv(new URL(".env.development", appDirectory)),
    ...inherited,
    ...local,
    NODE_ENV: "development",
  };
  if (
    !local.OTEL_EXPORTER_OTLP_HEADERS &&
    (local.OPENOBSERVE_USER_EMAIL ||
      local.OPENOBSERVE_USER_PASSWORD ||
      inherited.OPENOBSERVE_USER_EMAIL ||
      inherited.OPENOBSERVE_USER_PASSWORD) &&
    env.OTEL_EXPORTER_OTLP_ENDPOINT === "http://127.0.0.1:5080/api/default"
  ) {
    const credentials = `${env.OPENOBSERVE_USER_EMAIL ?? "dev@lennette.local"}:${env.OPENOBSERVE_USER_PASSWORD ?? "LocalOnly-Observe123!"}`;
    env.OTEL_EXPORTER_OTLP_HEADERS = `Authorization=Basic%20${Buffer.from(credentials).toString("base64")},stream-name=lennette_api`;
  }
  return env;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const [app, ...command] = process.argv.slice(2);
  if (!["api", "web", "desktop"].includes(app) || command.length === 0) {
    throw new Error("Usage: dev-env.mjs <api|web|desktop> <command> [args...]");
  }
  const root = new URL("../", import.meta.url);
  const directory = new URL(`apps/${app}/`, root);
  const child = spawn(process.execPath, ["--no-env-file", "run", ...command], {
    cwd: fileURLToPath(directory),
    env: developmentEnv(directory, root),
    stdio: "inherit",
  });
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => child.kill(signal));
  }
  child.on("error", (error) => {
    console.error(error.message);
    process.exit(1);
  });
  child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
}
