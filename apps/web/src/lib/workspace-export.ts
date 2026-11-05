import { accountRequest } from "./account-api";
import { clock, dateLabel, itemTitle, text } from "./workspace";
import type { ItemDetail, WorkspaceItem } from "./workspace";

export type ExportFormat = "md" | "txt" | "json" | "srt" | "vtt";

export const collectHistory = async (
  id: string,
  account: string,
  signal: AbortSignal,
  progress: (count: number) => void
): Promise<ItemDetail> => {
  let result: ItemDetail | undefined;
  let after: string | null = null;
  let bytes = 0;
  const cursors = new Set<string>();
  const ids = new Set<string>();
  do {
    signal.throwIfAborted();
    const params = new URLSearchParams({ account, limit: "20" });
    if (after) {
      params.set("after", after);
    }
    // Each page depends on the cursor returned by the preceding request.
    // eslint-disable-next-line no-await-in-loop
    const page = await accountRequest<ItemDetail>(
      `/workspace/items/${encodeURIComponent(id)}?${params}`,
      "GET",
      undefined,
      signal
    );
    signal.throwIfAborted();
    if (page.record.id !== id) {
      throw new Error("The selected record changed. Please retry.");
    }
    bytes += JSON.stringify(page).length * 2;
    if (!result) {
      result = { ...page, items: [] };
    }
    for (const item of page.items) {
      if (ids.has(item.id)) {
        throw new Error("Saved history changed during export. Please retry.");
      }
      ids.add(item.id);
      result.items.push(item);
    }
    if (
      ids.size > 10_000 ||
      (result.total_items ?? 0) > 10_000 ||
      bytes > 20 * 1024 * 1024
    ) {
      throw new Error(
        "This export exceeds the browser limit of 10,000 entries or 20 MiB."
      );
    }
    progress(ids.size);
    after = page.next;
    if (after && cursors.has(after)) {
      throw new Error("Pagination changed during export. Please retry.");
    }
    if (after) {
      cursors.add(after);
    }
  } while (after);
  if (result.total_items !== undefined && ids.size !== result.total_items) {
    throw new Error(
      "Saved history changed during export. Please retry after sync finishes."
    );
  }
  return result;
};

export const speakerName = (item: WorkspaceItem, extras: WorkspaceItem[]) => {
  if (item.kind !== "transcript") {
    return item.data["role"] === "assistant" ? "Lennette" : "You";
  }
  const key = text(item.data["speaker"]);
  const named = extras.find(
    (extra) =>
      extra.kind === "speaker" &&
      [
        key,
        `${text(item.data["who"])}:${key}`,
        text(item.data["who"]),
      ].includes(text(extra.data["speaker_key"]))
  );
  if (text(named?.data["name"])) {
    return text(named?.data["name"]);
  }
  if (item.data["who"] === "me") {
    return "You";
  }
  return key ? `Speaker ${Number(key) + 1}` : text(item.data["who"], "Speaker");
};

const subtitleTime = (raw: unknown, separator: string) => {
  const millis = Math.round(
    Math.max(0, Math.min(Number(raw) || 0, 604_800)) * 1000
  );
  return `${String(Math.floor(millis / 3_600_000)).padStart(2, "0")}:${String(Math.floor(millis / 60_000) % 60).padStart(2, "0")}:${String(Math.floor(millis / 1000) % 60).padStart(2, "0")}${separator}${String(millis % 1000).padStart(3, "0")}`;
};

const subtitles = (detail: ItemDetail, format: "srt" | "vtt") => {
  const separator = format === "srt" ? "," : ".";
  const cues = detail.items.toReversed().map((item, index) => {
    const start = Math.max(0, Number(item.data["start_time"]) || 0);
    const end = Math.max(
      start + 0.001,
      Number(item.data["end_time"]) || start + 1
    );
    const content = text(item.data["text"])
      .replaceAll(/[<>]/gu, "")
      .replaceAll(/\r?\n\s*\n/gu, "\n")
      .replaceAll("-->", "→");
    return `${index + 1}\n${subtitleTime(start, separator)} --> ${subtitleTime(end, separator)}\n${speakerName(item, detail.extras)}: ${content}`;
  });
  return `${format === "vtt" ? "WEBVTT\n\n" : ""}${cues.join("\n\n")}\n`;
};

export const serializeHistory = (detail: ItemDetail, format: ExportFormat) => {
  if (format === "json") {
    return JSON.stringify(
      { ...detail, next: null, items: detail.items.toReversed() },
      null,
      2
    );
  }
  if (format === "srt" || format === "vtt") {
    return subtitles(detail, format);
  }
  const title = itemTitle(detail.record);
  const recap = detail.extras.find((item) => item.kind === "recap");
  const heading = format === "md" ? `# ${title}` : title;
  const summary = recap
    ? `\n\nMeeting recap\n\n${text(recap.data["content"])}`
    : "";
  const lines = detail.items.toReversed().map((item) => {
    const stamp =
      item.kind === "transcript"
        ? clock(item.data["start_time"])
        : dateLabel(item.data["timestamp"]);
    return `${speakerName(item, detail.extras)} · ${stamp}\n\n${text(item.data["text"] ?? item.data["content"])}`;
  });
  return `${heading}${summary}\n\n${lines.join("\n\n---\n\n")}\n`;
};

export const saveExport = (detail: ItemDetail, format: ExportFormat) => {
  const blob = new Blob([serializeHistory(detail, format)], {
    type:
      format === "json"
        ? "application/json;charset=utf-8"
        : "text/plain;charset=utf-8",
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  const name = Array.from(itemTitle(detail.record), (char) =>
    (char.codePointAt(0) ?? 0) < 32 || '<>:"/\\|?*'.includes(char) ? "_" : char
  )
    .join("")
    .slice(0, 100);
  link.href = url;
  link.download = `${name || "lennette"}.${format}`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
};
