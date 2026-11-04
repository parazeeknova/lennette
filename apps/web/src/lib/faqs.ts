import { MANAGED_PLAN } from "./site";

export interface FaqEntry {
  question: string;
  answer: string;
}

export const HOME_FAQS: readonly FaqEntry[] = [
  {
    question: "Does Lennette join my call as a bot?",
    answer:
      "No. Lennette runs on your computer and captures microphone and system audio without adding a bot to your meeting. Live transcription and AI answers are processed by your configured providers.",
  },
  {
    question: "Can others see Lennette when I share my screen?",
    answer:
      "Windows capture exclusion works with capture methods that honor display affinity. On Linux, it requires our patched Hyprland setup. Ordinary Linux sessions do not hide the overlay. Test your meeting app and share mode before relying on it.",
  },
  {
    question: "Which meeting apps can I use with Lennette?",
    answer:
      "Lennette captures audio from your computer rather than connecting to a specific meeting service. Configure microphone and system audio for your device, then test it with your meeting app.",
  },
  {
    question: "How do I ask for help during a meeting?",
    answer:
      "Show or hide the overlay with Ctrl + Shift + Backslash. Type a question, or start listening to get suggestions for recognized questions using the meeting transcript and your selected context.",
  },
  {
    question: "Where are my conversations and transcripts stored?",
    answer:
      "Local mode keeps history on your device. Managed accounts sync conversations, transcripts, prompts, preferences and attachments across devices, with a choice to import existing history. Relevant context goes to the AI provider when you request an answer. Lennette does not store raw meeting audio.",
  },
];

export const PRICING_FAQS: readonly FaqEntry[] = [
  {
    question: "Are subscription payments refundable?",
    answer:
      "Paid subscriptions, renewals and upgrades are non-refundable, including unused time or usage, except where required by law. Cancel before your next renewal to prevent future charges. For duplicate or incorrect charges, contact harsh@itssingularity.com with your payment reference.",
  },
  {
    question: "What do I pay for on the free plan?",
    answer:
      "Nothing to Lennette. Add your own AI provider key for answers and a Deepgram key for live transcription. Those providers bill you directly for API usage. You do not need a Lennette account.",
  },
  {
    question: "What will Managed include?",
    answer: `The planned ${MANAGED_PLAN.price} monthly subscription includes fast models, 2,000 model-weighted AI credits and 10 GB of synced storage across three devices. Managed Plus at $40 includes fast models, 6,000 credits, a higher meeting allowance and 50 GB across six devices. Meeting allowances are being finalized before launch. AI credits vary with the model, context length and generated text; follow-ups use the same allowance. You will not need separate provider keys.`,
  },
  {
    question: "Does my audio stay on my device?",
    answer:
      "Live transcription sends audio to your speech provider. AI requests send relevant context and selected attachments to your AI provider. In local mode, history stays on your device. Managed accounts sync text, settings and attachments to Lennette servers, with a choice to import existing history. Managed speech opts out of Deepgram model training, and Lennette does not store raw meeting audio.",
  },
  {
    question: "Can I sign up for Managed now?",
    answer:
      "Managed access is coming soon. Join the waitlist to hear when it opens. You can download Lennette and use your own keys today.",
  },
];
