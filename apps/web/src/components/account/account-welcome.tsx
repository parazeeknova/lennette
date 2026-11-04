import { Link } from "@tanstack/react-router";

const GoogleMark = () => (
  <svg aria-hidden="true" viewBox="0 0 24 24" className="size-5 shrink-0">
    <path
      fill="#4285F4"
      d="M21.6 12.23c0-.71-.06-1.39-.18-2.05H12v3.88h5.38a4.6 4.6 0 0 1-2 3.02v2.51h3.24c1.9-1.75 2.98-4.33 2.98-7.36Z"
    />
    <path
      fill="#34A853"
      d="M12 22c2.7 0 4.96-.9 6.62-2.41l-3.24-2.51c-.9.6-2.05.96-3.38.96-2.61 0-4.82-1.76-5.61-4.12H3.05v2.59A10 10 0 0 0 12 22Z"
    />
    <path
      fill="#FBBC05"
      d="M6.39 13.92a6 6 0 0 1 0-3.84V7.49H3.05a10 10 0 0 0 0 9.02l3.34-2.59Z"
    />
    <path
      fill="#EA4335"
      d="M12 5.96c1.47 0 2.79.5 3.82 1.5l2.87-2.87A9.6 9.6 0 0 0 12 2a10 10 0 0 0-8.95 5.49l3.34 2.59A5.99 5.99 0 0 1 12 5.96Z"
    />
  </svg>
);

const GithubMark = () => (
  <svg
    aria-hidden="true"
    viewBox="0 0 24 24"
    fill="currentColor"
    className="size-5 shrink-0"
  >
    <path d="M12 .3A12 12 0 0 0 8.2 23.7c.6.1.8-.3.8-.6v-2.2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.8-1.3-1.8-1.1-.8.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0C17 5 18 5.3 18 5.3c.6 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6A12 12 0 0 0 12 .3Z" />
  </svg>
);

const MeetingPreview = () => (
  <div className="account-scene account-scene-meeting" aria-hidden="true">
    <div className="account-mini-window">
      <div className="account-window-top">
        <span className="account-live-dot" /> LIVE TRANSCRIPT{" "}
        <span className="ml-auto opacity-50">00:42</span>
      </div>
      <div className="account-wave">
        {Array.from({ length: 19 }, (_, i) => (
          <i
            key={i}
            style={{
              height: `${12 + ((i * 13) % 30)}px`,
              animationDelay: `${i * -0.12}s`,
            }}
          />
        ))}
      </div>
      <p className="text-[11px] text-ink-soft">What are our next steps?</p>
      <div className="account-ai-reply">
        <span className="text-sky-deep">✦ Lennette</span>
        <p className="mt-2">Here’s a clear plan for what comes next.</p>
        <div className="account-text-lines">
          <i />
          <i />
        </div>
      </div>
    </div>
  </div>
);

const SyncPreview = () => (
  <div className="account-scene account-scene-sync" aria-hidden="true">
    <div className="account-desktop">
      <div className="account-window-top">
        YOUR MEETING <span className="account-live-dot ml-auto" />
      </div>
      <div className="account-text-lines">
        <i />
        <i />
        <i />
      </div>
      <div className="account-sync-answer">✦ Here’s the context.</div>
    </div>
    <div className="account-phone">
      <div className="mx-auto mb-4 h-1 w-7 rounded-full bg-ink/15" />
      <div className="account-text-lines">
        <i />
        <i />
        <i />
      </div>
      <div className="account-sync-answer">✦ All caught up.</div>
    </div>
    <span className="account-sync-badge">✓ Up to date</span>
  </div>
);

const LibraryPreview = () => (
  <div className="account-scene account-scene-library" aria-hidden="true">
    <div className="account-file account-file-back">
      <span className="account-file-type">PDF</span>
      <p>Project brief</p>
      <div className="account-text-lines">
        <i />
        <i />
        <i />
      </div>
    </div>
    <div className="account-file account-file-front">
      <span className="account-file-type">NOTES</span>
      <p>Everything in context.</p>
      <div className="account-text-lines">
        <i />
        <i />
        <i />
      </div>
      <span className="mt-4 inline-block rounded-full bg-sky-deep/10 px-3 py-1 text-[9px] text-sky-deep">
        Saved to your workspace
      </span>
    </div>
  </div>
);

export const AccountWelcome = ({
  busy,
  onSignIn,
}: {
  busy: boolean;
  onSignIn: (provider: string) => Promise<void>;
}) => (
  <>
    <section className="account-welcome glass rounded-[2rem] p-7 sm:p-10">
      <div className="relative grid items-center gap-8 md:grid-cols-[1fr_320px] md:gap-12">
        <div>
          <p className="mb-4 text-xs font-semibold tracking-widest text-sky-deep uppercase">
            Your Lennette workspace
          </p>
          <h2 className="max-w-lg font-display text-3xl leading-tight sm:text-4xl">
            Less configuring.
            <br />
            More being present.
          </h2>
          <p className="mt-5 max-w-md text-sm leading-relaxed text-ink-soft">
            One account for your meetings, your context, and your next good
            answer. Sign in to get started, then choose the managed plan that
            fits your day.
          </p>
          <Link
            to="/pricing"
            className="mt-6 inline-flex items-center gap-3 text-sm font-semibold text-sky-deep"
          >
            Explore managed plans <span aria-hidden="true">↗</span>
          </Link>
        </div>
        <div className="rounded-3xl border border-white/70 bg-white/40 p-6">
          <h3 className="font-display text-2xl">Make yourself at home.</h3>
          <p className="mt-2 text-xs leading-relaxed text-ink-soft">
            New here? Your first sign-in creates your account.
          </p>
          <div className="mt-6 grid gap-3">
            <button
              type="button"
              disabled={busy}
              onClick={() => onSignIn("google")}
              className="account-provider"
            >
              <GoogleMark />
              <span>Continue with Google</span>
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => onSignIn("github")}
              className="account-provider"
            >
              <GithubMark />
              <span>Continue with GitHub</span>
            </button>
          </div>
          <p className="mt-4 text-[11px] leading-relaxed text-ink-soft">
            Free to create. Paid features require a subscription. By continuing,
            you agree to our{" "}
            <Link to="/terms" className="underline underline-offset-2">
              Terms
            </Link>{" "}
            and acknowledge our{" "}
            <Link to="/privacy" className="underline underline-offset-2">
              Privacy Policy
            </Link>
            .
          </p>
        </div>
      </div>
    </section>
    <section aria-labelledby="managed-features" className="pt-9">
      <div className="mb-7 text-center">
        <p className="mb-3 text-xs font-semibold tracking-widest text-sky-deep uppercase">
          Unlocked with a managed subscription
        </p>
        <h2 id="managed-features" className="font-display text-3xl sm:text-4xl">
          One less thing between you and the meeting.
        </h2>
      </div>
      <div className="grid gap-5 md:grid-cols-3">
        {[
          {
            Preview: MeetingPreview,
            title: "AI, without the setup",
            description:
              "Fast AI answers and live transcription, together. No provider accounts, API keys, or separate API bills to manage.",
          },
          {
            Preview: SyncPreview,
            title: "Pick up on another device",
            description:
              "Your conversations, live transcripts, and AI responses stay in sync across signed-in devices, even during a meeting.",
          },
          {
            Preview: LibraryPreview,
            title: "Keep the context close",
            description:
              "Bring your history, prompts, and supported attachments together. You choose whether to import existing local history.",
          },
        ].map(({ Preview, title, description }) => (
          <article
            key={title}
            className="account-feature glass overflow-hidden rounded-3xl"
          >
            <Preview />
            <div className="relative px-6 pt-2 pb-8">
              <h3 className="font-display text-2xl leading-tight">{title}</h3>
              <p className="mt-4 text-sm leading-relaxed text-ink-soft">
                {description}
              </p>
            </div>
          </article>
        ))}
      </div>
      <p className="mt-4 text-center text-[11px] text-ink-soft">
        Illustrative previews. Usage, storage, and device limits depend on your
        plan.
      </p>
    </section>
  </>
);
