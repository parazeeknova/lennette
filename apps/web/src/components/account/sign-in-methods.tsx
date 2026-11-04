export const SignInMethods = ({
  providers,
  busy,
  onConnect,
}: {
  providers: string[];
  busy: boolean;
  onConnect: (provider: string) => Promise<void>;
}) => (
  <section className="glass rounded-3xl p-6 sm:p-8">
    <h2 className="text-xl font-semibold">Sign-in methods</h2>
    <p className="mt-3 text-sm leading-relaxed text-ink-soft">
      Connect Google and GitHub to use either with this account. Your
      subscription and meeting history stay together. For security, connect a
      provider within 10 minutes of signing in.
    </p>
    <ul className="mt-4 divide-y divide-ink/10">
      {(
        [
          ["google", "Google"],
          ["github", "GitHub"],
        ] as const
      ).map(([provider, label]) => (
        <li
          key={provider}
          className="flex items-center justify-between gap-4 py-4"
        >
          <span className="font-medium">{label}</span>
          {providers.includes(provider) ? (
            <span className="text-sm text-sky-deep">Connected</span>
          ) : (
            <button
              type="button"
              disabled={busy}
              onClick={() => onConnect(provider)}
              className="glass btn-liquid glass-pill text-sm disabled:opacity-50"
            >
              Connect {label}
            </button>
          )}
        </li>
      ))}
    </ul>
    <p className="mt-3 text-xs leading-relaxed text-ink-soft">
      You’ll confirm access with the provider. An identity already connected to
      another Lennette account cannot be moved here automatically.
    </p>
  </section>
);
