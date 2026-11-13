import { Radio } from "lucide-react";
import { useEffect, useState } from "react";

import { clock, dateLabel, text } from "@/lib/workspace";
import type {
  ItemDetail,
  LiveState,
  WorkspaceItem,
  LiveAnswer,
} from "@/lib/workspace";
import { speakerName } from "@/lib/workspace-export";

import { button, paper } from "./workspace-ui";

const answerLabel = (answer: LiveAnswer) => {
  if (answer.truncated) {
    return "Partial live preview. Read the full answer in the saved conversation.";
  }
  if (!answer.done) {
    return "Lennette is answering…";
  }
  return answer.complete
    ? "Answer complete"
    : "Answer interrupted · check the saved conversation";
};

export const History = ({
  detail,
  kind,
}: {
  detail: ItemDetail | undefined;
  kind: WorkspaceItem["kind"];
}) => (
  <div className="space-y-4">
    {detail && detail.items.length === 0 ? (
      <p className="py-8 text-center text-sm text-ink-soft">
        No saved entries match this view. Clear the search, or wait for desktop
        sync.
      </p>
    ) : null}
    {detail?.items.toReversed().map((item) => (
      <article
        key={item.id}
        className={`${paper} ${item.data["role"] === "assistant" ? "border-sky/20 bg-sky/5" : ""}`}
      >
        <div className="mb-3 flex items-center justify-between gap-3 text-xs font-semibold text-ink-soft">
          <span>{speakerName(item, detail.extras)}</span>
          <span>
            {kind === "meeting"
              ? clock(item.data["start_time"])
              : dateLabel(item.data["timestamp"])}
          </span>
        </div>
        <p className="text-sm leading-7 [overflow-wrap:anywhere] whitespace-pre-wrap">
          {text(item.data["content"] ?? item.data["text"])}
        </p>
      </article>
    ))}
  </div>
);

export const LivePanel = ({
  live,
  meeting,
  conversation,
  status,
}: {
  live: LiveState;
  meeting: string | null;
  conversation: string | null;
  status: string;
}) => {
  const [tab, setTab] = useState(conversation ? "answers" : "transcript");
  const [follow, setFollow] = useState(true);
  const lines = live.transcripts
    .filter((line) => line.meeting === meeting)
    .toSorted((a, b) => a.start - b.start || a.at - b.at);
  const answers = live.answers.filter((answer) =>
    conversation
      ? answer.conversation === conversation
      : answer.meeting === meeting
  );
  const latest = `${lines.at(-1)?.key}:${lines.at(-1)?.content}`;
  const [end, setEnd] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    if (follow && end && latest) {
      end.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [latest, follow, end]);
  let transcriptVisibility = tab === "transcript" ? "" : "hidden xl:block";
  if (!meeting) {
    transcriptVisibility = "hidden";
  }
  return (
    <section className="mt-8 border-t border-white/80 pt-6">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 font-semibold">
          <Radio className="size-4 text-sky-deep" />
          Live companion
        </h3>
        <p className="text-xs text-ink-soft">
          {status === "Live"
            ? "Updates arrive as your desktop receives them"
            : "Reconnecting · saved history remains available"}
        </p>
      </div>
      {meeting && (
        <div className="mb-4 flex gap-2 xl:hidden">
          <button
            type="button"
            className={button}
            aria-pressed={tab === "transcript"}
            onClick={() => setTab("transcript")}
          >
            Transcript
          </button>
          <button
            type="button"
            className={button}
            aria-pressed={tab === "answers"}
            onClick={() => setTab("answers")}
          >
            AI answers
          </button>
        </div>
      )}
      <div className={meeting ? "grid gap-4 xl:grid-cols-2" : "grid gap-4"}>
        <section className={`${paper} min-w-0 ${transcriptVisibility}`}>
          <div className="mb-4 flex items-center justify-between gap-2">
            <h4 className="font-semibold">Live transcript</h4>
            <button
              type="button"
              aria-pressed={follow}
              onClick={() => setFollow((v) => !v)}
              className="rounded-full border border-sky/20 px-3 py-1 text-xs"
            >
              {follow ? "Following" : "Follow latest"}
            </button>
          </div>
          <div
            className="max-h-[45dvh] space-y-4 overflow-y-auto overscroll-contain pr-1"
            onWheel={() => setFollow(false)}
            onTouchMove={() => setFollow(false)}
          >
            {lines.length ? (
              lines.map((line) => (
                <div key={line.key} className={line.final ? "" : "opacity-60"}>
                  <p className="mb-1 flex justify-between gap-3 text-xs font-semibold text-ink-soft">
                    <span>{line.channel === 0 ? "You" : "Meeting audio"}</span>
                    <span>{clock(line.start)}</span>
                  </p>
                  <p className="text-sm leading-6 [overflow-wrap:anywhere] whitespace-pre-wrap">
                    {line.content}
                  </p>
                </div>
              ))
            ) : (
              <p className="text-sm leading-6 text-ink-soft">
                Waiting for live speech. Start a managed meeting on your desktop
                using this account.
              </p>
            )}
            <div ref={setEnd} />
          </div>
        </section>
        <section
          className={`${paper} min-w-0 ${tab === "answers" ? "" : "hidden xl:block"}`}
        >
          <h4 className="mb-4 font-semibold">Live AI answers</h4>
          <div className="max-h-[45dvh] space-y-5 overflow-y-auto overscroll-contain">
            {answers.length ? (
              answers.map((answer) => (
                <article key={answer.id}>
                  {answer.prompt ? (
                    <p className="mb-3 rounded-xl bg-sky/10 p-3 text-sm font-medium [overflow-wrap:anywhere]">
                      {answer.prompt}
                    </p>
                  ) : null}
                  <p className="text-sm leading-7 [overflow-wrap:anywhere] whitespace-pre-wrap">
                    {answer.content || "Thinking…"}
                  </p>
                  <p className="mt-2 text-xs text-ink-soft">
                    {answer.providerNotice ? `${answer.providerNotice} ` : ""}
                    {answerLabel(answer)}
                  </p>
                </article>
              ))
            ) : (
              <p className="text-sm leading-6 text-ink-soft">
                Answers from your desktop appear here as they are generated.
              </p>
            )}
          </div>
        </section>
      </div>
    </section>
  );
};
