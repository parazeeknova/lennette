import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MeetingAnalytics } from "@/components/workspace/meeting-analytics";
import { accountRequest } from "@/lib/account-api";
import type { ItemDetail, WorkspaceItem } from "@/lib/workspace";
import {
  collectHistory,
  serializeHistory,
  speakerName,
} from "@/lib/workspace-export";

vi.mock("@/lib/account-api", () => ({ accountRequest: vi.fn() }));
const meeting: WorkspaceItem = {
  id: "meeting",
  kind: "meeting",
  data: { title: "Planning" },
};
const line = (id: string, start: number): WorkspaceItem => ({
  id,
  kind: "transcript",
  data: {
    who: "them",
    speaker: 0,
    text: `Words ${id}`,
    start_time: start,
    end_time: start + 1.5,
  },
});
const speaker: WorkspaceItem = {
  id: "speaker",
  kind: "speaker",
  data: { speaker_key: "them:0", name: "Sam" },
};
const page = (
  items: WorkspaceItem[],
  next: string | null = null
): ItemDetail => ({
  record: meeting,
  items,
  next,
  extras: [speaker],
  files: [],
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("complete history export", () => {
  it("fetches all pages with the expected account, ignoring the visible search", async () => {
    vi.mocked(accountRequest)
      .mockResolvedValueOnce({
        ...page([line("new", 10)], "next-cursor"),
        total_items: 2,
      })
      .mockResolvedValueOnce(page([line("old", 0)]));
    const progress = vi.fn();
    const data = await collectHistory(
      "meeting",
      "account-a",
      new AbortController().signal,
      progress
    );
    expect(data.items).toHaveLength(2);
    expect(accountRequest).toHaveBeenNthCalledWith(
      2,
      expect.stringContaining("account=account-a&limit=20&after=next-cursor"),
      "GET",
      undefined,
      expect.any(AbortSignal)
    );
    expect(serializeHistory(data, "txt").indexOf("Words old")).toBeLessThan(
      serializeHistory(data, "txt").indexOf("Words new")
    );
    expect(JSON.parse(serializeHistory(data, "json")).extras[0].data.name).toBe(
      "Sam"
    );
    expect(progress).toHaveBeenLastCalledWith(2);
  });
  it("rejects failed pages, duplicate cursors and incomplete exports", async () => {
    vi.mocked(accountRequest)
      .mockResolvedValueOnce(page([line("new", 10)], "cursor"))
      .mockRejectedValueOnce(new Error("offline"));
    await expect(
      collectHistory("meeting", "a", new AbortController().signal, vi.fn())
    ).rejects.toThrow("offline");
    vi.mocked(accountRequest)
      .mockResolvedValueOnce(page([line("new", 10)], "cursor"))
      .mockResolvedValueOnce(page([line("old", 0)], "cursor"));
    await expect(
      collectHistory("meeting", "a", new AbortController().signal, vi.fn())
    ).rejects.toThrow("Pagination changed");
    vi.mocked(accountRequest).mockResolvedValueOnce({
      ...page([]),
      total_items: 2,
    });
    await expect(
      collectHistory("meeting", "a", new AbortController().signal, vi.fn())
    ).rejects.toThrow("history changed");
  });
  it("honors cancellation before fetching and rejects oversized archives", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      collectHistory("meeting", "a", controller.signal, vi.fn())
    ).rejects.toThrow();
    expect(accountRequest).not.toHaveBeenCalled();
    vi.mocked(accountRequest).mockResolvedValueOnce({
      ...page([]),
      total_items: 10_001,
    });
    await expect(
      collectHistory("meeting", "a", new AbortController().signal, vi.fn())
    ).rejects.toThrow("browser limit");
  });
  it("exports timed subtitles in chronological order with synced speaker names", () => {
    const detail = page([line("later", 60.25), line("first", 0)]);
    expect(speakerName(line("later", 60.25), detail.extras)).toBe("Sam");
    expect(serializeHistory(detail, "srt")).toContain(
      "1\n00:00:00,000 --> 00:00:01,500\nSam: Words first"
    );
    expect(serializeHistory(detail, "vtt")).toContain(
      "WEBVTT\n\n1\n00:00:00.000"
    );
    expect(serializeHistory(detail, "srt")).toContain(
      "00:01:00,250 --> 00:01:01,750"
    );
  });
});

describe("saved meeting analytics", () => {
  it("renders the synced timeline, coverage and stale warning, and searches evidence", () => {
    const search = vi.fn();
    const analysis: WorkspaceItem = {
      id: "analysis",
      kind: "analysis",
      data: {
        status: "ready",
        source_lines: 2,
        content: JSON.stringify({
          sampled_lines: 2,
          total_lines: 2,
          clipped_lines: true,
          segments: [
            {
              start: 0,
              end: 10,
              assessment: {
                tone: "positive",
                reason: "Expressed enthusiasm",
                evidence: [{ line_id: "one", quote: "Great progress" }],
              },
            },
            {
              start: 10,
              end: 20,
              assessment: {
                tone: "negative",
                reason: "Expressed concern",
                evidence: [{ line_id: "two", quote: "This worries me" }],
              },
            },
          ],
        }),
      },
    };
    render(
      <MeetingAnalytics
        analysis={analysis}
        total={3}
        duration={20}
        search={search}
      />
    );
    expect(screen.getAllByText("Mixed")[0]).toBeVisible();
    expect(screen.getByText(/transcript has changed/u)).toBeVisible();
    expect(screen.getByText(/2 sampled of 2 lines/u)).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: /0:10 to 0:20: Negative/u })
    );
    expect(screen.getByText("Expressed concern")).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: /Find supporting quote/u })
    );
    expect(search).toHaveBeenCalledWith("This worries me");
  });
  it("does not fabricate analytics when no saved or readable report exists", () => {
    render(
      <MeetingAnalytics
        analysis={undefined}
        total={0}
        duration={0}
        search={vi.fn()}
      />
    );
    expect(screen.getByText(/No saved assessment yet/u)).toBeVisible();
    expect(
      screen.queryByRole("group", { name: /Sentiment timeline/u })
    ).not.toBeInTheDocument();
  });
});
