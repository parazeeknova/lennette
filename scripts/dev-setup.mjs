import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const api = fileURLToPath(new URL("../apps/api/", import.meta.url));

function run(command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
  if (result.error) {
    console.error(`Could not start ${command}: ${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(
      command === "docker"
        ? "Local databases could not start. Start Docker Desktop and retry."
        : "API database migration failed; check apps/api/.env.development and root .env.local."
    );
    process.exit(result.status ?? 1);
  }
}

run(process.execPath, ["run", "infra:up"]);
run(process.execPath, ["run", "migrate"], api);
