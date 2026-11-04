import { createFileRoute } from "@tanstack/react-router";

import { LegalPage } from "@/components/legal-page";
import { TERMS } from "@/lib/legal";
import { pageHead } from "@/lib/seo";

export const Route = createFileRoute("/terms")({
  head: () => pageHead("/terms"),
  component: () => <LegalPage document={TERMS} />,
});
