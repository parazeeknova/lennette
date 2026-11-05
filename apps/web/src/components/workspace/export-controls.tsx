import { Download } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { AccountError } from "@/lib/account-api";
import type { WorkspaceItem } from "@/lib/workspace";
import { collectHistory, saveExport } from "@/lib/workspace-export";
import type { ExportFormat } from "@/lib/workspace-export";

import { ExportFormatPicker } from "./export-format";
import { button } from "./workspace-ui";

export const ExportControls = ({
  item,
  account,
  expired,
}: {
  item: WorkspaceItem;
  account: string;
  expired: () => void;
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

  return (
    <div className="flex max-w-full flex-wrap items-center justify-end gap-2">
      <ExportFormatPicker
        value={format}
        meeting={item.kind === "meeting"}
        disabled={count !== null}
        change={setFormat}
      />
      <button
        type="button"
        className={button}
        disabled={count !== null}
        aria-label="Export all saved history"
        onClick={() => {
          void exportAll();
        }}
      >
        <Download aria-hidden="true" className="size-4" />
        <span>{count === null ? "Export" : "Exporting…"}</span>
      </button>
      {count === null ? null : (
        <>
          <output aria-live="polite" className="text-xs text-ink-soft">
            {count} entries
          </output>
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
        </>
      )}
      {error ? (
        <p
          role="alert"
          className="w-full max-w-sm text-right text-xs leading-5 text-rose-800"
        >
          {error}
        </p>
      ) : null}
    </div>
  );
};
