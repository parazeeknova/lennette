import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

import { developmentEnv } from "./dev-env.mjs";

test("app defaults work without a local file; root overrides stay scoped", () => {
  const directory = mkdtempSync(join(tmpdir(), "lennette-env-"));
  try {
    for (const app of ["api", "web"]) {
      mkdirSync(join(directory, app));
      writeFileSync(
        join(directory, app, ".env.development"),
        `APP=${app}\nVALUE=default\n`
      );
    }
    const root = pathToFileURL(`${directory}/`);
    const api = new URL("api/", root);
    const web = new URL("web/", root);
    assert.equal(developmentEnv(api, root, {}).VALUE, "default");
    assert.equal(developmentEnv(api, root, { VALUE: "shell" }).VALUE, "shell");
    writeFileSync(
      join(directory, ".env.local"),
      'VALUE=override\nPRIVATE_KEY="local test value"\n'
    );
    const result = developmentEnv(api, root, { VALUE: "shell" });
    assert.equal(result.VALUE, "override");
    assert.equal(result.PRIVATE_KEY, "local test value");
    assert.equal(result.APP, "api");
    assert.equal(result.NODE_ENV, "development");
    assert.equal(developmentEnv(web, root, {}).APP, "web");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
