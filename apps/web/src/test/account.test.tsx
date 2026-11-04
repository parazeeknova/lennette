import type * as Router from "@tanstack/react-router";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type * as AccountApi from "@/lib/account-api";
import { AccountError } from "@/lib/account-api";
import { AccountPage } from "@/routes/account";

const { request } = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("@tanstack/react-router", async (original) => ({
  ...(await original<typeof Router>()),
  Link: ({
    to,
    children,
    className,
  }: {
    to: string;
    children: ReactNode;
    className?: string;
  }) => (
    <a href={to} className={className}>
      {children}
    </a>
  ),
}));
vi.mock("@/lib/account-api", async (original) => ({
  ...(await original<typeof AccountApi>()),
  accountRequest: request,
}));
vi.mock("@/components/page-shell", () => ({
  PageShell: ({ children }: { children: ReactNode }) => <main>{children}</main>,
}));
class MockEvents {
  static current: MockEvents;
  listeners = new Map<string, () => void>();
  constructor() {
    MockEvents.current = this;
  }
  // EventSource test double stores event callbacks for explicit delivery.
  // eslint-disable-next-line promise/prefer-await-to-callbacks
  addEventListener(name: string, callback: () => void) {
    this.listeners.set(name, callback);
  }
  close() {
    this.listeners.clear();
  }
}
const usage = {
  subscription: {
    plan: "managed",
    status: "active",
    period_end: "2026-12-04T00:00:00Z",
  },
  limits: {
    ai_microdollars: 2_000_000,
    transcription_seconds: 18_000,
    storage_bytes: 1e10,
  },
  ai_used_microdollars: 500_000,
  transcription_seconds: 3600,
  storage_bytes: 2e9,
};
beforeEach(() => {
  vi.stubGlobal("EventSource", MockEvents);
  request.mockReset();
  request.mockImplementation((path: string) => {
    if (path === "/me") {
      return Promise.resolve({
        id: "user",
        email: "user@example.test",
        name: "Meeting User",
      });
    }
    if (path === "/billing/usage") {
      return Promise.resolve(usage);
    }
    if (path === "/devices") {
      return Promise.resolve([
        {
          session_id: "device",
          name: "Laptop",
          current: true,
          created_at: "2026-11-04T00:00:00Z",
        },
      ]);
    }
    return Promise.resolve(undefined);
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it("shows sign-in choices when the session is missing", async () => {
  request.mockRejectedValue(new AccountError("authentication required", 401));
  render(<AccountPage />);
  expect(
    await screen.findByRole("button", { name: "Continue with Google" })
  ).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Continue with GitHub" })
  ).toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(request.mock.calls.map(([path]) => path)).toEqual(["/me"]);
  expect(
    screen.getByRole("heading", { name: "AI, without the setup" })
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: "Pick up on another device" })
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: "Keep the context close" })
  ).toBeInTheDocument();
});

it("offers recovery from an unavailable API without showing raw fetch errors", async () => {
  request.mockRejectedValueOnce(new TypeError("Failed to fetch"));
  render(<AccountPage />);
  expect(
    await screen.findByText(/Account services are temporarily unavailable/u)
  ).toBeInTheDocument();
  expect(screen.queryByText("Failed to fetch")).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Continue with Google" })
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Meeting User")).toBeInTheDocument();
  expect(
    screen.queryByText(/Account services are temporarily unavailable/u)
  ).not.toBeInTheDocument();
});

it("renders usage, payment management and devices, and refreshes on server events", async () => {
  render(<AccountPage />);
  expect(await screen.findByText("Meeting User")).toBeInTheDocument();
  expect(screen.getByText("1,500 credits remaining")).toBeInTheDocument();
  expect(screen.getByText("8 GB remaining")).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "Manage subscription & payment method" })
  ).toBeInTheDocument();
  const calls = request.mock.calls.length;
  MockEvents.current.listeners.get("changed")?.();
  await waitFor(() => expect(request.mock.calls.length).toBeGreaterThan(calls));
  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  await waitFor(() =>
    expect(request).toHaveBeenCalledWith("/devices/device", "DELETE")
  );
});

it("requires typed confirmation before deleting and handles session expiry", async () => {
  render(<AccountPage />);
  const button = await screen.findByRole("button", {
    name: "Permanently delete account",
  });
  expect(button).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Type DELETE to confirm"), {
    target: { value: "DELETE" },
  });
  expect(button).toBeEnabled();
  MockEvents.current.listeners.get("expired")?.();
  expect(
    await screen.findByText("Your session expired. Sign in again.")
  ).toBeInTheDocument();
});
