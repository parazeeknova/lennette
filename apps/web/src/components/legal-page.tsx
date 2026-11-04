import { Link } from "@tanstack/react-router";

import { PageShell } from "./page-shell";

export interface LegalSection {
  id: string;
  title: string;
  paragraphs: string[];
}

export interface LegalDocument {
  kind: "terms" | "privacy";
  title: string;
  intro: string;
  summary: string;
  sections: LegalSection[];
}

export const LEGAL_UPDATED = "November 4, 2026";
export const LEGAL_EMAIL = "harsh@itssingularity.com";

export const LegalPage = ({ document }: { document: LegalDocument }) => (
  <PageShell
    breadcrumb={document.title}
    title={document.title}
    intro={document.intro}
  >
    <div className="mb-8 flex flex-wrap items-center justify-between gap-4">
      <nav
        aria-label="Legal documents"
        className="glass inline-flex glass-pill p-1.5"
      >
        {(
          [
            ["terms", "/terms", "Terms of Service"],
            ["privacy", "/privacy", "Privacy Policy"],
          ] as const
        ).map(([kind, to, label]) => (
          <Link
            key={kind}
            to={to}
            aria-current={document.kind === kind ? "page" : undefined}
            className={`rounded-full px-4 py-2.5 text-sm font-medium transition-colors ${document.kind === kind ? "bg-white/70 text-sky-deep shadow-sm" : "text-ink-soft hover:text-ink"}`}
          >
            {label}
          </Link>
        ))}
      </nav>
      <p className="text-xs tracking-wide text-ink-soft">
        Last updated <time dateTime="2026-11-04">{LEGAL_UPDATED}</time>
      </p>
    </div>

    <div className="grid items-start gap-7 lg:grid-cols-[220px_minmax(0,1fr)] lg:gap-10">
      <aside className="lg:sticky lg:top-8 lg:self-start">
        <nav
          aria-label="On this page"
          className="glass rounded-3xl p-5 lg:max-h-[calc(100dvh-4rem)] lg:overflow-y-auto lg:overscroll-contain"
        >
          <p className="mb-4 px-2 text-xs font-semibold tracking-widest text-sky-deep uppercase">
            On this page
          </p>
          <ol className="grid gap-1 sm:grid-cols-2 lg:grid-cols-1">
            {document.sections.map((section, index) => (
              <li key={section.id}>
                <a
                  href={`#${section.id}`}
                  className="flex gap-3 rounded-xl px-2 py-2 text-sm leading-snug text-ink-soft transition-colors hover:bg-white/45 hover:text-ink"
                >
                  <span
                    aria-hidden="true"
                    className="pt-0.5 text-[0.65rem] text-sky-deep/70"
                  >
                    {String(index + 1).padStart(2, "0")}
                  </span>
                  <span>{section.title}</span>
                </a>
              </li>
            ))}
          </ol>
        </nav>
      </aside>

      <article className="glass min-w-0 rounded-3xl px-6 py-8 sm:px-10 sm:py-10">
        <div className="mb-10 border-b border-ink/10 pb-8">
          <p className="mb-3 text-xs font-semibold tracking-widest text-sky-deep uppercase">
            The essentials
          </p>
          <p className="text-lg leading-relaxed text-ink">{document.summary}</p>
        </div>
        <div className="space-y-10">
          {document.sections.map((section, index) => (
            <section
              key={section.id}
              id={section.id}
              aria-labelledby={`${section.id}-heading`}
              className="scroll-mt-8"
            >
              <div className="mb-4 flex items-baseline gap-3">
                <span
                  aria-hidden="true"
                  className="font-display text-lg text-sky-deep/65"
                >
                  {String(index + 1).padStart(2, "0")}
                </span>
                <h2
                  id={`${section.id}-heading`}
                  className="font-display text-[clamp(1.5rem,3vw,1.9rem)] leading-tight text-ink"
                >
                  {section.title}
                </h2>
              </div>
              <div className="space-y-4 text-[0.95rem] leading-[1.85] text-ink-soft">
                {section.paragraphs.map((paragraph) => (
                  <p key={paragraph}>{paragraph}</p>
                ))}
              </div>
            </section>
          ))}
        </div>
        <div className="mt-12 rounded-2xl border border-sky-deep/10 bg-white/40 p-5 sm:p-6">
          <h2 className="font-display text-xl text-ink">
            A question about{" "}
            {document.kind === "terms" ? "these terms" : "your privacy"}?
          </h2>
          <p className="mt-2 text-sm leading-relaxed text-ink-soft">
            Contact Singularity Works at{" "}
            <a
              href={`mailto:${LEGAL_EMAIL}`}
              className="break-all text-sky-deep underline decoration-sky-deep/30 underline-offset-4"
            >
              {LEGAL_EMAIL}
            </a>
            .
          </p>
        </div>
      </article>
    </div>
  </PageShell>
);
