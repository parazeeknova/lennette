import { useQuery } from "@tanstack/react-query";
import { Search } from "lucide-react";
import { useEffect } from "react";

import { AccountError, accountRequest } from "@/lib/account-api";
import { text } from "@/lib/workspace";
import type { ItemDetail, WorkspaceItem } from "@/lib/workspace";

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
  const recap = metadata.data?.extras.find((extra) => extra.kind === "recap");
  return (
    <div className="mb-6 space-y-5">
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
