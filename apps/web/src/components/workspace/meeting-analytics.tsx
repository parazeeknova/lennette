import { useState } from "react";

import { clock, dateLabel, object, text } from "@/lib/workspace";
import type { WorkspaceItem } from "@/lib/workspace";

import { paper } from "./workspace-ui";

const tones = {
  positive: { label: "Positive", score: 1, color: "bg-emerald-600" },
  satisfactory: { label: "Satisfactory", score: 0.4, color: "bg-teal-600" },
  neutral: { label: "Neutral", score: 0, color: "bg-slate-400" },
  negative: { label: "Negative", score: -1, color: "bg-rose-600" },
  mixed: { label: "Mixed", score: 0, color: "bg-amber-500" },
  unknown: { label: "Insufficient evidence", score: 0, color: "bg-slate-300" },
};
type Tone = keyof typeof tones;
interface Segment {
  start: number;
  end: number;
  tone: Tone;
  reason: string;
  evidence: { quote: string; line: string }[];
}

const readReport = (content: unknown) => {
  try {
    const report = object(JSON.parse(text(content)));
    if (!Array.isArray(report["segments"]) || report["segments"].length > 12) {
      return null;
    }
    const segments: Segment[] = report["segments"].map((raw: unknown) => {
      const segment = object(raw);
      const assessment = object(segment["assessment"]);
      const candidate = text(assessment["tone"]);
      const tone: Tone = Object.hasOwn(tones, candidate)
        ? (candidate as Tone)
        : "unknown";
      const quotes = assessment["evidence"];
      return {
        start: Number(segment["start"]) || 0,
        end: Number(segment["end"]) || 0,
        tone,
        reason: text(assessment["reason"]),
        evidence: Array.isArray(quotes)
          ? quotes.slice(0, 2).map((quote: unknown) => ({
              quote: text(object(quote)["quote"]),
              line: text(object(quote)["line_id"]),
            }))
          : [],
      };
    });
    return {
      segments,
      sampled: Number(report["sampled_lines"]) || 0,
      total: Number(report["total_lines"]) || 0,
      clipped: report["clipped_lines"] === true,
    };
  } catch {
    return null;
  }
};

const overallTone = (segments: Segment[]): Tone => {
  const known = segments
    .map((s) => s.tone)
    .filter((tone) => tone !== "unknown");
  if (!known.length || known.length * 2 < segments.length) {
    return "unknown";
  }
  if (
    known.includes("mixed") ||
    (known.includes("negative") &&
      known.some((tone) => tone === "positive" || tone === "satisfactory"))
  ) {
    return "mixed";
  }
  return (
    (["positive", "negative", "satisfactory", "neutral"] as const).find(
      (tone) =>
        known.filter((value) => value === tone).length * 2 >= known.length
    ) ?? "mixed"
  );
};

export const MeetingAnalytics = ({
  analysis,
  total,
  duration,
  search,
}: {
  analysis: WorkspaceItem | undefined;
  total: number | undefined;
  duration: unknown;
  search: (value: string) => void;
}) => {
  const [selected, setSelected] = useState(0);
  const report =
    analysis?.data["status"] === "ready"
      ? readReport(analysis.data["content"])
      : null;
  const segment = report?.segments[selected];
  const stale =
    analysis &&
    total !== undefined &&
    Number(analysis.data["source_lines"]) !== total;
  return (
    <section className={`${paper} space-y-4`} aria-label="Meeting analytics">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <h3 className="font-semibold">Meeting sentiment</h3>
        <p className="text-xs text-ink-soft">
          {total ?? "—"} saved lines · {clock(duration)} duration
        </p>
      </div>
      <p className="text-xs leading-6 text-ink-soft">
        Tone expressed in the transcript, not a verdict on the meeting. Generate
        or refresh this assessment in the desktop app.
      </p>
      {stale ? (
        <p className="text-sm text-amber-800">
          The transcript has changed since this assessment.
        </p>
      ) : null}
      {report ? (
        <>
          <p className="text-lg font-semibold">
            {tones[overallTone(report.segments)].label}{" "}
            <span className="text-xs font-normal text-ink-soft">
              · {report.segments.length} sections
            </span>
          </p>
          <fieldset
            className="flex w-full min-w-0 gap-1 overflow-x-auto pb-2"
            aria-label="Sentiment timeline; select a section for evidence"
          >
            {report.segments.map((section, index) => {
              const tone = tones[section.tone];
              const height = Math.max(5, Math.abs(tone.score) * 44);
              return (
                <button
                  key={`${section.start}:${index}`}
                  type="button"
                  onClick={() => setSelected(index)}
                  aria-pressed={index === selected}
                  aria-label={`${clock(section.start)} to ${clock(section.end)}: ${tone.label}`}
                  className={`min-w-11 flex-1 rounded-xl px-1 pb-2 transition-colors ${selected === index ? "bg-sky/15 ring-1 ring-sky/30 ring-inset" : "hover:bg-white/60"}`}
                >
                  <span className="relative block h-28">
                    <span className="absolute top-14 right-0 left-0 h-px bg-ink/10" />
                    <span
                      className={`absolute right-1 left-1 rounded-sm ${tone.color}`}
                      style={{ height, top: tone.score > 0 ? 56 - height : 56 }}
                    />
                  </span>
                  <span className="text-[10px] text-ink-soft">
                    {clock(section.start)}
                  </span>
                </button>
              );
            })}
          </fieldset>
          <ul
            className="flex flex-wrap gap-x-4 gap-y-2 text-xs text-ink-soft"
            aria-label="Timeline legend"
          >
            {Object.entries(tones).map(([key, tone]) => (
              <li key={key} className="flex items-center gap-1.5">
                <span
                  aria-hidden="true"
                  className={`size-2 rounded-full ${tone.color}`}
                />
                {tone.label}
              </li>
            ))}
          </ul>
          {segment ? (
            <div className="space-y-3 border-t border-ink/10 pt-4">
              <h4 className="text-sm font-semibold">
                {clock(segment.start)} – {clock(segment.end)} ·{" "}
                {tones[segment.tone].label}
              </h4>
              <p className="text-sm leading-7 [overflow-wrap:anywhere]">
                {segment.reason}
              </p>
              {segment.evidence.map((evidence, index) => (
                <button
                  key={`${evidence.line}:${index}`}
                  type="button"
                  onClick={() => search(evidence.quote.slice(0, 200))}
                  className="block w-full rounded-xl bg-sky/10 p-4 text-left text-sm leading-7 [overflow-wrap:anywhere] hover:bg-sky/20"
                >
                  <span className="mb-1 block text-xs font-medium text-sky-deep">
                    Find supporting quote in transcript
                  </span>
                  “{evidence.quote}”
                </button>
              ))}
            </div>
          ) : null}
          <p className="text-xs leading-6 text-ink-soft">
            {report.sampled} sampled of {report.total} lines
            {report.clipped ? " · passages were shortened" : ""}. Text alone may
            miss tone, humor or context.
          </p>
        </>
      ) : (
        <p className="text-sm leading-7 text-ink-soft">
          {analysis
            ? "This assessment is pending, failed or unreadable. Open the desktop app to generate it again."
            : "No saved assessment yet. Analyze tone in the desktop app, then sync to see the timeline here."}
        </p>
      )}
      {analysis ? (
        <p className="text-xs text-ink-soft">
          Saved {dateLabel(analysis.data["updated_at"])}
        </p>
      ) : null}
    </section>
  );
};
