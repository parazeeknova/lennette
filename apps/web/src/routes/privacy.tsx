import { createFileRoute } from "@tanstack/react-router";

import { LegalPage } from "@/components/legal-page";
import { PRIVACY } from "@/lib/legal";
import { pageHead } from "@/lib/seo";

export const Route = createFileRoute("/privacy")({
  head: () => pageHead("/privacy"),
  component: () => <LegalPage document={PRIVACY} />,
});
