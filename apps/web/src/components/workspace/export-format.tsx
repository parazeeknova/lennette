import { Check, ChevronDown } from "lucide-react";
import { useEffect, useRef } from "react";

import type { ExportFormat } from "@/lib/workspace-export";

import { button } from "./workspace-ui";

const formats: { value: ExportFormat; label: string; description: string }[] = [
  {
    value: "md",
    label: "Markdown",
    description: "Formatted notes and history",
  },
  {
    value: "txt",
    label: "Plain text",
    description: "Simple, readable transcript",
  },
  { value: "json", label: "JSON", description: "History with saved analytics" },
  { value: "srt", label: "SRT", description: "Timed subtitles" },
  { value: "vtt", label: "WebVTT", description: "Subtitles for web players" },
];

export const ExportFormatPicker = ({
  value,
  meeting,
  disabled,
  change,
}: {
  value: ExportFormat;
  meeting: boolean;
  disabled: boolean;
  change: (value: ExportFormat) => void;
}) => {
  const dropdown = useRef<HTMLDetailsElement>(null);
  const trigger = useRef<HTMLElement>(null);
  useEffect(() => {
    const close = (event: Event) => {
      if (
        event.target instanceof Node &&
        !dropdown.current?.contains(event.target) &&
        dropdown.current
      ) {
        dropdown.current.open = false;
      }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && dropdown.current?.open) {
        event.preventDefault();
        dropdown.current.open = false;
        trigger.current?.focus();
      }
    };
    document.addEventListener("pointerdown", close);
    document.addEventListener("focusin", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("focusin", close);
      document.removeEventListener("keydown", escape);
    };
  }, []);
  return (
    <details ref={dropdown} className="group relative">
      <summary
        ref={trigger}
        aria-label={`Export format: ${formats.find((format) => format.value === value)?.label}`}
        aria-disabled={disabled}
        tabIndex={disabled ? -1 : 0}
        onClick={(event) => {
          if (disabled) {
            event.preventDefault();
          }
        }}
        className={`${button} cursor-pointer list-none select-none group-open:bg-white/90 [&::-webkit-details-marker]:hidden ${disabled ? "cursor-not-allowed opacity-40" : ""}`}
      >
        {formats.find((format) => format.value === value)?.label}
        <ChevronDown
          aria-hidden="true"
          className="size-3.5 text-ink-soft transition-transform group-open:rotate-180"
        />
      </summary>
      <div className="absolute top-full right-0 z-50 mt-2 w-64 max-w-[calc(100vw-3rem)] rounded-2xl border border-white/90 bg-cloud/95 p-1.5 shadow-xl shadow-sky-deep/15 backdrop-blur-xl">
        <p className="px-3 pt-2 pb-1 text-[10px] font-semibold tracking-widest text-ink-soft uppercase">
          Export format
        </p>
        <ul aria-label="Export format options" className="space-y-0.5">
          {formats
            .filter(
              (format) =>
                meeting || (format.value !== "srt" && format.value !== "vtt")
            )
            .map((format) => (
              <li key={format.value}>
                <button
                  type="button"
                  disabled={disabled}
                  aria-pressed={value === format.value}
                  aria-label={`${format.label}: ${format.description}`}
                  onClick={() => {
                    change(format.value);
                    if (dropdown.current) {
                      dropdown.current.open = false;
                    }
                    trigger.current?.focus();
                  }}
                  className={`flex min-h-11 w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors focus-visible:outline-2 focus-visible:outline-sky-deep ${value === format.value ? "bg-sky/15" : "hover:bg-white/80"}`}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block text-sm font-semibold">
                      {format.label}
                    </span>
                    <span className="mt-0.5 block text-xs text-ink-soft">
                      {format.description}
                    </span>
                  </span>
                  {value === format.value ? (
                    <Check
                      aria-hidden="true"
                      className="size-4 shrink-0 text-sky-deep"
                    />
                  ) : (
                    <span className="text-[10px] text-ink-soft">
                      .{format.value}
                    </span>
                  )}
                </button>
              </li>
            ))}
        </ul>
        <p className="mx-3 mt-1 border-t border-ink/10 pt-2 pb-2 text-[11px] leading-5 text-ink-soft">
          Includes all saved pages. Live text is included after sync.
        </p>
      </div>
    </details>
  );
};
