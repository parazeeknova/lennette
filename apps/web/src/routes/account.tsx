import { createFileRoute } from "@tanstack/react-router";

import { AccountPage } from "@/components/account/account-page";

export const Route = createFileRoute("/account")({
  head: () => ({
    meta: [
      { title: "Your account · Lennette" },
      { name: "robots", content: "noindex, nofollow" },
    ],
  }),
  component: AccountPage,
});
