import { spawn } from "node:child_process";
import { createWriteStream, readFileSync } from "node:fs";
import { createServer } from "node:net";
const log = new URL("../target/dev-lifecycle.log", import.meta.url);
const output = createWriteStream(log);
const child = spawn(process.execPath, ["--no-env-file", "scripts/dev.mjs"], {
  stdio: ["pipe", "pipe", "pipe"],
});
child.stdout.pipe(output, { end: false });
child.stderr.pipe(output, { end: false });
const exited = new Promise((resolve, reject) => {
  child.on("exit", resolve);
  child.on("error", reject);
});
try {
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 500));
    const text = readFileSync(log, "utf8");
    if (
      text.includes("overlay window ready") &&
      text.includes("API listening") &&
      text.includes("VITE v")
    )
      break;
    if (child.exitCode !== null)
      throw new Error("Dev startup exited early; inspect target/dev-lifecycle.log");
  }
  const text = readFileSync(log, "utf8");
  if (
    !text.includes("overlay window ready") ||
    !text.includes("API listening") ||
    !text.includes("VITE v")
  )
    throw new Error("Dev startup timed out");
  console.log("PASS all three apps started; requesting Ctrl+C shutdown.");
  child.stdin.write(Buffer.from([3]));
  const code = await Promise.race([
    exited,
    new Promise((_, reject) =>
      setTimeout(() => reject(new Error("Shutdown timed out")), 30_000).unref(),
    ),
  ]);
  if (code !== 0) throw new Error(`Dev exited ${code}`);
  for (const port of [8080, 8090]) {
    await new Promise((resolve, reject) => {
      const probe = createServer();
      probe.once("error", reject);
      probe.listen(port, () => probe.close(resolve));
    });
  }
  console.log("PASS supervisor exited cleanly after Ctrl+C.");
} finally {
  if (child.exitCode === null) child.stdin.write(Buffer.from([3]));
  output.end();
}
