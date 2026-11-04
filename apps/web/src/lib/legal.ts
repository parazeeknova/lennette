import type { LegalDocument } from "@/components/legal-page";

export const TERMS: LegalDocument = {
  kind: "terms",
  title: "Terms of Service",
  intro:
    "A few ground rules for using Lennette, your account, and the services that help it run.",
  summary:
    "You keep ownership of your content. Use Lennette with the permissions your meetings require, check AI-generated answers, and manage paid access from your account.",
  sections: [
    {
      id: "agreement",
      title: "Using Lennette",
      paragraphs: [
        "These terms govern your use of the Lennette website, desktop application, and managed services operated under Singularity Works (“Lennette”, “we”, or “us”). By using the service, you agree to these terms. If you use Lennette for an organization, you must have authority to agree on its behalf.",
        "Lennette is intended for people aged 18 or older. You must also meet any higher age or other eligibility requirements that apply to you. The Privacy Policy explains how information is handled when you use the service.",
      ],
    },
    {
      id: "local-managed",
      title: "Local and managed modes",
      paragraphs: [
        "In local or bring-your-own-key mode, you manage your own AI and transcription accounts, credentials, and provider charges. You are responsible for following those providers’ terms. Local storage does not mean offline AI: requesting AI answers or live transcription sends relevant content to the provider you select.",
        "Managed features require an account and, where applicable, an active subscription. Availability, included usage, storage limits, and supported features are described on the pricing page and at checkout. Features described as planned or coming soon are not guaranteed to be available before launch.",
      ],
    },
    {
      id: "account-security",
      title: "Your account and security",
      paragraphs: [
        "Use an account you are authorized to access, keep your sign-in provider secure, and protect your devices. Do not share or sell account access, bypass device limits, or use another person’s credentials without permission. You are responsible for activity you authorize through your account.",
        "You can review and remove signed-in sessions in your account dashboard. Tell us promptly if you suspect unauthorized access. Security controls, session expiry, and rate limits may restrict access to protect you and the service.",
      ],
    },
    {
      id: "responsible-use",
      title: "Meetings and responsible use",
      paragraphs: [
        "Before capturing, transcribing, uploading, or sharing meeting content, inform participants and obtain all permissions required by law, your employer, and the meeting or platform rules. You must have the right to process personal, confidential, copyrighted, and other material you provide.",
        "Do not use Lennette for unlawful surveillance, impersonation, harassment, fraud, prohibited assistance in an assessment, or other deceptive or illegal activity. Do not upload malware, executables, unsupported archives, or content that violates another person’s rights. Do not attack the service, evade safeguards or billing limits, or interfere with other users.",
      ],
    },
    {
      id: "your-content",
      title: "Your content and AI output",
      paragraphs: [
        "You retain your rights in conversations, transcripts, prompts, documents, and other content you provide. You grant us permission to process, store, transmit, and display that content only as needed to provide the features you use, keep the service secure, and meet legal obligations. This does not transfer ownership of your content to us.",
        "AI answers and transcripts can be incomplete, inaccurate, or misleading. Review them before acting or sharing. They are not a substitute for qualified medical, legal, financial, or other professional advice, and the service is not intended for emergency or safety-critical decisions. Outputs may not be unique, and third-party rights can still apply.",
      ],
    },
    {
      id: "billing",
      title: "Subscriptions and payments",
      paragraphs: [
        "For paid services, the price, currency, billing interval, applicable taxes, and included usage shown at checkout govern your purchase. Recurring subscriptions renew as disclosed at checkout until canceled. Usage allowances apply to the stated billing period and do not become cash or carry over unless expressly offered.",
        "Dodo Payments handles managed-plan checkout and billing as the payment merchant of record. Its applicable buyer terms and privacy notice also apply to the payment transaction. Your account dashboard provides access to subscription and payment-method management.",
        "Cancel before the next renewal to avoid another recurring charge. The billing portal shows your cancellation and access dates. Deleting your Lennette account is different from simply canceling renewal: account deletion also removes hosted account data and initiates attachment erasure.",
      ],
    },
    {
      id: "refunds",
      title: "Refunds and billing issues",
      paragraphs: [
        "Paid subscriptions, renewals and plan upgrades are non-refundable, including unused subscription time, AI credits, transcription allowances and storage, except where required by applicable law. Cancel before your next renewal to prevent future charges.",
        "For a refund request, duplicate or incorrect charge, or problem accessing a purchase, contact harsh@itssingularity.com with the account email and payment reference. Do not send full payment-card details. We will review the request with the payment provider under the terms disclosed at checkout and applicable law.",
        "Canceling renewal does not by itself issue a refund for a previous charge. These terms do not limit any mandatory cancellation, refund, or consumer rights you have under applicable law.",
      ],
    },
    {
      id: "availability",
      title: "Availability and changes",
      paragraphs: [
        "We may update the application, change supported providers, maintain infrastructure, or apply reasonable usage and security controls. Networks, third-party providers, and maintenance can interrupt service. We do not promise uninterrupted operation or that every file, platform, or meeting setup will be supported.",
        "We will communicate material changes to paid-plan pricing or features before they apply, where required, so you can review your subscription. Keep your own copies of material you need; synchronization is not a substitute for an independent backup.",
      ],
    },
    {
      id: "ending-use",
      title: "Ending use and deleting your account",
      paragraphs: [
        "You may stop using Lennette at any time. You can request account deletion in the account dashboard. After billing cancellation succeeds, hosted account records and sessions are removed, and private attachment erasure is processed through a retryable cleanup queue. Storage-provider outages may delay physical erasure. Local copies and exports on your devices are not remotely erased.",
        "We may suspend or terminate access for a material breach of these terms, nonpayment, abuse, a security risk, or a legal requirement. Where appropriate, we will provide notice and an opportunity to resolve the issue. Records required for payment compliance, disputes, fraud prevention, or other legal obligations may be retained as explained in the Privacy Policy.",
      ],
    },
    {
      id: "rights-liability",
      title: "Software rights and responsibility",
      paragraphs: [
        "Lennette branding, website materials, and service components remain subject to their owners’ rights. Source code and third-party components are governed by their applicable license files; these terms do not remove rights those licenses grant you.",
        "To the extent permitted by law, the service is provided as available, without a guarantee of accuracy, suitability for a particular purpose, or uninterrupted availability. You are responsible for reviewing outputs and using the service appropriately. Nothing in these terms excludes liability or rights that cannot lawfully be excluded, including mandatory consumer protections.",
      ],
    },
    {
      id: "contact-changes",
      title: "Questions and updates to these terms",
      paragraphs: [
        "Contact harsh@itssingularity.com about these terms or a service issue. We will try to resolve concerns directly. You retain any right to contact a competent regulator or court under applicable law.",
        "We may revise these terms and update the date shown on this page. Material changes will be communicated through an appropriate channel where required. New terms apply prospectively and do not override mandatory rights or the terms of a completed purchase.",
      ],
    },
  ],
};

export const PRIVACY: LegalDocument = {
  kind: "privacy",
  title: "Privacy Policy",
  intro:
    "What Lennette processes, where your meeting content goes, and the choices you have along the way.",
  summary:
    "Local mode keeps your saved history on your device. Managed accounts sync selected content across devices. AI and transcription providers process the content needed for the features you use; Lennette does not retain raw meeting audio.",
  sections: [
    {
      id: "scope",
      title: "Who this policy covers",
      paragraphs: [
        "This policy describes how Lennette, operated under Singularity Works, handles personal information through its website, desktop app, and managed service. Contact harsh@itssingularity.com with privacy questions or requests. If an organization directs your use of Lennette, it may have additional responsibilities and policies for the meeting content it controls.",
      ],
    },
    {
      id: "information",
      title: "Information we process",
      paragraphs: [
        "Account information: when you sign in with Google or GitHub, we receive the provider’s account identifier, verified email address, and display name needed to create and secure your Lennette account. We do not receive your Google or GitHub password.",
        "Content: conversations, transcripts, AI responses, prompts, profiles, selected preferences, and supported attachments you choose to process or sync. Meeting content may include other participants’ personal information. Share only material you are entitled to process.",
        "Service information: device/session identifiers, device names, sign-in and expiry times, subscription status, payment references, billing periods, and aggregate AI, transcription, and storage usage. Our payment provider processes checkout and payment details; Lennette does not receive full payment-card numbers.",
        "Website and support information: information you send in a support message or waitlist signup, including your email and basic browser information where submitted. Hosting and security services may process IP addresses, request information, and technical identifiers. Operational logs, metrics, and traces help detect errors and abuse; they are not intended to record meeting bodies, uploaded files, or provider credentials.",
      ],
    },
    {
      id: "local-mode",
      title: "Local mode and your own provider keys",
      paragraphs: [
        "In local mode, saved chat and meeting history is held in the desktop’s encrypted local database, and provider credentials are kept in local credential storage. A Lennette account is not required for bring-your-own-key use. Your operating system, backups, exports, or files you save separately may create additional local copies.",
        "Using online AI or transcription still transmits relevant text, selected images or documents, or audio to your configured provider. Those providers handle that information under their own policies and your account settings. Your own provider API keys and device-specific settings are not included in managed history sync.",
      ],
    },
    {
      id: "managed-sync",
      title: "Managed storage and live synchronization",
      paragraphs: [
        "When you connect a managed account, you choose whether to import existing local history. New supported content then syncs automatically while the account is connected, including live meeting transcripts and AI responses for other signed-in devices. Signing out stops managed sync on that device; it does not delete data already hosted or remove local copies.",
        "Managed text and account metadata are stored in our hosted database. Supported attachments are stored in private Cloudflare R2 storage and accessed through authenticated requests. Uploaded files undergo type, structural, and malware checks; encrypted, active, malicious, and unsupported files may be rejected. Hosted information can be processed by our servers to provide these features, so managed sync is not end-to-end encrypted.",
      ],
    },
    {
      id: "audio-ai",
      title: "Audio and AI processing",
      paragraphs: [
        "Live transcription streams audio to the speech provider, directly in BYOK mode or through the managed backend. Lennette does not retain raw meeting audio as part of its managed storage. Text transcripts and derived content can be saved locally or synced as described above. A speech provider’s own processing and retention rules still apply.",
        "AI requests send the relevant conversation context, prompts, and selected attachment content to the configured AI service. Managed chat uses Merge Gateway and its downstream AI providers; managed speech uses Deepgram with its model-improvement opt-out setting. Provider selection may change as the service evolves.",
        "Lennette does not use your meeting content to train its own AI models. Third-party processing, retention, and training commitments depend on the provider, service agreement, and settings involved; BYOK users should review their chosen providers’ policies.",
      ],
    },
    {
      id: "purposes",
      title: "Why we use this information",
      paragraphs: [
        "We use information to authenticate users, deliver transcription and AI answers, synchronize content, manage subscriptions and usage limits, answer support requests, send requested service or waitlist messages, prevent abuse, investigate faults, and comply with applicable legal obligations.",
        "Where data-protection law requires a legal basis, processing may be necessary to provide the service you request, based on your consent, needed to meet legal obligations, or supported by legitimate interests such as service security and reliability where that basis is available. You can withdraw consent for optional processing or request help understanding the basis applicable to your information.",
      ],
    },
    {
      id: "service-providers",
      title: "Providers and sharing",
      paragraphs: [
        "We share information with service providers only as needed for their role: Google or GitHub for sign-in; Dodo Payments for checkout, subscriptions, taxes, and payment compliance; Cloudflare for website delivery, security, and R2 storage; Merge Gateway and downstream AI providers for managed AI; Deepgram for managed transcription; and Resend for waitlist and service emails. Our hosting, malware-scanning, and monitoring infrastructure also processes information needed to operate the service.",
        "These providers may process information in countries different from where you live. Their applicable privacy notices, contracts, and transfer arrangements govern their handling of information. Contact us for information about the providers and arrangements relevant to your use.",
        "We do not sell your personal information or share meeting content for third-party advertising. We may disclose information when legally required, to protect people or the service, or as part of a business transfer subject to appropriate notice and applicable protections.",
      ],
    },
    {
      id: "cookies",
      title: "Cookies and sessions",
      paragraphs: [
        "The account dashboard uses essential cookies for OAuth sign-in and browser sessions. Production session cookies are HttpOnly, Secure, and SameSite=Lax. Browser sessions expire after 30 minutes of inactivity or a maximum of 12 hours; desktop sessions follow separate access/refresh expiry and device-revocation controls.",
        "Live account updates do not extend the browser’s idle timeout. You can sign out, revoke devices, or clear cookies using your browser. Hosting and security providers may also use strictly necessary security mechanisms. The current site does not intentionally use advertising trackers or session replay. Blocking essential cookies can prevent sign-in and account features from working.",
      ],
    },
    {
      id: "retention-deletion",
      title: "Retention and deletion",
      paragraphs: [
        "Hosted content is generally retained while your account exists until you delete it. Subscription expiry does not automatically erase your saved content. Deleting a conversation or removing a file reference initiates cleanup when no other active content references that file. Unlinked uploads have a 24-hour grace period before reclamation; failed-upload orphan cleanup uses a separate delayed retry process.",
        "You can request account deletion in the dashboard. After billing cancellation succeeds, account records and sessions are deleted and private attachments enter a durable erasure queue. The files become inaccessible through the deleted account immediately, but storage outages or retries can delay physical erasure. Local copies, exports, and copies shared outside Lennette remain under their holders’ control.",
        "Operational/security logs, payment records, support correspondence, and restricted backup copies may remain for the period needed for their purpose, disaster recovery, disputes, fraud prevention, or legal obligations. Dodo Payments and other providers may retain records under their own legal duties. Deleting a Lennette account does not erase independent records those parties are legally required to keep.",
      ],
    },
    {
      id: "your-choices",
      title: "Your choices and requests",
      paragraphs: [
        "You can use local/BYOK mode, choose whether to import existing history, stop managed sync by signing out, review usage and devices, revoke sessions, manage your subscription, and delete your account. Use available app export features before deletion if you need a copy of your content.",
        "Depending on applicable law, you may request access, correction, deletion, portability, restriction, objection, or withdrawal of consent, and may have the right to complain to a data-protection authority. Email harsh@itssingularity.com to make a request, raise a privacy grievance, or ask to leave the waitlist. We may need to verify your identity and account authority before acting; do not send passwords or full payment-card details.",
      ],
    },
    {
      id: "safety-children",
      title: "Security and age restrictions",
      paragraphs: [
        "We use safeguards such as authenticated access, private attachment storage, encrypted transport, local database encryption, malware checks, rate limits, and session controls. No service can guarantee absolute security. Protect your devices and sign-in provider, and contact us promptly if you suspect misuse.",
        "Lennette is intended for adults aged 18 or older and is not directed to children. If you believe a child has provided personal information, contact us so we can investigate and take appropriate action.",
      ],
    },
    {
      id: "policy-updates",
      title: "Updates and contact",
      paragraphs: [
        "We will update the date on this page when this policy changes. Where required, we will provide notice of material changes and obtain consent before introducing processing that requires it. For privacy requests or concerns, contact Singularity Works at harsh@itssingularity.com.",
      ],
    },
  ],
};
