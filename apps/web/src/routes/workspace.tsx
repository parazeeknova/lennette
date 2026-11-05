import { createFileRoute } from "@tanstack/react-router";

import { WorkspacePage } from "@/components/workspace/workspace-page";

export const Route = createFileRoute("/workspace")({
  head: () => ({
    meta: [
      { title: "Your workspace · Lennette" },
      { name: "robots", content: "noindex, nofollow" },
    ],
  }),
  component: WorkspacePage,
});
