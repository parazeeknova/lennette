import { useQuery } from "@tanstack/react-query";
import { Download, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { AccountError, accountRequest } from "@/lib/account-api";
import { text } from "@/lib/workspace";
import type { ItemDetail, WorkspaceItem } from "@/lib/workspace";
import { collectHistory, saveExport } from "@/lib/workspace-export";
import type { ExportFormat } from "@/lib/workspace-export";

import { MeetingAnalytics } from "./meeting-analytics";
import { button, paper } from "./workspace-ui";

export const RecordTools = ({
  item,
  account,
  expired,
  search,
  setSearch,
}: {
  item: WorkspaceItem;
  account: string;
  expired: () => void;
  search: string;
  setSearch: (value: string) => void;
}) => {
  const [format, setFormat] = useState<ExportFormat>("md");
  const [count, setCount] = useState<number | null>(null);
  const [error, setError] = useState("");
  const running = useRef<AbortController | null>(null);
  useEffect(
    () => () => {
      running.current?.abort();
    },
    []
  );
  const metadata = useQuery({
    queryKey: ["workspace", account, "metadata", item.id],
    queryFn: ({ signal }) =>
      accountRequest<ItemDetail>(
        `/workspace/items/${encodeURIComponent(item.id)}?${new URLSearchParams({ account, limit: "1" })}`,
        "GET",
        undefined,
        signal
      ),
    gcTime: 0,
    retry: false,
  });
  useEffect(() => {
    if (
      metadata.error instanceof AccountError &&
      metadata.error.status === 401
    ) {
      expired();
    }
  }, [metadata.error, expired]);
  const exportAll = async () => {
    if (running.current) {
      return;
    }
    const controller = new AbortController();
    running.current = controller;
    setError("");
    setCount(0);
    try {
      const detail = await collectHistory(
        item.id,
        account,
        controller.signal,
        setCount
      );
      controller.signal.throwIfAborted();
      saveExport(detail, format);
    } catch (error_) {
      if (!controller.signal.aborted) {
        if (error_ instanceof AccountError && error_.status === 401) {
          expired();
        }
        setError(
          error_ instanceof Error
            ? error_.message
            : "Export failed. Please retry."
        );
      }
    }
    if (running.current === controller) {
      running.current = null;
    }
    if (!controller.signal.aborted) {
      setCount(null);
    }
  };
  const recap = metadata.data?.extras.find((extra) => extra.kind === "recap");
  return (
    <div className="mb-6 space-y-5">
      <div className="flex flex-wrap items-center gap-2">
        <label className="sr-only" htmlFor="export-format">
          Export format
        </label>
        <select
          id="export-format"
          value={format}
          disabled={count !== null}
          onChange={(event) => setFormat(event.target.value as ExportFormat)}
          className="min-h-11 rounded-2xl border border-white/80 bg-white/60 px-3 text-sm"
        >
          <option value="md">Markdown</option>
          <option value="txt">Plain text</option>
          <option value="json">JSON with analytics</option>
          {item.kind === "meeting" ? (
            <>
              <option value="srt">SRT subtitles</option>
              <option value="vtt">WebVTT subtitles</option>
            </>
          ) : null}
        </select>
        <button
          type="button"
          className={button}
          disabled={count !== null}
          onClick={() => {
            void exportAll();
          }}
        >
          <Download className="size-4" />
          {count === null
            ? "Export all saved history"
            : `Exporting ${count} entries…`}
        </button>
        {count === null ? null : (
          <button
            type="button"
            className={button}
            onClick={() => {
              running.current?.abort();
              running.current = null;
              setCount(null);
            }}
          >
            Cancel export
          </button>
        )}
      </div>
      <p className="text-xs leading-6 text-ink-soft">
        Exports include every saved page. Live text is included after desktop
        sync saves it.
      </p>
      {error ? (
        <p role="alert" className="text-sm text-rose-800">
          {error}
        </p>
      ) : null}
      {metadata.isError ? (
        <div className="text-sm text-ink-soft">
          <p>Saved analytics are temporarily unavailable.</p>
          <button
            type="button"
            className={`${button} mt-2`}
            onClick={() => {
              void metadata.refetch();
            }}
          >
            Retry analytics
          </button>
        </div>
      ) : null}
      {item.kind === "meeting" && metadata.data ? (
        <MeetingAnalytics
          key={item.id}
          analysis={metadata.data.extras.find(
            (extra) => extra.kind === "analysis"
          )}
          total={metadata.data.total_items}
          duration={metadata.data.record.data["duration_seconds"]}
          search={setSearch}
        />
      ) : null}
      {recap && text(recap.data["content"]) ? (
        <details className={paper}>
          <summary className="cursor-pointer font-semibold">
            Meeting recap
          </summary>
          <p className="mt-4 text-sm leading-7 [overflow-wrap:anywhere] whitespace-pre-wrap">
            {text(recap.data["content"])}
          </p>
        </details>
      ) : null}
      <label className="flex items-center gap-3 rounded-2xl border border-white/80 bg-white/60 px-4 py-3">
        <Search aria-hidden="true" className="size-4 shrink-0 text-ink-soft" />
        <input
          aria-label={
            item.kind === "meeting"
              ? "Search saved transcript"
              : "Search saved messages"
          }
          value={search}
          maxLength={200}
          onChange={(event) => setSearch(event.target.value)}
          placeholder={
            item.kind === "meeting"
              ? "Search the full saved transcript…"
              : "Search all saved messages…"
          }
          className="w-full min-w-0 bg-transparent text-sm outline-none"
        />
      </label>
    </div>
  );
};
