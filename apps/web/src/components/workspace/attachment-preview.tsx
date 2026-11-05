import { Suspense, lazy, useEffect, useState } from "react";

import { API_URL, AccountError } from "@/lib/account-api";
import { itemTitle, text } from "@/lib/workspace";
import type { WorkspaceItem } from "@/lib/workspace";

const PdfPreview = lazy(() => import("./pdf-preview"));

export const AttachmentPreview = ({
  item,
  expired,
}: {
  item: WorkspaceItem;
  expired: () => void;
}) => {
  const [preview, setPreview] = useState<{
    url: string;
    mime: string;
    body: string;
  } | null>(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    let url: string | undefined;
    void (async () => {
      const response = await fetch(
        `${API_URL}/v1/attachments/${encodeURIComponent(item.id)}`,
        {
          credentials: "include",
          cache: "no-store",
          signal: AbortSignal.any([
            controller.signal,
            AbortSignal.timeout(30_000),
          ]),
        }
      );
      if (response.status === 401) {
        expired();
        return;
      }
      if (!response.ok) {
        throw new AccountError(
          "This file could not be loaded. Please retry.",
          response.status
        );
      }
      const blob = await response.blob();
      if (controller.signal.aborted) {
        return;
      }
      if (blob.size > 10 * 1024 * 1024) {
        throw new Error("This file is too large to preview.");
      }
      url = URL.createObjectURL(blob);
      const mime = text(item.data["mime_type"]);
      const body = mime === "text/plain" ? await blob.text() : "";
      if (!controller.signal.aborted) {
        setPreview({ url, mime, body: body.slice(0, 300_000) });
      }
    })().catch((error_) => {
      if (!controller.signal.aborted) {
        setError(
          error_ instanceof Error ? error_.message : "File unavailable."
        );
      }
    });
    return () => {
      controller.abort();
      if (url) {
        URL.revokeObjectURL(url);
      }
    };
    // Each explicit retry must rerun the request even when the item is unchanged.
    // eslint-disable-next-line react/exhaustive-effect-dependencies
  }, [item.id, item.data, expired, attempt]);
  if (error) {
    return (
      <div className="space-y-3 p-5 text-sm text-ink-soft">
        <p role="alert">{error}</p>
        <button
          type="button"
          className="glass glass-pill px-5 py-3 text-sm font-semibold"
          onClick={() => {
            setError("");
            setPreview(null);
            setAttempt((value) => value + 1);
          }}
        >
          Retry
        </button>
      </div>
    );
  }
  if (!preview) {
    return (
      <output className="p-5 text-sm text-ink-soft">Loading your file…</output>
    );
  }
  const title = itemTitle(item);
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-ink-soft">
          {Math.ceil(
            Number(item.data["size_bytes"] ?? 0) / 1024
          ).toLocaleString()}{" "}
          KB
        </p>
        <a
          href={preview.url}
          download={Array.from(title, (char) =>
            (char.codePointAt(0) ?? 0) < 32 || "/\\".includes(char) ? "_" : char
          ).join("")}
          className="glass glass-pill px-5 py-3 text-sm font-semibold"
        >
          Download original
        </a>
      </div>
      {["image/png", "image/jpeg", "image/webp"].includes(preview.mime) ? (
        <img
          src={preview.url}
          alt={title}
          className="mx-auto max-h-[65dvh] max-w-full rounded-2xl object-contain"
        />
      ) : null}
      {preview.mime === "application/pdf" ? (
        <Suspense
          fallback={
            <output className="block py-6 text-sm text-ink-soft">
              Opening PDF…
            </output>
          }
        >
          <PdfPreview url={preview.url} />
        </Suspense>
      ) : null}
      {preview.mime === "text/plain" ? (
        <pre className="text-sm leading-7 [overflow-wrap:anywhere] whitespace-pre-wrap">
          {preview.body}
        </pre>
      ) : null}
      {[
        "image/png",
        "image/jpeg",
        "image/webp",
        "application/pdf",
        "text/plain",
      ].includes(preview.mime) ? null : (
        <p className="py-12 text-center text-ink-soft">
          Download this document to open it in your preferred app.
        </p>
      )}
    </div>
  );
};
