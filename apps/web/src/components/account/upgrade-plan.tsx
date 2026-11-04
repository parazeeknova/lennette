import { useState } from "react";

import { accountRequest, paymentRedirect } from "@/lib/account-api";

interface Quote {
  quote_id: string;
  amount: number;
  currency: string;
}
const formatCharge = (quote: Quote) => {
  const format = new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: quote.currency,
  });
  const digits = format.resolvedOptions().maximumFractionDigits ?? 2;
  return format.format(quote.amount / 10 ** digits);
};
const button = "glass btn-liquid glass-pill text-sm disabled:opacity-50";

export const UpgradePlan = ({
  eligible,
  disabled,
  onUpdated,
}: {
  eligible: boolean;
  disabled: boolean;
  onUpdated: () => Promise<void>;
}) => {
  const [quote, setQuote] = useState<Quote | null>(null);
  const [working, setWorking] = useState(false);
  const [notice, setNotice] = useState("");
  const review = async () => {
    setWorking(true);
    setNotice("");
    try {
      setQuote(await accountRequest<Quote>("/billing/upgrade/quote", "POST"));
    } catch (error) {
      setNotice(
        error instanceof Error
          ? error.message
          : "Upgrade quote unavailable. Please try again."
      );
    }
    setWorking(false);
  };
  const confirm = async () => {
    if (!quote) {
      return;
    }
    setWorking(true);
    setNotice("");
    try {
      const result = await accountRequest<{ payment_url: string | null }>(
        "/billing/upgrade",
        "POST",
        { quote_id: quote.quote_id }
      );
      setQuote(null);
      if (result.payment_url) {
        paymentRedirect(result.payment_url);
      } else {
        setNotice(
          "Upgrade requested. Plus access will appear after payment is confirmed."
        );
      }
      await onUpdated();
    } catch (error) {
      setQuote(null);
      setNotice(
        error instanceof Error
          ? error.message
          : "Upgrade could not be confirmed. Check your billing portal before retrying."
      );
    }
    setWorking(false);
  };
  if (!eligible) {
    return null;
  }
  return (
    <div className="mt-6 rounded-2xl border border-white/70 bg-white/35 p-5">
      <h3 className="font-display text-2xl">A little more room? Go Plus.</h3>
      <p className="mt-3 text-sm leading-relaxed text-ink-soft">
        Upgrade for the $22.90 price difference before applicable taxes, rather
        than paying another $40. Review your exact charge before confirming.
      </p>
      {quote ? (
        <div className="mt-5 space-y-4">
          <p className="font-semibold">Due now: {formatCharge(quote)}</p>
          <p className="text-xs leading-relaxed text-ink-soft">
            Dodo’s quote includes applicable taxes and credits. Confirming
            requests a charge to your saved payment method. Monthly billing
            restarts when the upgrade takes effect, then renews at $40/month
            plus applicable taxes. Your existing plan stays active until payment
            succeeds.
          </p>
          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              disabled={disabled || working}
              className={button}
              onClick={confirm}
            >
              Confirm upgrade · {formatCharge(quote)}
            </button>
            <button
              type="button"
              disabled={working}
              className={button}
              onClick={() => setQuote(null)}
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <button
          type="button"
          disabled={disabled || working}
          className={`${button} mt-5`}
          onClick={review}
        >
          {working ? "Checking upgrade…" : "Upgrade to Managed Plus"}
        </button>
      )}
      {notice && (
        <p role="alert" className="mt-4 text-sm leading-relaxed text-ink-soft">
          {notice}
        </p>
      )}
    </div>
  );
};
