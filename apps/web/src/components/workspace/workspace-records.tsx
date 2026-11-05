import type { UseQueryResult } from "@tanstack/react-query";
import {
  ArrowLeft,
  ArrowRight,
  FileText,
  FolderOpen,
  Radio,
  Search,
  X,
} from "lucide-react";
import type { Dispatch, SetStateAction } from "react";

import { AccountError } from "@/lib/account-api";
import { dateLabel, itemTitle, text } from "@/lib/workspace";
import type {
  ItemKind,
  ItemPage,
  ItemDetail,
  WorkspaceItem,
  LiveState,
  LiveMeeting,
} from "@/lib/workspace";

import { AttachmentPreview } from "./attachment-preview";
import { RecordTools } from "./record-tools";
import { History, LivePanel } from "./workspace-history";
import { button, paper, Pager } from "./workspace-ui";

const labels: Record<string, string> = {
  conversation: "Conversations",
  meeting: "Meetings",
  file: "Attachments",
  attachment: "Attachments",
};

const hasLive = (item: WorkspaceItem, live: LiveState, now: number) => {
  if (item.kind === "meeting") {
    return (
      live.meetings.some(
        (m) => m.id === item.id && m.active && now - m.at < 70_000
      ) ||
      live.transcripts.some(
        (t) => t.meeting === item.id && now - t.at < 180_000
      ) ||
      live.answers.some((a) => a.meeting === item.id && now - a.at < 180_000)
    );
  }
  return (
    item.kind === "conversation" &&
    live.answers.some((a) => a.conversation === item.id && now - a.at < 180_000)
  );
};
type Pages = Dispatch<SetStateAction<(string | null)[]>>;
interface ListProps {
  kind: ItemKind;
  selected: WorkspaceItem | null;
  search: string;
  setSearch: (value: string) => void;
  query: string;
  active: LiveMeeting[];
  list: UseQueryResult<ItemPage, Error>;
  open: (item: WorkspaceItem) => void;
  navigate: (kind: ItemKind) => void;
  pages: (string | null)[];
  setPages: Pages;
}
export const ListPane = ({
  kind,
  selected,
  search,
  setSearch,
  query,
  active,
  list,
  open,
  navigate,
  pages,
  setPages,
}: ListProps) => (
  <section
    aria-label="Saved items"
    className={`glass min-w-0 rounded-3xl p-4 ${selected ? "hidden lg:block" : ""}`}
  >
    <h2 className="mb-4 font-display text-xl">{labels[kind]}</h2>
    {(kind === "file" || kind === "attachment") && (
      <div className="mb-4 flex gap-2">
        <button
          type="button"
          className={button}
          aria-pressed={kind === "file"}
          onClick={() => navigate("file")}
        >
          Uploads
        </button>
        <button
          type="button"
          className={button}
          aria-pressed={kind === "attachment"}
          onClick={() => navigate("attachment")}
        >
          Documents
        </button>
      </div>
    )}
    <label className="mb-4 flex items-center gap-2 rounded-2xl border border-white/80 bg-white/50 px-3 py-2.5">
      <Search className="size-4 shrink-0 text-ink-soft" />
      <span className="sr-only">Search titles</span>
      <input
        type="search"
        value={search}
        maxLength={200}
        onChange={(event) => setSearch(event.target.value)}
        placeholder="Search titles…"
        className="w-full min-w-0 bg-transparent text-sm outline-none"
      />
    </label>
    {active.length > 0 && (
      <div className="mb-4 space-y-2">
        <p className="px-2 text-xs font-semibold tracking-wide text-sky-deep">
          LIVE NOW
        </p>
        {active.map((meeting) => (
          <button
            key={meeting.id}
            type="button"
            onClick={() =>
              open({
                id: meeting.id,
                kind: "meeting",
                data: { title: "Live meeting" },
              })
            }
            className="flex w-full items-center gap-2 rounded-2xl bg-sky/15 p-3 text-left text-sm font-medium"
          >
            <Radio className="size-4 text-sky-deep" />
            {itemTitle(
              list.data?.items.find((item) => item.id === meeting.id) ?? {
                id: meeting.id,
                kind: "meeting",
                data: { title: "Live meeting" },
              }
            )}
            <ArrowRight className="ml-auto size-4" />
          </button>
        ))}
      </div>
    )}
    {list.isPending && (
      <output className="p-4 text-sm text-ink-soft">
        Loading your history…
      </output>
    )}
    {list.isError && (
      <div className="p-3 text-sm text-ink-soft">
        <p role="alert">History could not be loaded.</p>
        <button
          type="button"
          className={`${button} mt-3`}
          onClick={() => {
            void list.refetch();
          }}
        >
          Retry
        </button>
      </div>
    )}
    {list.data?.items.length === 0 && (
      <p className="p-4 text-sm leading-6 text-ink-soft">
        {query
          ? "No matching titles."
          : "Nothing synced here yet. Sign in to the same account on your desktop and enable sync."}
      </p>
    )}
    <div className="space-y-2">
      {list.data?.items.map((item) => (
        <button
          key={item.id}
          type="button"
          onClick={() => open(item)}
          aria-current={selected?.id === item.id ? "true" : undefined}
          className={`w-full rounded-2xl border p-3.5 text-left transition-colors ${selected?.id === item.id ? "border-sky/50 bg-sky/15" : "border-transparent hover:border-white/90 hover:bg-white/60"}`}
        >
          <span className="block truncate text-sm font-semibold">
            {itemTitle(item)}
          </span>
          <span className="mt-1 block text-xs text-ink-soft">
            {dateLabel(
              item.data["updated_at"] ??
                item.data["started_at"] ??
                item.data["created_at"]
            ) || "Synced from desktop"}
          </span>
        </button>
      ))}
    </div>
    <Pager
      older={list.data?.next ?? null}
      previous={pages.length > 1}
      busy={list.isFetching}
      back={() => setPages((v) => v.slice(0, -1))}
      forward={(cursor) => setPages((v) => [...v, cursor])}
    />
  </section>
);
interface DetailProps {
  account: string;
  search: string;
  setSearch: (value: string) => void;
  current: WorkspaceItem | null;
  selected: WorkspaceItem | null;
  setSelected: (item: WorkspaceItem | null) => void;
  detail: UseQueryResult<ItemDetail, Error>;
  detailPages: (string | null)[];
  setDetailPages: Pages;
  open: (item: WorkspaceItem) => void;
  live: LiveState;
  status: string;
  expired: () => void;
  now: number;
}
export const DetailPane = ({
  account,
  search,
  setSearch,
  current,
  selected,
  setSelected,
  detail,
  detailPages,
  setDetailPages,
  open,
  live,
  status,
  expired,
  now,
}: DetailProps) => (
  <section
    aria-label="Selected content"
    className={`glass min-w-0 rounded-3xl ${selected ? "" : "hidden lg:block"}`}
  >
    {current ? (
      <>
        <div className="flex items-start gap-3 border-b border-white/70 px-4 py-5 sm:px-6">
          <button
            type="button"
            onClick={() => setSelected(null)}
            className={`${button} shrink-0 lg:hidden`}
            aria-label="Back to saved items"
          >
            <ArrowLeft className="size-4" />
          </button>
          <div className="min-w-0 flex-1">
            <p className="mb-1 text-xs text-ink-soft">
              {labels[current.kind]?.toUpperCase()}
            </p>
            <h2 className="font-display text-2xl [overflow-wrap:anywhere]">
              {itemTitle(current)}
            </h2>
          </div>
          <button
            type="button"
            onClick={() => setSelected(null)}
            className="rounded-full p-2 text-ink-soft hover:bg-white/50"
            aria-label="Close selected item"
          >
            <X className="size-4" />
          </button>
        </div>
        <div className="max-h-[calc(100dvh-14rem)] overflow-y-auto overscroll-contain p-4 sm:p-6">
          {current.kind === "file" ? (
            <AttachmentPreview
              key={current.id}
              item={current}
              expired={expired}
            />
          ) : (
            <>
              {(current.kind === "meeting" ||
                current.kind === "conversation") && (
                <RecordTools
                  key={current.id}
                  item={current}
                  account={account}
                  expired={expired}
                  search={search}
                  setSearch={setSearch}
                />
              )}
              {hasLive(current, live, now) && (
                <LivePanel
                  live={live}
                  meeting={current.kind === "meeting" ? current.id : null}
                  conversation={
                    current.kind === "conversation" ? current.id : null
                  }
                  status={status}
                />
              )}
              {detail.isPending && (
                <output className="mb-5 text-sm text-ink-soft">
                  Loading saved content…
                </output>
              )}
              {detail.isError && (
                <div className="mb-5 text-sm text-ink-soft">
                  <p role="alert">
                    {detail.error instanceof AccountError &&
                    detail.error.status === 404
                      ? "This record is no longer available, or its history has not synced yet."
                      : "Saved content is temporarily unavailable."}
                  </p>
                  <button
                    type="button"
                    onClick={() => {
                      void detail.refetch();
                    }}
                    className={`${button} mt-3`}
                  >
                    Retry
                  </button>
                </div>
              )}
              {current.kind === "attachment" ? (
                <div className={paper}>
                  <h3 className="mb-4 font-semibold">Document text</h3>
                  <div className="text-sm leading-7 [overflow-wrap:anywhere] whitespace-pre-wrap">
                    {text(
                      current.data["ocr_text"],
                      "No extracted text is available for this document."
                    )}
                  </div>
                </div>
              ) : (
                <History detail={detail.data} kind={current.kind} />
              )}
              {detail.data && current.kind !== "attachment" && (
                <Pager
                  older={detail.data.next}
                  previous={detailPages.length > 1}
                  busy={detail.isFetching}
                  back={() => setDetailPages((v) => v.slice(0, -1))}
                  forward={(cursor) => setDetailPages((v) => [...v, cursor])}
                />
              )}
              {detail.data?.files.length ? (
                <section className="mt-7">
                  <h3 className="mb-3 font-semibold">Attachments</h3>
                  <div className="flex flex-wrap gap-2">
                    {detail.data.files.map((file) => (
                      <button
                        key={file.id}
                        type="button"
                        className={button}
                        onClick={() => open(file)}
                      >
                        <FileText className="size-4" />
                        {itemTitle(file)}
                      </button>
                    ))}
                  </div>
                </section>
              ) : null}
            </>
          )}
        </div>
      </>
    ) : (
      <div className="flex min-h-[65dvh] flex-col items-center justify-center px-6 text-center">
        <FolderOpen className="mb-5 size-10 text-sky-deep/60" />
        <h2 className="font-display text-3xl">Pick up where you left off.</h2>
        <p className="mt-4 max-w-sm text-sm leading-7 text-ink-soft">
          Choose a conversation, meeting or attachment. During a managed
          meeting, follow the live transcript and AI answers from any device.
        </p>
      </div>
    )}
  </section>
);
