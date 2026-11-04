import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { UpgradePlan } from "@/components/account/upgrade-plan";

const { request } = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("@/lib/account-api", () => ({
  accountRequest: request,
  paymentRedirect: vi.fn(),
}));
afterEach(() => {
  cleanup();
  request.mockReset();
});

it("requires an exact quote and explicit confirmation before charging", async () => {
  const updated = vi.fn().mockResolvedValue(undefined);
  request
    .mockResolvedValueOnce({ quote_id: "quote", amount: 2702, currency: "USD" })
    .mockResolvedValueOnce({ payment_url: null });
  render(<UpgradePlan eligible disabled={false} onUpdated={updated} />);
  fireEvent.click(
    screen.getByRole("button", { name: "Upgrade to Managed Plus" })
  );
  const confirm = await screen.findByRole("button", {
    name: "Confirm upgrade · $27.02",
  });
  expect(request).toHaveBeenCalledTimes(1);
  expect(request).toHaveBeenCalledWith("/billing/upgrade/quote", "POST");
  fireEvent.click(confirm);
  await waitFor(() =>
    expect(request).toHaveBeenCalledWith("/billing/upgrade", "POST", {
      quote_id: "quote",
    })
  );
  await waitFor(() => expect(updated).toHaveBeenCalledOnce());
});

it("canceling a quote never requests a charge", async () => {
  request.mockResolvedValue({
    quote_id: "quote",
    amount: 2290,
    currency: "USD",
  });
  render(<UpgradePlan eligible disabled={false} onUpdated={vi.fn()} />);
  fireEvent.click(
    screen.getByRole("button", { name: "Upgrade to Managed Plus" })
  );
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  expect(request).toHaveBeenCalledTimes(1);
  expect(
    screen.queryByRole("button", { name: /Confirm upgrade/u })
  ).not.toBeInTheDocument();
});
