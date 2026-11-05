import { Link, createFileRoute } from "@tanstack/react-router";

import { FaqAccordion } from "@/components/faq-accordion";
import { PageShell } from "@/components/page-shell";
import { PRICING_FAQS } from "@/lib/faqs";
import { pageHead } from "@/lib/seo";
import { MANAGED_PLAN, MANAGED_PLUS_PLAN } from "@/lib/site";

const MANAGED_MONTHLY_PRICE = MANAGED_PLAN.price;

interface Plan {
  name: string;
  price: string;
  cadence: string;
  summary: string;
  note: string;
  features: string[];
  cta: {
    label: string;
    to: "/downloads" | "/account";
    hash?: string;
  };
  emphasis: boolean;
}

const plans: Plan[] = [
  {
    name: "Free · Bring your own keys",
    price: "$0",
    cadence: "forever",
    summary:
      "Use Lennette with your own AI and transcription API keys. Choose your providers and pay them directly for what you use.",
    note: "Lennette is free. Your providers may charge for API usage.",
    features: [
      "Full desktop app",
      "Your choice of supported AI models",
      "Live transcription with your Deepgram key",
      "Chat and meeting history stored locally",
      "Windows, Linux and macOS builds",
    ],
    cta: {
      label: "Download Lennette",
      to: "/downloads",
    },
    emphasis: false,
  },
  {
    name: "Managed",
    price: MANAGED_MONTHLY_PRICE,
    cadence: "per month",
    summary:
      "The full Lennette experience, with AI answers and live transcription included in one monthly plan. No provider keys to manage.",
    note: "Billed monthly in USD. Cancel before your next renewal.",
    features: [
      "2,000 model-weighted AI credits per month",
      "300 minutes of live transcription per month",
      "Fast models for live AI answers",
      "Speaker labels in your transcripts",
      "10 GB of synced text and attachments · 3 devices",
      "AI and transcription API costs included",
    ],
    cta: {
      label: "Get Managed",
      to: "/account",
      hash: "subscription",
    },
    emphasis: true,
  },
  {
    name: MANAGED_PLUS_PLAN.name,
    price: MANAGED_PLUS_PLAN.price,
    cadence: "per month",
    summary:
      "More meeting time, more AI credits and fast models for live answers.",
    note: "Billed monthly in USD. Cancel before your next renewal.",
    features: [
      "6,000 model-weighted AI credits per month",
      "1,200 minutes of live transcription per month",
      "Fast models with a larger AI allowance",
      "50 GB of synced text and attachments · 6 devices",
      "Live transcripts and answers across your devices",
      "AI and transcription API costs included",
    ],
    cta: { label: "Get Managed Plus", to: "/account", hash: "subscription" },
    emphasis: false,
  },
];

const PlanCard = ({ plan }: { plan: Plan }) => {
  const ctaClass = [
    "glass glass-pill btn-liquid mt-8 w-full text-[0.95rem]",
    plan.emphasis
      ? "glass-blue text-on-sky hover:bg-[oklch(0.4_0.14_252/0.25)]"
      : "",
  ]
    .filter(Boolean)
    .join(" ");

  const ctaLabel = <span>{plan.cta.label}</span>;

  return (
    <article
      className={`flex flex-col rounded-3xl p-7 sm:p-8 ${
        plan.emphasis ? "glass glass-blue text-on-sky" : "glass text-ink"
      }`}
    >
      <div className="relative z-10 flex flex-1 flex-col">
        <h2
          className={`font-display text-2xl ${
            plan.emphasis ? "text-on-sky" : "text-ink"
          }`}
        >
          {plan.name}
        </h2>

        <p className="mt-6 flex flex-wrap items-baseline gap-x-2 gap-y-2">
          <span
            className={`font-display text-5xl leading-none ${
              plan.emphasis ? "text-on-sky" : "text-ink"
            }`}
          >
            {plan.price}
          </span>
          <span
            className={plan.emphasis ? "text-on-sky-soft" : "text-ink-soft"}
          >
            {plan.cadence}
          </span>
        </p>

        <p
          className={`mt-4 min-h-22 leading-relaxed ${
            plan.emphasis ? "text-on-sky-soft" : "text-ink-soft"
          }`}
        >
          {plan.summary}
        </p>

        <ul
          className={`mt-6 flex flex-1 flex-col gap-2.5 text-[0.95rem] ${
            plan.emphasis ? "text-on-sky-soft" : "text-ink-soft"
          }`}
        >
          {plan.features.map((feature) => (
            <li key={feature} className="flex items-start gap-2.5">
              <span
                aria-hidden="true"
                className={`mt-[0.45rem] size-1.5 shrink-0 rounded-full ${
                  plan.emphasis ? "bg-on-sky/70" : "bg-sky-deep/60"
                }`}
              />
              <span>{feature}</span>
            </li>
          ))}
        </ul>

        <p
          className={`mt-6 text-sm leading-relaxed ${
            plan.emphasis ? "text-on-sky-soft" : "text-ink-soft"
          }`}
        >
          {plan.note}
        </p>

        <Link
          to={plan.cta.to}
          {...(plan.cta.hash ? { hash: plan.cta.hash } : {})}
          className={ctaClass}
        >
          {ctaLabel}
        </Link>
      </div>
    </article>
  );
};

const PricingPage = () => (
  <PageShell
    breadcrumb="Pricing"
    titleIcon={["Simple", "pricing."]}
    intro={`Bring your own keys and use Lennette for free. Prefer everything included? Get Managed from ${MANAGED_MONTHLY_PRICE} a month, with AI and live transcription included.`}
  >
    <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-3">
      {plans.map((plan) => (
        <PlanCard key={plan.name} plan={plan} />
      ))}
    </div>

    <p className="mt-6 text-sm leading-relaxed text-ink-soft">
      Paid subscriptions, renewals and upgrades are non-refundable, including
      unused time or usage, except where required by law. Cancel before your
      next renewal to prevent future charges. See our{" "}
      <Link to="/terms" hash="refunds" className="underline underline-offset-4">
        refund terms
      </Link>
      .
    </p>

    <section className="mt-16">
      <h2 className="font-display text-2xl text-ink">
        Questions about pricing
      </h2>
      <FaqAccordion items={PRICING_FAQS} />
    </section>
  </PageShell>
);

export const Route = createFileRoute("/pricing")({
  head: () => pageHead("/pricing", PRICING_FAQS),
  component: PricingPage,
});
