import assert from "node:assert/strict";
import { test } from "node:test";
import { waitForLocalIngestion } from "./openobserve.ts";

test("readiness requires an authenticated real span accepted by the local ingester", async () => {
  let calls = 0;
  await waitForLocalIngestion({ OPENOBSERVE_USER_EMAIL: "dev@test.local", OPENOBSERVE_USER_PASSWORD: "test-only" }, async (url, options) => {
    calls++;
    assert.equal(url, "http://127.0.0.1:5080/api/default/v1/traces");
    assert.equal(options.method, "POST");
    assert.equal(options.headers.Authorization, `Basic ${Buffer.from("dev@test.local:test-only").toString("base64")}`);
    const span = JSON.parse(options.body).resourceSpans[0].scopeSpans[0].spans[0];
    assert.match(span.traceId, /^[a-f0-9]{32}$/);
    assert.match(span.spanId, /^[a-f0-9]{16}$/);
    assert(BigInt(span.endTimeUnixNano) > BigInt(span.startTimeUnixNano));
    return Response.json({});
  });
  assert.equal(calls, 1);
});

test("readiness fails immediately on invalid ingestion credentials", async () => {
  let calls = 0;
  await assert.rejects(waitForLocalIngestion({}, async () => {
    calls++;
    return new Response("private server detail", { status: 401 });
  }), /ingestion authentication failed/);
  assert.equal(calls, 1);
});

test("a reachable collector that rejects the span is not reported as ready", async () => {
  let calls = 0;
  await waitForLocalIngestion({}, async () => {
    calls++;
    return Response.json(calls === 1 ? { partialSuccess: { rejectedSpans: "1" } } : {});
  });
  assert.equal(calls, 2);
});
