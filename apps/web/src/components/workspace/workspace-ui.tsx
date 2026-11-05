import { ArrowLeft, ArrowRight } from "lucide-react";

export const button =
  "glass glass-pill inline-flex min-h-11 items-center justify-center gap-2 px-4 py-2.5 text-sm font-semibold disabled:cursor-not-allowed disabled:opacity-40 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-sky-deep";
export const paper =
  "rounded-2xl border border-white/70 bg-white/60 p-4 sm:p-5";

export const Pager = ({
  label = "History pages",
  older,
  previous,
  busy,
  back,
  forward,
}: {
  label?: string;
  older: string | null;
  previous: boolean;
  busy: boolean;
  back: () => void;
  forward: (cursor: string) => void;
}) =>
  older || previous ? (
    <nav
      aria-label={label}
      className="mt-5 flex items-center justify-between gap-2"
    >
      <button
        type="button"
        className={button}
        disabled={!previous || busy}
        onClick={back}
      >
        <ArrowLeft className="size-4" />
        Newer
      </button>
      <button
        type="button"
        className={button}
        disabled={!older || busy}
        onClick={() => {
          if (older) {
            forward(older);
          }
        }}
      >
        Older
        <ArrowRight className="size-4" />
      </button>
    </nav>
  ) : null;
