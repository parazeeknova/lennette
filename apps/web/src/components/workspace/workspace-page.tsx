import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { FolderOpen, MessageSquare, Radio, Settings } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";

import { AccountError, accountRequest } from "@/lib/account-api";
import type {
  ItemDetail,
  ItemKind,
  ItemPage,
  WorkspaceItem,
} from "@/lib/workspace";

import { useWorkspaceLive } from "./use-workspace-live";
import { ListPane, DetailPane } from "./workspace-records";
import { button } from "./workspace-ui";

interface User {
  id: string;
  name: string;
  email: string;
}

const Frame = ({ children }: { children: ReactNode }) => (
  <main className="relative min-h-dvh bg-cloud text-ink">
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_top_left,oklch(0.88_0.07_240),transparent_60%)]"
    />
    <div className="relative mx-auto max-w-[1680px] p-3 sm:p-5">{children}</div>
  </main>
);

const Workspace = ({ user, expired }: { user: User; expired: () => void }) => {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 10_000);
    return () => clearInterval(timer);
  }, []);
  const [kind, setKind] = useState<ItemKind>("meeting");
  const [hiddenItems, setHiddenItems] = useState<Set<string>>(() => new Set());
  const [selected, setSelected] = useState<WorkspaceItem | null>(null);
  const [search, setSearch] = useState("");
  const [query, setQuery] = useState("");
  const [pages, setPages] = useState<(string | null)[]>([null]);
  const [detailPages, setDetailPages] = useState<(string | null)[]>([null]);
  const { live, status } = useWorkspaceLive(user.id, expired);
  useEffect(() => {
    const timer = setTimeout(() => {
      setQuery(search);
      setPages([null]);
    }, 250);
    return () => clearTimeout(timer);
  }, [search]);
  const list = useQuery({
    queryKey: ["workspace", user.id, "items", kind, query, pages.at(-1)],
    gcTime: 0,
    queryFn: ({ signal }) => {
      const params = new URLSearchParams({ kind, q: query, account: user.id });
      const after = pages.at(-1);
      if (after) {
        params.set("after", after);
      }
      return accountRequest<ItemPage>(
        `/workspace/items?${params}`,
        "GET",
        undefined,
        signal
      );
    },
    retry: false,
  });
  const detail = useQuery({
    queryKey: [
      "workspace",
      user.id,
      "detail",
      selected?.id,
      detailPages.at(-1),
    ],
    gcTime: 0,
    queryFn: ({ signal }) => {
      const params = new URLSearchParams({ account: user.id });
      const after = detailPages.at(-1);
      if (after) {
        params.set("after", after);
      }
      return accountRequest<ItemDetail>(
        `/workspace/items/${selected?.id ?? ""}?${params}`,
        "GET",
        undefined,
        signal
      );
    },
    enabled: selected !== null && selected.kind !== "file",
    retry: false,
  });
  useEffect(() => {
    if (
      [list.error, detail.error].some(
        (error) => error instanceof AccountError && error.status === 401
      )
    ) {
      expired();
    }
  }, [list.error, detail.error, expired]);
  useEffect(() => {
    if (
      detail.error instanceof AccountError &&
      detail.error.status === 404 &&
      selected
    ) {
      const { id } = selected;
      queueMicrotask(() => {
        setSelected((current) => (current?.id === id ? null : current));
        setHiddenItems((current) => new Set([...current, id].slice(-128)));
      });
    }
  }, [detail.error, selected]);
  const open = (item: WorkspaceItem) => {
    setSelected(item);
    setDetailPages([null]);
  };
  const navigate = (next: ItemKind) => {
    setKind(next);
    setSelected(null);
    setSearch("");
    setQuery("");
    setPages([null]);
  };
  const active = live.meetings
    .filter((m) => m.active && now - m.at < 70_000 && !hiddenItems.has(m.id))
    .filter((m, i, all) => all.findIndex((other) => other.id === m.id) === i);
  const current = detail.data?.record ?? selected;
  return (
    <>
      <header className="glass mb-4 flex flex-wrap items-center justify-between gap-3 rounded-3xl px-5 py-4">
        <Link to="/" className="flex items-center gap-2 font-display text-2xl">
          <img src="/lennette.png" alt="" width={34} height={24} />
          lennette
        </Link>
        <Link to="/account" className={button}>
          <Settings className="size-4" />
          <span className="hidden sm:inline">{user.name || "Account"}</span>
          <span className="sm:hidden">Account</span>
        </Link>
      </header>
      <div className="grid min-h-[calc(100dvh-8rem)] gap-3 lg:grid-cols-[190px_285px_minmax(0,1fr)]">
        <aside className="flex min-w-0 flex-col lg:py-4">
          <nav
            aria-label="Workspace"
            className="flex gap-2 overflow-x-auto pb-2 lg:flex-col"
          >
            {(
              [
                {
                  id: "conversation",
                  label: "Conversations",
                  icon: MessageSquare,
                },
                { id: "meeting", label: "Meetings", icon: Radio },
                { id: "file", label: "Attachments", icon: FolderOpen },
              ] as const
            ).map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => navigate(tab.id)}
                aria-current={
                  kind === tab.id ||
                  (tab.id === "file" && kind === "attachment")
                    ? "page"
                    : undefined
                }
                className={`flex shrink-0 items-center gap-2 rounded-2xl px-4 py-3 text-sm font-medium transition-colors ${kind === tab.id || (tab.id === "file" && kind === "attachment") ? "glass glass-blue" : "hover:bg-white/50"}`}
              >
                <tab.icon className="size-4" />
                {tab.label}
              </button>
            ))}
          </nav>
          <output
            aria-live="polite"
            className="glass mt-3 flex items-center gap-2.5 self-start rounded-2xl px-3 py-3 text-xs leading-5 font-medium text-ink-soft lg:mt-auto lg:self-stretch"
          >
            <span
              aria-hidden="true"
              className={`size-2 shrink-0 rounded-full ring-4 ${status === "Live" ? "bg-emerald-600 ring-emerald-600/10" : "bg-amber-600 ring-amber-600/10"}`}
            />
            <span>{status === "Live" ? "Workspace connected" : status}</span>
          </output>
        </aside>
        <ListPane
          kind={kind}
          selected={selected}
          search={search}
          setSearch={setSearch}
          query={query}
          active={active}
          list={list}
          open={open}
          navigate={navigate}
          pages={pages}
          setPages={setPages}
        />
        <DetailPane
          now={now}
          current={current}
          selected={selected}
          setSelected={setSelected}
          detail={detail}
          detailPages={detailPages}
          setDetailPages={setDetailPages}
          open={open}
          live={live}
          status={status}
          expired={expired}
        />
      </div>
    </>
  );
};

const WorkspaceSession = ({ retry }: { retry: () => void }) => {
  const cache = useQueryClient();
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const expired = useCallback(() => {
    setUser(null);
    setError("Your session ended. Sign in again to continue.");
    void cache
      .cancelQueries({ queryKey: ["workspace"] })
      .then(() => cache.removeQueries({ queryKey: ["workspace"] }));
  }, [cache]);
  useEffect(() => {
    const controller = new AbortController();
    const load = async () => {
      try {
        const value = await accountRequest<User>(
          "/me",
          "GET",
          undefined,
          controller.signal
        );
        if (!controller.signal.aborted) {
          setUser(value);
          setError("");
        }
      } catch (error_) {
        if (!controller.signal.aborted) {
          setError(
            error_ instanceof AccountError && error_.status === 401
              ? "Sign in to see your synced workspace."
              : "Your workspace is temporarily unavailable. Please retry."
          );
        }
      }
      if (!controller.signal.aborted) {
        setLoading(false);
      }
    };
    void load();
    return () => {
      controller.abort();
      void cache
        .cancelQueries({ queryKey: ["workspace"] })
        .then(() => cache.removeQueries({ queryKey: ["workspace"] }));
    };
  }, [cache]);
  if (loading) {
    return (
      <Frame>
        <output className="block py-24 text-center">
          Opening your workspace…
        </output>
      </Frame>
    );
  }
  if (user) {
    return (
      <Frame>
        <Workspace key={user.id} user={user} expired={expired} />
      </Frame>
    );
  }
  return (
    <Frame>
      <section className="glass mx-auto mt-16 max-w-xl rounded-3xl p-8 text-center">
        <FolderOpen className="mx-auto mb-5 size-9 text-sky-deep" />
        <h1 className="font-display text-3xl">Your meetings. Everywhere.</h1>
        <output className="mt-4 block leading-7 text-ink-soft">{error}</output>
        <p className="mt-3 text-sm leading-6 text-ink-soft">
          Conversations, transcripts, attachments and live answers from your
          signed-in desktop, together in one place.
        </p>
        <div className="mt-7 flex flex-wrap justify-center gap-3">
          <Link to="/account" className={button}>
            Sign in to Lennette
          </Link>
          <button type="button" className={button} onClick={retry}>
            Retry
          </button>
        </div>
        <Link
          to="/"
          className="mt-6 inline-block text-sm underline underline-offset-4"
        >
          Back to Lennette
        </Link>
      </section>
    </Frame>
  );
};
export const WorkspacePage = () => {
  const [attempt, setAttempt] = useState(0);
  return (
    <WorkspaceSession key={attempt} retry={() => setAttempt((n) => n + 1)} />
  );
};
