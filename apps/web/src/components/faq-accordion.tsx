import type { FaqEntry } from "@/lib/faqs";

export const FaqAccordion = ({ items }: { items: readonly FaqEntry[] }) => (
  <div className="mt-14 border-t border-ink/10">
    {items.map((faq, index) => (
      <details
        key={faq.question}
        className="group border-b border-ink/10"
        open={index === 0}
      >
        <summary className="flex cursor-pointer list-none items-center justify-between gap-8 py-5 text-left text-lg font-medium text-ink transition-colors hover:text-sky-deep">
          <span>{faq.question}</span>
          <svg
            aria-hidden="true"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            className="size-5 shrink-0 text-ink-soft transition-transform duration-300 group-open:rotate-180"
          >
            <path d="m6 9 6 6 6-6" />
          </svg>
        </summary>
        <p className="max-w-2xl pb-6 text-ink-soft">{faq.answer}</p>
      </details>
    ))}
  </div>
);
