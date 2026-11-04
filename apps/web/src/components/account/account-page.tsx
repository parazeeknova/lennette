import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { PageShell } from "@/components/page-shell";
import {
  API_URL,
  AccountError,
  accountRequest,
  paymentRedirect,
} from "@/lib/account-api";

import { AccountWelcome } from "./account-welcome";

interface User {
  id: string;
  email: string;
  name: string;
}
interface Device {
  session_id: string;
  name: string;
  created_at: string;
  current: boolean;
}
interface Usage {
  subscription: { plan: string; status: string; period_end: string } | null;
  limits: {
    ai_microdollars: number;
    transcription_seconds: number;
    storage_bytes: number;
  } | null;
  ai_used_microdollars: number;
  transcription_seconds: number;
  storage_bytes: number;
}
interface Snapshot {
  user: User;
  usage: Usage;
  devices: Device[];
}
const button =
  "glass glass-pill px-5 py-3 text-sm font-semibold disabled:opacity-50";
const card = "glass rounded-3xl p-6 sm:p-8";
const number = (value: number) =>
  new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value);

const AccountLayout = ({
  signedIn,
  children,
}: {
  signedIn: boolean;
  children: ReactNode;
}) => (
  <PageShell
    title={signedIn ? "Your account" : "Your meetings. All together."}
    intro={
      signedIn
        ? "Manage your subscription, usage and signed-in devices."
        : "Create a Lennette account, then choose a managed plan for AI, live transcription, and meeting history that follows you across devices."
    }
  >
    <div
      className={`mx-auto space-y-6 text-ink ${signedIn ? "max-w-3xl" : "max-w-5xl"}`}
    >
      {children}
    </div>
  </PageShell>
);

const Meter = ({
  label,
  used,
  limit,
  unit,
}: {
  label: string;
  used: number;
  limit: number;
  unit: string;
}) => (
  <div className="space-y-2">
    <div className="flex justify-between gap-4 text-sm">
      <span>{label}</span>
      <span>
        {number(used)} / {number(limit)} {unit}
      </span>
    </div>
    <progress
      className="h-2 w-full accent-sky-deep"
      max={Math.max(limit, 1)}
      value={Math.min(used, limit)}
      aria-label={label}
    />
    <p className="text-xs text-ink-soft">
      {number(Math.max(0, limit - used))} {unit} remaining
    </p>
  </div>
);

export const AccountPage = () => {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [unavailable, setUnavailable] = useState(false);
  const [connected, setConnected] = useState(false);
  const [confirmation, setConfirmation] = useState("");
  const refreshing = useRef(false);
  const refreshAgain = useRef(false);
  const refresh = useCallback(async () => {
    if (refreshing.current) {
      refreshAgain.current = true;
      return;
    }
    refreshing.current = true;
    try {
      do {
        refreshAgain.current = false;
        // Events arriving during a fetch require one fresh snapshot after it completes.
        // eslint-disable-next-line no-await-in-loop
        const user = await accountRequest<User>("/me");
        // Only request private dashboard data after confirming a valid session.
        // eslint-disable-next-line no-await-in-loop
        const [usage, devices] = await Promise.all([
          accountRequest<Usage>("/billing/usage"),
          accountRequest<Device[]>("/devices"),
        ]);
        setSnapshot({ user, usage, devices });
        setUnavailable(false);
      } while (refreshAgain.current);
    } catch (error) {
      if (error instanceof AccountError && error.status === 401) {
        setSnapshot(null);
        setUnavailable(false);
      } else {
        setUnavailable(true);
      }
    }
    refreshing.current = false;
    setLoading(false);
  }, []);
  useEffect(() => {
    queueMicrotask(() => {
      refresh().catch(() => setNotice("Account unavailable. Please reload."));
    });
  }, [refresh]);
  const signedIn = snapshot !== null;
  useEffect(() => {
    if (!signedIn) {
      return;
    }
    const events = new EventSource(`${API_URL}/v1/account/events`, {
      withCredentials: true,
    });
    events.addEventListener("changed", () => {
      setConnected(true);
      void refresh();
    });
    events.addEventListener("expired", () => {
      events.close();
      setSnapshot(null);
      setNotice("Your session expired. Sign in again.");
    });
    events.addEventListener("error", () => setConnected(false));
    const onFocus = () => {
      void refresh();
    };
    window.addEventListener("focus", onFocus);
    let lastActivity = Date.now();
    const activity = () => {
      if (Date.now() - lastActivity < 60_000) {
        return;
      }
      lastActivity = Date.now();
      void accountRequest("/browser/activity", "POST").catch(() => {
        void refresh();
      });
    };
    window.addEventListener("pointerdown", activity);
    window.addEventListener("keydown", activity);
    return () => {
      events.close();
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("pointerdown", activity);
      window.removeEventListener("keydown", activity);
    };
  }, [signedIn, refresh]);

  const act = async (operation: () => Promise<void>) => {
    setBusy(true);
    setNotice("");
    try {
      await operation();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : "Please try again.");
    }
    setBusy(false);
  };
  const signIn = (provider: string) =>
    act(async () => {
      const result = await accountRequest<{ authorization_url: string }>(
        `/browser/auth/${provider}`,
        "POST"
      );
      const url = new URL(result.authorization_url);
      if (
        url.protocol !== "https:" ||
        !["accounts.google.com", "github.com"].includes(url.hostname)
      ) {
        throw new Error("Sign-in link was invalid.");
      }
      window.location.assign(url.href);
    });
  return (
    <AccountLayout signedIn={signedIn}>
      {notice && (
        <p role="alert" className="glass rounded-2xl p-4">
          {notice}
        </p>
      )}
      {loading && <output>Loading your account…</output>}
      {unavailable && (
        <div className="glass flex flex-wrap items-center justify-between gap-4 rounded-2xl p-5">
          <output className="max-w-xl text-sm text-ink-soft">
            Account services are temporarily unavailable. Please retry to sign
            in or load your dashboard.
          </output>
          <button
            type="button"
            className={button}
            disabled={busy}
            onClick={() => act(refresh)}
          >
            Try again
          </button>
        </div>
      )}
      {!loading && snapshot && (
        <>
          <section className={card}>
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div>
                <h2 className="text-xl font-semibold">{snapshot.user.name}</h2>
                <p className="text-ink-soft">{snapshot.user.email}</p>
              </div>
              <button
                type="button"
                className={button}
                disabled={busy}
                onClick={() =>
                  act(async () => {
                    await accountRequest("/browser/logout", "POST");
                    setSnapshot(null);
                  })
                }
              >
                Sign out
              </button>
            </div>
            <p className="mt-4 text-xs text-ink-soft" aria-live="polite">
              {connected
                ? "Live account updates connected"
                : "Reconnecting live updates…"}{" "}
              · Browser sessions expire after 30 minutes idle or 12 hours.
            </p>
          </section>
          <section className={card}>
            <h2 className="mb-3 text-xl font-semibold">
              Subscription & payments
            </h2>
            <p>
              {snapshot.usage.subscription
                ? `${snapshot.usage.subscription.plan === "managed_plus" ? "Managed Plus" : "Managed"} · ${snapshot.usage.subscription.status}`
                : "Local / bring your own keys"}
            </p>
            {snapshot.usage.subscription && (
              <p className="mt-1 text-sm text-ink-soft">
                Current billing period ends{" "}
                {new Date(
                  snapshot.usage.subscription.period_end
                ).toLocaleDateString()}
                .
              </p>
            )}
            <div className="mt-5 flex flex-wrap gap-3">
              {snapshot.usage.subscription ? (
                <button
                  type="button"
                  className={button}
                  disabled={busy}
                  onClick={() =>
                    act(async () => {
                      const result = await accountRequest<{ url: string }>(
                        "/billing/portal",
                        "POST"
                      );
                      paymentRedirect(result.url);
                    })
                  }
                >
                  Manage subscription & payment method
                </button>
              ) : (
                ["managed", "managed_plus"].map((plan) => (
                  <button
                    type="button"
                    key={plan}
                    className={button}
                    disabled={busy}
                    onClick={() =>
                      act(async () => {
                        const result = await accountRequest<{
                          checkout_url: string;
                        }>("/billing/checkout", "POST", { plan });
                        paymentRedirect(result.checkout_url);
                      })
                    }
                  >
                    {plan === "managed"
                      ? "Managed · $17.10/month"
                      : "Managed Plus · $40/month"}
                  </button>
                ))
              )}
            </div>
            <p className="mt-4 text-xs text-ink-soft">
              Checkout and payment details are handled securely by Dodo
              Payments.
            </p>
          </section>
          <section className={`${card} space-y-5`}>
            <h2 className="text-xl font-semibold">Current usage</h2>
            <Meter
              label="AI credits"
              used={snapshot.usage.ai_used_microdollars / 1000}
              limit={(snapshot.usage.limits?.ai_microdollars ?? 0) / 1000}
              unit="credits"
            />
            <Meter
              label="Meeting transcription"
              used={snapshot.usage.transcription_seconds / 60}
              limit={(snapshot.usage.limits?.transcription_seconds ?? 0) / 60}
              unit="minutes"
            />
            <Meter
              label="Synced storage"
              used={snapshot.usage.storage_bytes / 1e9}
              limit={(snapshot.usage.limits?.storage_bytes ?? 0) / 1e9}
              unit="GB"
            />
            <p className="text-xs text-ink-soft">
              AI credits depend on the selected model and the amount of text
              processed. Storage covers synced text and attachments.
            </p>
          </section>
          <section className={card}>
            <h2 className="mb-4 text-xl font-semibold">Signed-in devices</h2>
            <ul className="divide-y divide-ink/10">
              {snapshot.devices.map((device) => (
                <li
                  key={device.session_id}
                  className="flex items-center justify-between gap-4 py-4"
                >
                  <div>
                    <p className="font-medium">
                      {device.name}
                      {device.current ? " · This session" : ""}
                    </p>
                    <p className="text-xs text-ink-soft">
                      Signed in {new Date(device.created_at).toLocaleString()}
                    </p>
                  </div>
                  <button
                    type="button"
                    className={button}
                    disabled={busy}
                    onClick={() =>
                      act(async () => {
                        await accountRequest(
                          `/devices/${device.session_id}`,
                          "DELETE"
                        );
                        await refresh();
                      })
                    }
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          </section>
          <section className={card}>
            <h2 className="text-xl font-semibold">Delete account</h2>
            <p className="mt-3 text-sm text-ink-soft">
              Permanently cancel your subscription and delete your server
              account, conversations, transcripts, prompts, settings and R2
              attachments. Local copies and previously exported files on your
              devices remain on those devices. This cannot be undone.
            </p>
            <label className="mt-5 block text-sm" htmlFor="delete-confirm">
              Type DELETE to confirm
            </label>
            <input
              id="delete-confirm"
              value={confirmation}
              onChange={(e) => setConfirmation(e.target.value)}
              autoComplete="off"
              className="mt-2 w-full rounded-xl border border-ink/20 bg-white/50 p-3"
            />
            <button
              type="button"
              className={`${button} mt-4 text-red-800`}
              disabled={busy || confirmation !== "DELETE"}
              onClick={() =>
                act(async () => {
                  await accountRequest("/me", "DELETE");
                  setSnapshot(null);
                  setConfirmation("");
                  setNotice(
                    "Your account has been deleted. Private attachment erasure is queued and retries automatically if storage is temporarily unavailable."
                  );
                })
              }
            >
              Permanently delete account
            </button>
          </section>
        </>
      )}
      {!loading && !snapshot && (
        <AccountWelcome busy={busy} onSignIn={signIn} />
      )}
    </AccountLayout>
  );
};
