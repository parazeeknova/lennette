import { afterEach, expect, it, vi } from "vitest";

import { accountRequest, AccountError } from "@/lib/account-api";
import { searchResponse } from "@/lib/search-response";

afterEach(() => vi.unstubAllGlobals());

it("accepts durable deletion and sends explicit confirmation with cookies", async () => {
  const fetcher = vi
    .fn()
    .mockResolvedValue(new Response(null, { status: 202 }));
  vi.stubGlobal("fetch", fetcher);
  await expect(accountRequest("/me", "DELETE")).resolves.toBeUndefined();
  expect(fetcher).toHaveBeenCalledWith(
    expect.stringContaining("/v1/me"),
    expect.objectContaining({
      credentials: "include",
      headers: { "X-Delete-Confirmation": "DELETE" },
    })
  );
});

it("preserves structured failures instead of treating them as success", async () => {
  vi.stubGlobal(
    "fetch",
    vi
      .fn()
      .mockResolvedValue(
        Response.json(
          { error: { message: "quota exhausted" } },
          { status: 402 }
        )
      )
  );
  await expect(
    accountRequest("/billing/checkout", "POST", { plan: "managed" })
  ).rejects.toEqual(new AccountError("quota exhausted", 402));
});

it("never caches or indexes the account page", () => {
  const response = searchResponse(
    new Response("account"),
    new Request("https://lennette.itssingularity.com/account"),
    false
  );
  expect(response.headers.get("Cache-Control")).toBe("no-store");
  expect(response.headers.get("X-Robots-Tag")).toContain("noindex");
});
