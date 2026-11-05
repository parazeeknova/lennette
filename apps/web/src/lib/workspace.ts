export type ItemKind = "conversation" | "meeting" | "file" | "attachment";
export interface WorkspaceItem {
  id: string;
  kind:
    | ItemKind
    | "message"
    | "transcript"
    | "recap"
    | "context"
    | "speaker"
    | "analysis";
  data: Record<string, unknown>;
}
export interface ItemPage {
  items: WorkspaceItem[];
  next: string | null;
}
export interface ItemDetail extends ItemPage {
  record: WorkspaceItem;
  extras: WorkspaceItem[];
  files: WorkspaceItem[];
}
export const text = (value: unknown, fallback = ""): string =>
  typeof value === "string" || typeof value === "number"
    ? String(value)
    : fallback;
export const itemTitle = (item: WorkspaceItem) =>
  text(
    item.data["title"] || item.data["name"],
    item.kind === "meeting" ? "Untitled meeting" : "Untitled"
  );
export const dateLabel = (value: unknown) => {
  if (!value) {
    return "";
  }
  const raw = text(value);
  const numeric = /^\d+$/u.test(raw) ? Number(raw) : Number.NaN;
  let normalized = raw;
  if (/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/u.test(raw)) {
    normalized = `${raw.replace(" ", "T")}Z`;
  }
  const date = new Date(
    Number.isNaN(numeric) ? normalized : numeric * (numeric < 1e12 ? 1000 : 1)
  );
  return Number.isNaN(date.getTime())
    ? ""
    : date.toLocaleString(undefined, {
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      });
};
export const clock = (seconds: unknown) => {
  const n = Math.max(0, Math.floor(Number(seconds) || 0));
  return `${Math.floor(n / 60)}:${String(n % 60).padStart(2, "0")}`;
};

export interface LiveTranscript {
  key: string;
  meeting: string;
  device: string;
  channel: number;
  start: number;
  content: string;
  final: boolean;
  at: number;
}
export interface LiveAnswer {
  id: string;
  meeting: string | null;
  conversation: string | null;
  receivedBytes: number;
  prompt: string;
  content: string;
  done: boolean;
  complete: boolean;
  truncated: boolean;
  at: number;
}
export interface LiveMeeting {
  id: string;
  device: string;
  active: boolean;
  at: number;
}
export interface LiveState {
  transcripts: LiveTranscript[];
  answers: LiveAnswer[];
  meetings: LiveMeeting[];
}
export const emptyLive = (): LiveState => ({
  transcripts: [],
  answers: [],
  meetings: [],
});
export const object = (value: unknown): Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
export const validCursor = (value: unknown): value is string =>
  typeof value === "string" && /^\d{1,20}-\d{1,20}$/u.test(value);
export const newerCursor = (next: string, previous: string) => {
  const [a = 0n, b = 0n] = next.split("-").map(BigInt);
  const [c = 0n, d = 0n] = previous.split("-").map(BigInt);
  return a > c || (a === c && b > d);
};

interface FrameContext {
  meeting: string | null;
  device: string;
  at: number;
  event: Record<string, unknown>;
}
const applyPresence = (state: LiveState, frame: FrameContext): LiveState => {
  if (!frame.meeting) {
    return state;
  }
  const id = frame.meeting;
  const prior = state.meetings.find(
    (m) => m.id === id && m.device === frame.device
  );
  if (prior && prior.at > frame.at) {
    return state;
  }
  return {
    ...state,
    meetings: [
      ...state.meetings.filter((m) => m.id !== id || m.device !== frame.device),
      {
        id,
        device: frame.device,
        active: frame.event["type"] === "meeting_active",
        at: frame.at,
      },
    ].slice(-12),
  };
};
const applyTranscript = (state: LiveState, frame: FrameContext): LiveState => {
  if (!frame.meeting) {
    return state;
  }
  const result = object(frame.event["result"]);
  const channelIndex = result["channel_index"];
  const channel = Number(Array.isArray(channelIndex) ? channelIndex[0] : 0);
  const { alternatives } = object(result["channel"]);
  const content = text(
    object(Array.isArray(alternatives) ? alternatives[0] : null)["transcript"]
  ).slice(0, 16_384);
  const start = Number(result["start"] ?? 0);
  if (!content || !Number.isFinite(start) || !Number.isFinite(channel)) {
    return state;
  }
  const key = [
    frame.meeting,
    frame.device,
    text(frame.event["capture_id"]),
    channel,
    start,
  ].join(":");
  const prior = state.transcripts.find((t) => t.key === key);
  if (
    prior &&
    (prior.at > frame.at || (prior.final && result["is_final"] !== true))
  ) {
    return state;
  }
  const line: LiveTranscript = {
    key,
    meeting: frame.meeting,
    device: frame.device,
    channel,
    start,
    content,
    final: result["is_final"] === true,
    at: frame.at,
  };
  return {
    ...state,
    transcripts: [
      ...state.transcripts.filter((t) => t.key !== key),
      line,
    ].slice(-200),
  };
};
const missingCompletion = (event: Record<string, unknown>, received: number) =>
  event["type"] === "ai_done" &&
  typeof event["total_bytes"] === "number" &&
  event["total_bytes"] !== received;

const answerProgress = (
  prior: LiveAnswer | undefined,
  event: Record<string, unknown>
) => {
  let content = prior?.content ?? "";
  let receivedBytes = prior?.receivedBytes ?? 0;
  let missing = false;
  if (event["type"] === "ai_snapshot") {
    content = text(event["content"]);
    receivedBytes = Number(
      event["received_bytes"] ?? new TextEncoder().encode(content).length
    );
  }
  if (event["type"] === "ai_delta") {
    const delta =
      typeof event["delta"] === "string"
        ? event["delta"]
        : text(object(event["delta"])["content"]);
    missing =
      typeof event["offset"] === "number" && event["offset"] !== receivedBytes;
    receivedBytes =
      Number(event["offset"] ?? receivedBytes) +
      new TextEncoder().encode(delta).length;
    if (!prior?.truncated && !missing) {
      content += delta;
    }
  }

  if (missingCompletion(event, receivedBytes)) {
    missing = true;
  }
  return {
    content,
    receivedBytes,
    truncated:
      prior?.truncated === true ||
      missing ||
      event["truncated"] === true ||
      content.length > 65_536,
  };
};
const applyAnswer = (state: LiveState, frame: FrameContext): LiveState => {
  const { event } = frame;
  const id = text(event["stream_id"]);
  if (!id || id.length > 80) {
    return state;
  }
  const prior = state.answers.find((a) => a.id === id);
  if (prior && prior.at > frame.at) {
    return state;
  }
  const { content, receivedBytes, truncated } = answerProgress(prior, event);
  const answer: LiveAnswer = {
    id,
    receivedBytes,
    meeting: frame.meeting,
    conversation:
      typeof event["conversation_id"] === "string"
        ? event["conversation_id"]
        : null,
    prompt: text(event["prompt"], prior?.prompt ?? "").slice(0, 16_384),
    content: content.slice(0, 65_536),
    done: event["type"] === "ai_done" || event["done"] === true,
    complete: event["complete"] === true,
    truncated,
    at: frame.at,
  };
  return {
    ...state,
    answers: [...state.answers.filter((a) => a.id !== id), answer].slice(-8),
  };
};
/** Replace interim speech and deduplicate resumed streams without retaining unbounded history. */
export const applyLive = (state: LiveState, input: unknown): LiveState => {
  const raw = object(input);
  const at = Number(raw["emitted_at"]);
  if (
    !Number.isFinite(at) ||
    Date.now() - at > 180_000 ||
    at > Date.now() + 30_000
  ) {
    return state;
  }
  const frame: FrameContext = {
    meeting: typeof raw["meeting_id"] === "string" ? raw["meeting_id"] : null,
    device: text(raw["device_id"]),
    event: object(raw["event"]),
    at,
  };
  switch (frame.event["type"]) {
    case "meeting_active":
    case "meeting_ended": {
      return applyPresence(state, frame);
    }
    case "transcript": {
      return applyTranscript(state, frame);
    }
    case "ai_start":
    case "ai_delta":
    case "ai_done":
    case "ai_snapshot": {
      return applyAnswer(state, frame);
    }
    default: {
      return state;
    }
  }
};
