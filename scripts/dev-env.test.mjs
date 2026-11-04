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

test("local OpenObserve overrides derive ingestion auth without replacing explicit headers", () => {
  const directory = mkdtempSync(join(tmpdir(), "lennette-observe-env-"));
  try {
    mkdirSync(join(directory, "api"));
    writeFileSync(
      join(directory, "api", ".env.development"),
      "OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:5080/api/default\nOTEL_EXPORTER_OTLP_HEADERS=default\n"
    );
    writeFileSync(
      join(directory, ".env.local"),
      "OPENOBSERVE_USER_EMAIL=test@example.test\nOPENOBSERVE_USER_PASSWORD=local-test-only\n"
    );
    const root = pathToFileURL(`${directory}/`);
    const result = developmentEnv(new URL("api/", root), root, {});
    expectHeader(result.OTEL_EXPORTER_OTLP_HEADERS);
    writeFileSync(
      join(directory, ".env.local"),
      "OPENOBSERVE_USER_EMAIL=test@example.test\nOTEL_EXPORTER_OTLP_HEADERS=explicit\n"
    );
    assert.equal(
      developmentEnv(new URL("api/", root), root, {})
        .OTEL_EXPORTER_OTLP_HEADERS,
      "explicit"
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

function expectHeader(value) {
  const encoded = value.split("Basic%20")[1].split(",")[0];
  assert.equal(
    Buffer.from(encoded, "base64").toString(),
    "test@example.test:local-test-only"
  );
}

test("development origins override shared production URLs and support explicit dev overrides", () => {
  const directory = mkdtempSync(join(tmpdir(), "lennette-origin-env-"));
  try {
    mkdirSync(join(directory, "api"));
    writeFileSync(join(directory, "api", ".env.development"), "DEV_WEBSITE_URL=http://localhost:8080\nDEV_API_PUBLIC_URL=http://localhost:8090\n");
    writeFileSync(join(directory, ".env.local"), "WEBSITE_URL=https://lennette.itssingularity.com\nAPI_PUBLIC_URL=https://lennapi.itssingularity.com\n");
    const root = pathToFileURL(`${directory}/`);
    const app = new URL("api/", root);
    assert.equal(developmentEnv(app, root, {}).WEBSITE_URL, "http://localhost:8080");
    assert.equal(developmentEnv(app, root, {}).API_PUBLIC_URL, "http://localhost:8090");
    writeFileSync(join(directory, ".env.local"), "DEV_WEBSITE_URL=http://127.0.0.1:8080\n");
    assert.equal(developmentEnv(app, root, {}).WEBSITE_URL, "http://127.0.0.1:8080");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
