export const API_URL = import.meta.env.DEV
  ? ((import.meta.env["VITE_DEV_API_URL"] as string | undefined) ??
    "http://localhost:8090")
  : ((import.meta.env["VITE_API_URL"] as string | undefined) ??
    "https://lennapi.itssingularity.com");

export class AccountError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "AccountError";
    this.status = status;
  }
}

export const accountRequest = async <T>(
  path: string,
  method = "GET",
  body?: unknown
): Promise<T> => {
  const response = await fetch(`${API_URL}/v1${path}`, {
    method,
    credentials: "include",
    cache: "no-store",
    headers: {
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      ...(method === "DELETE" && path === "/me"
        ? { "X-Delete-Confirmation": "DELETE" }
        : {}),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(60_000),
  });
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as {
      error?: { message?: string };
    } | null;
    throw new AccountError(
      payload?.error?.message ?? "The request failed. Please try again.",
      response.status
    );
  }
  if (response.status === 204 || response.status === 202) {
    return undefined as T;
  }
  return response.json() as Promise<T>;
};

export const paymentRedirect = (value: unknown) => {
  if (typeof value !== "string") {
    throw new TypeError("Billing link was unavailable.");
  }
  const url = new URL(value);
  if (url.protocol !== "https:") {
    throw new Error("Billing link was invalid.");
  }
  window.location.assign(url.href);
};
