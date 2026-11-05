import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
  cleanup,
} from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkspaceLive } from "@/components/workspace/use-workspace-live";
import { WorkspacePage } from "@/components/workspace/workspace-page";
import type * as AccountApi from "@/lib/account-api";
import { AccountError, accountRequest } from "@/lib/account-api";
import {
  applyLive,
  emptyLive,
  newerCursor,
  validCursor,
} from "@/lib/workspace";

vi.mock("@/lib/account-api", async (original) => ({
  ...(await original<typeof AccountApi>()),
  accountRequest: vi.fn(),
}));
vi.mock("@tanstack/react-router", () => ({
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
const account = "11111111-1111-4111-8111-111111111111";
const meeting = "22222222-2222-4222-8222-222222222222";
const conversation = "33333333-3333-4333-8333-333333333333";
const frame = (event: Record<string, unknown>, cursor = "1-0") => ({
  type: "live",
  meeting_id: meeting,
  device_id: "desktop",
  emitted_at: Date.now(),
  live_cursor: cursor,
  event,
});
const speech = (content: string, final: boolean) =>
  frame({
    type: "transcript",
    result: {
      channel_index: [0, 2],
      start: 0,
      is_final: final,
      channel: { alternatives: [{ transcript: content }] },
    },
  });

class MockSocket extends EventTarget {
  static CLOSED = 3;
  static instances: MockSocket[] = [];
  readyState = 1;
  readonly url: string;
  constructor(url: string | URL) {
    super();
    this.url = String(url);
    MockSocket.instances.push(this);
  }
  emit(value: unknown) {
    this.dispatchEvent(
      new MessageEvent("message", { data: JSON.stringify(value) })
    );
  }
  close() {
    this.readyState = 3;
    this.dispatchEvent(new Event("close"));
  }
}
const api = vi.mocked(accountRequest);
const client = () =>
  new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
beforeEach(() => {
  MockSocket.instances = [];
  vi.stubGlobal("WebSocket", MockSocket);
  vi.stubGlobal("requestAnimationFrame", (draw: FrameRequestCallback) =>
    setTimeout(() => draw(0), 0)
  );
  vi.stubGlobal("cancelAnimationFrame", clearTimeout);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  api.mockReset();
});

describe("workspace live state", () => {
  it("replaces interim speech and never downgrades a final segment", () => {
    let state = applyLive(emptyLive(), speech("Hel", false));
    state = applyLive(state, speech("Hello", true));
    state = applyLive(state, speech("Hell", false));
    expect(state.transcripts).toHaveLength(1);
    expect(state.transcripts[0]?.content).toBe("Hello");
    expect(state.transcripts[0]?.final).toBe(true);
  });
  it("assembles the right conversation's AI stream and bounds retained memory", () => {
    let state = applyLive(
      emptyLive(),
      frame({
        type: "ai_snapshot",
        stream_id: "answer",
        conversation_id: conversation,
        content: "First",
        prompt: "Question?",
        done: false,
      })
    );
    state = applyLive(
      state,
      frame({
        type: "ai_delta",
        stream_id: "answer",
        conversation_id: conversation,
        delta: { content: " second" },
      })
    );
    expect(state.answers[0]?.content).toBe("First second");
    expect(state.answers[0]?.conversation).toBe(conversation);
    for (let index = 0; index < 20; index += 1) {
      state = applyLive(
        state,
        frame({
          type: "ai_snapshot",
          stream_id: String(index),
          content: "x".repeat(80_000),
        })
      );
    }
    expect(state.answers).toHaveLength(8);
    expect(
      state.answers.every((answer) => answer.content.length <= 65_536)
    ).toBe(true);
  });
  it("marks missing AI chunks as partial instead of joining unrelated text", () => {
    let state = applyLive(
      emptyLive(),
      frame({ type: "ai_delta", stream_id: "gapped", offset: 0, delta: "A" })
    );
    state = applyLive(
      state,
      frame({ type: "ai_delta", stream_id: "gapped", offset: 2, delta: "C" })
    );
    expect(state.answers[0]?.content).toBe("A");
    expect(state.answers[0]?.truncated).toBe(true);
  });
  it("rejects malformed and stale frames and compares large cursors without numeric rounding", () => {
    const initial = emptyLive();
    expect(
      applyLive(initial, {
        ...frame({ type: "meeting_active" }),
        emitted_at: Date.now() - 181_000,
      })
    ).toBe(initial);
    expect(
      applyLive(
        initial,
        frame({ type: "transcript", result: { start: "bad" } })
      )
    ).toBe(initial);
    expect(validCursor("1-2-3")).toBe(false);
    expect(
      newerCursor("18446744073709551615-2", "18446744073709551615-1")
    ).toBe(true);
    expect(newerCursor("4-0", "4-0")).toBe(false);
  });
});

describe("workspace access and mobile navigation", () => {
  it("does not fetch private history before a valid browser session", async () => {
    api.mockRejectedValue(new AccountError("Sign in", 401));
    const cache = client();
    render(
      <QueryClientProvider client={cache}>
        <WorkspacePage />
      </QueryClientProvider>
    );
    expect(
      await screen.findByText("Sign in to see your synced workspace.")
    ).toBeInTheDocument();
    expect(api.mock.calls.map((call) => call[0])).toEqual(["/me"]);
    expect(MockSocket.instances).toHaveLength(0);
    cache.clear();
  });
  it("opens a saved conversation, escapes message HTML, returns to the mobile list and clears data on revocation", async () => {
    const dangerous = '<img src=x onerror="window.stolen=true">';
    api.mockImplementation((path) => {
      if (path === "/me") {
        return Promise.resolve({
          id: account,
          name: "Sam",
          email: "sam@example.com",
        });
      }
      if (path.startsWith("/workspace/items/")) {
        return Promise.resolve({
          record: {
            id: conversation,
            kind: "conversation",
            data: { title: "Project plan" },
          },
          items: [
            {
              id: "message",
              kind: "message",
              data: { role: "assistant", content: dangerous, timestamp: 1 },
            },
          ],
          next: null,
          extras: [],
          files: [],
        });
      }
      return Promise.resolve({
        items: [
          {
            id: conversation,
            kind: "conversation",
            data: { title: "Project plan" },
          },
        ],
        next: null,
      });
    });
    const cache = client();
    render(
      <QueryClientProvider client={cache}>
        <WorkspacePage />
      </QueryClientProvider>
    );
    fireEvent.click(
      await screen.findByRole("button", { name: /Project plan/u })
    );
    expect(await screen.findByText(dangerous)).toBeInTheDocument();
    expect(document.querySelector('img[src="x"]')).toBeNull();
    expect(screen.getByRole("region", { name: "Saved items" })).toHaveClass(
      "hidden"
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Back to saved items" })
    );
    expect(screen.queryByText(dangerous)).not.toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Saved items" })).not.toHaveClass(
      "hidden"
    );
    act(() => MockSocket.instances[0]?.emit({ type: "session_revoked" }));
    expect(
      await screen.findByText("Your session ended. Sign in again to continue.")
    ).toBeInTheDocument();
    await waitFor(() =>
      expect(cache.getQueriesData({ queryKey: ["workspace"] })).toHaveLength(0)
    );
    cache.clear();
  });
});

describe("workspace reconnect transport", () => {
  it("resumes its cursor, ignores duplicate replay and stops reconnecting after sign-out", async () => {
    vi.useFakeTimers();
    vi.spyOn(Math, "random").mockReturnValue(0);
    api.mockResolvedValue({ id: account });
    const cache = client();
    const expired = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={cache}>{children}</QueryClientProvider>
    );
    const { result, unmount } = renderHook(
      () => useWorkspaceLive(account, expired),
      { wrapper }
    );
    const [first] = MockSocket.instances;
    expect(first?.url).toContain(`account=${account}`);
    expect(first?.url).not.toMatch(/token|bearer|secret/iu);
    await act(async () => {
      first?.emit({ type: "ready", server_time: Date.now() });
      first?.emit(
        frame(
          { type: "ai_delta", stream_id: "answer", delta: { content: "A" } },
          "100-0"
        )
      );
      await vi.advanceTimersByTimeAsync(30);
    });
    expect(result.current.live.answers[0]?.content).toBe("A");
    await act(async () => {
      first?.close();
      await vi.advanceTimersByTimeAsync(600);
    });
    const [, second] = MockSocket.instances;
    expect(second?.url).toContain("live_after=100-0");
    await act(async () => {
      second?.emit({
        type: "replay",
        frames: [
          frame(
            { type: "ai_delta", stream_id: "answer", delta: { content: "A" } },
            "100-0"
          ),
          frame(
            { type: "ai_delta", stream_id: "answer", delta: { content: "B" } },
            "101-0"
          ),
        ],
      });
      await vi.advanceTimersByTimeAsync(30);
    });
    expect(result.current.live.answers[0]?.content).toBe("AB");
    await act(async () => {
      second?.emit({ type: "session_revoked" });
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(expired).toHaveBeenCalledOnce();
    expect(MockSocket.instances).toHaveLength(2);
    unmount();
    cache.clear();
  });
});
