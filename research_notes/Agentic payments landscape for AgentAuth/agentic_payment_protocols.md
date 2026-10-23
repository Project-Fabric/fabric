# Agentic payment protocols and network programs (status as of 21 October 2026)

Method note: about 20 searches and fetches, run on 2026-10-21. Primary sources fetched directly: the AP2 GitHub repo, ap2-protocol.org (home page and Agent Authorization page), the FIDO Alliance AP2 donation page, Visa's press release of 2026-10-20, the ACP GitHub repo, and docs.stripe.com (/agents and /mcp). Mastercard.com could not be fetched because the domain was blocked for the fetch tool. Mastercard facts therefore come from search snippets and trade press, and are flagged as such. Some snippets came from secondary or aggregator blogs (eco.com, stellagent.ai, openhermit and similar). These are marked "secondary" and should be read as lower-confidence.

## Google AP2 (Agent Payments Protocol)

### Takeaway
AP2 is a published, open (Apache-2.0), pre-1.0 specification (v0.2), donated to the FIDO Alliance on 2026-04-28/29. Its core is signed "Mandates": SD-JWT verifiable credentials that the user approves on a "Trusted Surface", either closed (the human is present and signs the exact checkout) or open (pre-authorised constraints, bound to an agent's key, which the agent later closes). It gives a non-repudiable audit trail "aiding" disputes but **assigns no liability**. It is framed entirely around a user buying from a merchant (consumer checkout). It has no constructs for corporate approval chains, payouts or vendor payments.

### Cited Findings
- **Repo status:** the repo describes itself as "code samples and demos of the Agent Payments Protocol". It is Apache-2.0, has about 3.2k stars and 54 commits, and the Releases section is empty. A PyPI package is "planned"; for now installation is from Git. Samples use ADK and Gemini, but "the protocol doesn't require the use of either" — [GitHub google-agentic-commerce/AP2](https://github.com/google-agentic-commerce/AP2) (fetched 2026-10-21)
- **FIDO donation:** dated **2026-04-29** by FIDO. Google "also published AP2 v0.2 on GitHub". The donation is "intended to ensure AP2 remains platform-agnostic and open to broader industry participation". v0.2 adds "Human Not Present" payments, in which agents "execute purchases independently, based on instructions pre-authorised by the user" — [FIDO Alliance](https://fidoalliance.org/google-donates-agent-payments-protocol-to-fido-alliance/)
- **FIDO working groups:** on 2026-04-28 FIDO announced an **Agentic Authentication Technical Working Group** (how users "securely and privately delegate actions to AI agents" with phishing-resistant authentication) and a separate **Payments Technical Working Group** chaired by Mastercard and Visa members. Initial contributions are Google's AP2 and Mastercard's Verifiable Intent — [FIDO press release via search](https://www.fidoalliance.org/fido-alliance-to-develop-standards-for-trusted-ai-agent-interactions/); [PPC Land](https://ppc.land/fido-alliance-forms-working-groups-to-lock-down-ai-agent-payments/)
- **Mandate model in the current (v0.2) docs:** two mandate types, each with an Open and a Closed form.
  - **Checkout Mandate:** "Captures the reference to the specific items and purchase details negotiated between the agent and the merchant, and is shared with the merchant." Open = the user's constraints and goals before the cart is final. Closed = authorisation for a finalised checkout.
  - **Payment Mandate:** "Authorizes a payment against a specific payment instrument, and is shared with the Credential Provider, Networks and the Merchant Payment Processor." Open = budget and allowed instruments for autonomous execution. Closed = a specific amount tied to a finalised checkout.
  - Source: [ap2-protocol.org](https://ap2-protocol.org/)
- **Terminology change (needs confirmation):** the v0.1 launch (Google Cloud, Sept 2025) used the terms **Intent Mandate / Cart Mandate / Payment Mandate**. The v0.2 site uses **Checkout and Payment mandates with Open/Closed states**, and an "Intent" mandate no longer appears as a named type. Open Checkout appears to take the Intent Mandate's role. I did not fetch the Sept 2025 blog in this session; it is linked at [Google Cloud blog](https://cloud.google.com/blog/products/ai-machine-learning/announcing-agents-to-payments-ap2-protocol). Inference from [ap2-protocol.org](https://ap2-protocol.org/).
- **Credential format:**
  - Mandates are **SD-JWT VCs** (`dc+sd-jwt`), with examples signed ES256.
  - Verifiable digital credentials are presented via **OpenID4VP**. ISO mDoc (18013-5) is noted as a possible alternative.
  - The agent proves possession with a **Key Binding JWT** using the key in the open mandate's `cnf` claim (RFC 7800).
  - Source: [AP2 Agent Authorization](https://ap2-protocol.org/ap2/agent_authorization/)
- **How consent and user presence are captured:**
  - The user approves Mandate Content on a "Trusted Surface". Two trust models are defined: "User Credential" and "Trusted Agent Provider".
  - In the Trusted Agent Provider model, the provider's Trusted Surface "displays the Mandate Content to the user and obtains any necessary user authorization and consent". The Agent must not be able to access the provider's signing key.
  - The page notes that some models use "a directly trusted user key... such as a passkey or hardware-attested key".
  - The Digital Credentials API is recommended. No specific biometric or PIN method is mandated.
  - Source: [AP2 Agent Authorization](https://ap2-protocol.org/ap2/agent_authorization/)
- **Human-present vs not-present:**
  - Human present: "the User directly signs closed Mandate Content".
  - Not present: "the User signs open Mandate Content", and then the Agent signs the closed content. Open mandates "are necessary to allow the Agent to perform autonomous actions".
  - Source: [AP2 Agent Authorization](https://ap2-protocol.org/ap2/agent_authorization/)
- **Agent identity:** an open mandate is "bound to a particular Agent who is allowed to use the Mandate". Identity is the `cnf` key, not a separate agent-identity credential. Verifiers "MUST return a signed Mandate Receipt" — [AP2 Agent Authorization](https://ap2-protocol.org/ap2/agent_authorization/)
- **Roles:** the spec describes a "role-based architecture". Parties named include the agent, merchant, Credential Provider, Networks, Merchant Payment Processor, issuer and PSP — [ap2-protocol.org](https://ap2-protocol.org/)
- **Liability and disputes:** "AP2 provides a non-repudiable, cryptographic audit trail for every transaction, aiding in dispute resolution." The site raises "who is accountable" as a question but **does not assign liability**. The Agent Authorization page has no liability or dispute language — [ap2-protocol.org](https://ap2-protocol.org/); [Agent Authorization](https://ap2-protocol.org/ap2/agent_authorization/)
- **A2A, MCP, UCP and x402:**
  - AP2 is "available as an extension for the open-source Agent2Agent (A2A) protocol and Universal Commerce Protocol" and is described as "a non-proprietary, open extension for A2A and MCP".
  - x402 appears in the samples: "Human Present x402" and "Human Not Present x402".
  - Source: [ap2-protocol.org](https://ap2-protocol.org/)
- **Adoption (secondary sources, low confidence):**
  - Public deployments by April 2026 reportedly included a PayPal wallet integration and a Mastercard Agent Pay pilot — [OpenHermit (secondary)](https://www.openhermit.com/blog/agent-payment-protocols-compared)
  - PayPal is described as part of a "60-collaborator AP2" group — [eco.com (secondary)](https://eco.com/support/en/articles/14730447-paypal-agentic-commerce-explained)
  - Claims of a Google I/O (May 2026) consumer launch were not corroborated by any primary source.

### Inferences
- AP2's Open Payment Mandate (budget, allowed instruments, bound to an agent key) is the closest published analogue to a pre-granted "mandate" for an internal agent. However, the spec assumes the signer is the purchasing user and the counterparty is a merchant. It has no multi-approver chains, no role-based approval, and no constructs for refunds, payouts or vendor payments.
- The concrete crypto stack (SD-JWT VC, KB-JWT, OpenID4VP) is a reasonable interop target if an internal-authority system wants to emit externally verifiable mandates.

### Gaps
- I could not fetch the full v0.2 specification text (/specification/ returned 404, and the raw specification.md path returned 404). Exact field lists for the mandates are unverified.
- I found no primary source for production volumes or a named list of live merchants.

## Visa Intelligent Commerce (VIC) and Trusted Agent Protocol (TAP)

### Takeaway
VIC (announced April 2025) is Visa's umbrella programme of agentic tokens, agent registration and authentication. TAP (October 2025) is an open, HTTP-Message-Signatures-based way for merchants to verify registered agents. The most relevant recent item is a **live B2B pilot announced 2026-10-20**, in which a BILL Payment Agent pays approved supplier invoices via Billtrust, registered through TAP and the Visa "Agentic Directory". The agent only executes payments the buyer has already approved, and the approval workflow itself stays in BILL.

### Cited Findings
- **BILL/Billtrust pilot (2026-10-20):** the "first live pilot to enable trusted AI agents to initiate and complete approved buyer invoice payments".
  - The BILL Payment Agent is "registered using Visa's Trusted Agent Protocol and Agentic Directory".
  - Billtrust uses TAP to "recognize, authenticate, and grant access to the authorized agent", replacing CAPTCHA and portal logins.
  - Payments use tokenized credentials. The agent "only executes eligible payments that have already been approved by the buyer", and each payment is tied to a specific invoice.
  - Status: **pilot**, "currently live and processing end-to-end transactions with participating buyers". There is no GA date.
  - Source: [Visa press release](https://usa.visa.com/about-visa/newsroom/press-releases.releaseId.22856.html)
- **TAP design (secondary):** TAP launched on 2025-10-14 to distinguish legitimate agents from bots. It is built on HTTP Message Signatures (RFC 9421) with published keys, was developed with Cloudflare, and had feedback from Adyen, Checkout.com, Microsoft, Shopify, Stripe, Worldpay and others. One analysis notes that the TAP GitHub repo has had no pushes since late October 2025 — [stellagent (secondary)](https://stellagent.ai/insights/visa-intelligent-commerce-vic-explained); [eco.com (secondary)](https://eco.com/support/en/articles/15192003-mastercard-agent-pay-vs-visa-trusted-agent-2026-compared)
- **Single integration (trade press):** Visa packages VIC as a single merchant integration supporting four agent protocols: TAP, Machine Payments Protocol, ACP and UCP — [TechInformed](https://techinformed.com/visa-opens-one-integration-for-ai-agent-payments/)
- **Ramp partnership (secondary, unverified):** a partnership announced 2026-03-31 reportedly extends VIC and TAP to corporate bill pay — [search snippet via beri.net (secondary)](https://www.beri.net/article/visa-intelligent-commerce-connect-ai-agent-payments-2026)
- **European live transactions (unverified):** a claim of live agentic transactions in Europe as of 2026-07-02 with 30+ issuers comes from a third-party aggregator only — [commonworld.one (unverified)](https://commonworld.one/atlas/platforms/visa-intelligent-commerce/)
- **Asia Pacific pilot:** Visa is building AI commerce infrastructure for an Asia Pacific pilot in 2026 — [AI News](https://www.artificialintelligence-news.com/news/visa-ai-commerce-intelligent-commerce-2026/)

### Inferences
- Visa's B2B pilot validates the "company's agent pays company's bills" use case. Visa's layer only covers agent identity and tokens at the acceptance edge. Approval, delegated authority and audit live in the AP platform (BILL), which is a gap that independent governance tooling could fill.

### Gaps
- I found no primary Visa doc on the VIC passkey or "Visa Payment Passkey" flow for agent payments, or on liability allocation for agent-initiated transactions.
- The VIC developer docs on developer.visa.com were not fetched.

## Mastercard Agent Pay

### Takeaway
Agent Pay was announced on 2025-04-29 and is built on **Agentic Tokens**: network tokens bound to a specific agent and consent scope. It has since grown to include an Acceptance Framework (Fiserv integration, Jan 2026), **Verifiable Intent** (an open-source spec released 2026-03-05 with Google, contributed to FIDO), and "Agent Pay for Machines" (June 2026). Live transactions have been reported in Hong Kong (HSBC, using Payment Passkeys) and elsewhere. API specs for intent were said to be "coming later".

### Cited Findings
- **Agentic Tokens (secondary):** network tokens with extra fields binding them to one AI agent and one consent policy. Scope is set at issuance and enforced by the network — [stellagent (secondary)](https://stellagent.ai/insights/mastercard-agent-pay-agentic-tokens); [eco.com (secondary)](https://eco.com/support/en/articles/15192001-what-is-mastercard-agent-pay-ai-agent-commerce-protocol-in-2026)
- **Hong Kong live transaction:** the first live agentic transaction in Hong Kong was with HSBC. Each transaction uses an Agentic Token "uniquely issued to each agent", and consent and confirmation were secured via **Payment Passkeys**. This comes from the press-release title and search snippet; the page itself could not be fetched — [Mastercard HK newsroom](https://www.mastercard.com/news/ap/en-hk/newsroom/press-releases/en-hk/2026/mastercard-completes-its-first-live-agentic-transaction-in-hong-kong/)
- **Verifiable Intent (2026-03-05):** an open-source spec and reference implementation on GitHub and at verifiableintent.dev. It ties consumer identity, agent instructions and transaction outcome into one tamper-resistant record. Partners are Google, Fiserv and IBM. API specs for Agent Pay "will come later" on Mastercard Developers — [Mastercard story](https://www.mastercard.com/sg/en/news-and-trends/stories/2026/verifiable-intent.html); [PYMNTS](https://www.pymnts.com/mastercard/2026/mastercard-unveils-open-standard-to-verify-ai-agent-transactions/)
- **Verifiable Intent internals (secondary, unverified):** reportedly an SD-JWT delegation chain with eight constraint types, with mappings to AP2, UCP and ACP — [stellagent (secondary)](https://stellagent.ai/insights/mastercard-verifiable-intent)
- **Agent Pay for Machines (2026-06-10):** reportedly live with 30+ partners across cards, stablecoins and Layer-1s — [Genfinity (crypto trade press)](https://genfinity.io/2026/06/10/mastercard-agent-pay-for-machines-launch/)
- **Maturity:** one standards tracker still listed Agent Pay as "announced" as of June 2026 — [agenticcommerceprotocol.info (secondary)](https://agenticcommerceprotocol.info/standards/agent-pay)
- **Developer docs:** Mastercard publishes developer material on "The Rails for Agentic Commerce" — [Mastercard Developers](https://developer.mastercard.com/merchant-cloud/documentation/tutorials-and-guides/ai-revolution/14/)

### Inferences
- Mastercard's documented scope is consumer purchases (cardholder, issuer, agent, merchant). I found no Mastercard material on corporate internal agents or approval chains.

### Gaps
- The Mastercard primary pages were blocked. The agent registration and verification process (for example the "Know Your Agent" onboarding), the liability rules, and the September–October 2026 status are unverified.

## OpenAI + Stripe Agentic Commerce Protocol (ACP), Instant Checkout, Shared Payment Tokens

### Takeaway
ACP is an Apache-2.0 open spec maintained by OpenAI and Stripe, **in beta**. Its latest stable version is **2026-04-17** (cart, feed, orders, authentication, MCP), and it contains a Checkout API and a Delegate Payment spec. It targets consumer checkout. Instant Checkout in ChatGPT launched on 2025-09-29, but OpenAI reportedly scaled it back in **March 2026** in favour of product discovery plus merchant-owned checkout, while the protocol continues.

### Cited Findings
- **Repo:** "The specification is maintained by OpenAI and Stripe and is currently in beta." The governance model is "Founding Maintainers", with a stated aim of neutral foundation stewardship. The license is Apache 2.0. The latest stable version is 2026-04-17 ("Cart, feed, orders, authentication, and MCP"). The repo contains RFCs on payment handlers, capability negotiation and discounts. **No B2B mention** — [GitHub ACP](https://github.com/agentic-commerce-protocol/agentic-commerce-protocol)
- **Launch:** the launch (2025-09-29) covered U.S. Etsy sellers with single-item purchases, with Shopify to follow. Non-Stripe merchants could use the SPT API or the Delegated Payments Spec — [OpenAI](https://openai.com/index/buy-it-in-chatgpt/); [Stripe newsroom](https://stripe.com/newsroom/news/stripe-openai-instant-checkout); [TechCrunch](https://techcrunch.com/2025/09/29/openai-takes-on-google-amazon-with-new-agentic-shopping-system)
- **Shared Payment Tokens:** SPTs let an app start a payment without exposing the buyer's credentials, scoped to one merchant and one cart total — [Stripe newsroom](https://stripe.com/newsroom/news/stripe-openai-instant-checkout). A $0.15 per-token fee is reported only by secondary sources and is unverified.
- **March 2026 change:** OpenAI said the first Instant Checkout "did not offer the flexibility" it wanted and shifted to product discovery with merchants' own checkout. Reports attribute this to weak uptake — [Storyboard18](https://storyboard18.com/digital/openai-reduces-chatgpt-shopping-focus-shifts-to-product-discovery-93192.html); [eesel (secondary)](https://www.eesel.ai/blog/chatgpt-instant-checkout-2026). I did not find OpenAI's own primary post.

### Inferences
- ACP and SPTs only cover a buyer and agent paying a merchant. They do not address a company governing outbound money movement.

### Gaps
- The Delegate Payment spec's consent and authentication fields were not read in detail.

## Coinbase x402

### Takeaway
x402 revives HTTP 402 for machine-to-machine and pay-per-request payments, typically in stablecoins. Governance moved to an x402 Foundation under the **Linux Foundation on 2026-04-02**, with premier members including AWS, American Express, Cloudflare, Coinbase, Google, Mastercard, Shopify, Solana Foundation, Stripe and Visa. AP2 ships x402 sample flows.

### Cited Findings
- **Origin:** the x402 Foundation was formed by Coinbase and Cloudflare in Sept 2025. Coinbase developed the transaction flow, and Cloudflare built Agents SDK and MCP tooling — [The Block](https://www.theblock.co/post/372064/cloudflare-coinbase-launch-x402-foundation); [Blockworks](https://blockworks.com/news/coinbase-cloudflare-x402)
- **Linux Foundation launch:** the foundation launched under the Linux Foundation on 2026-04-02, with 40 organisations at launch and the premier members listed above — [Coinbase CDP docs](https://docs.cdp.coinbase.com/x402/support/x402-foundation). This conflicts with a report of "23 founding companies" — [CriptoNoticias](https://www.criptonoticias.com/tecnologia/google-amazon-mastercard-suman-coinbase-pagos-ia/)
- **V2 headers (single secondary source):** V2 defines `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE` and `PAYMENT-RESPONSE` headers — [wavect (secondary)](https://wavect.io/es/blog/x402-payments-comparison-2026.md)
- **AP2 samples:** AP2 includes human-present and human-not-present x402 samples — [ap2-protocol.org](https://ap2-protocol.org/)

### Inferences
- x402 is a settlement and payment-signalling mechanism (an agent paying for an API or resource). It carries no consent, approval or delegated-authority semantics of its own; those must come from AP2-style mandates or from the payer's wallet policy.

### Gaps
- The x402.org spec text was not fetched. Dispute and refund semantics are not documented in the sources I found.

## PayPal, Stripe agent toolkit/MCP, Adyen

### Takeaway
Stripe's MCP server is the most directly relevant to "company agents moving the company's money". It exposes write access, including refunds, and public-preview **Treasury `send_money` and `transfer_money`** tools. It requires **human confirmation for certain writes such as refunds and outbound payments**, and is moving to "agent API keys" (full-access and untagged restricted keys stop working from Dec 2026). PayPal's Agent Toolkit exposes invoicing, disputes and payments. Adyen launched "Adyen Agentic" (June 2026) on the merchant side.

### Cited Findings
- **Stripe MCP (docs fetched 2026-10-21):**
  - Tools include generic `stripe_api_read` and `stripe_api_write` (any POST, PATCH, PUT or DELETE). Supported methods include Create refund, invoices, payment links, disputes (update), transfers and payouts (read).
  - Treasury tools (public preview) are `send_money`, `setup_recipient`, `transfer_money` and `convert_currency`.
  - "Stripe requires human confirmation before it takes certain `stripe_api_write` actions, such as refunds and outbound payments." The user clicks a URL and approves, the agent receives an approval token, and approvals expire after 24 h.
  - Stripe advises: "Turn on your MCP client's tool-approval setting".
  - Authentication is OAuth or **agent API keys** "with only the permissions the client needs". From Dec 2026, MCP "will stop accepting full-access secret keys and restricted API keys without the Agent tag" (migrate by 2026-11-30).
  - Admins can disable MCP access team-wide and revoke OAuth sessions. Tool-call logs are visible in Workbench.
  - Source: [Stripe MCP docs](https://docs.stripe.com/mcp)
- **Stripe agents hub:** offers an agent plugin and skills, "Accept payments from agents" (machine payments), the Stripe Directory, and an agentic commerce overview — [Stripe Agents](https://docs.stripe.com/agents)
- **PayPal:** the developer hub offers an agent toolkit, an LLM connection guide and an agentic commerce network — [PayPal developer](https://developer.paypal.com/ai-tools/get-started). Per secondary sources (unverified), the toolkit covers payments, invoicing, disputes, shipment tracking, subscriptions and reporting across the OpenAI Agents SDK, LangChain, Bedrock, CrewAI, Vercel and MCP. A remote MCP server reportedly launched in April 2025, and PayPal is reported to have launched "Agent Ready" — [stellagent (secondary)](https://stellagent.ai/insights/paypal-agentic-commerce-agent-ready); [RetailBoss](https://retailboss.co/paypal-launches-agent-ready-to-enable-ai-powered-agentic-payments)
- **Adyen:**
  - The docs (modified March 2026) describe a product feed, Agentic Checkout API, Delegated Payment Spec and Agentic Commerce Suite API — [Adyen docs](https://docs.adyen.com/online-payments/agentic-commerce.md)
  - "Adyen Agentic" (2026-06-16) has Feed, Cart and Payments layers, with participants including Amex, Mastercard, Salesforce and Visa — per search snippet of the press release, not fetched directly.

### Inferences
- Stripe's built-in human confirmation is a single-user, per-action approval tied to the Stripe account. It does not express policies (thresholds, approver roles, multi-step chains) or produce exportable, vendor-neutral evidence. This is a gap relative to finance-ops governance.

### Gaps
- The PayPal and Adyen primary docs were not fetched in detail.

## Standards bodies (FIDO, EMVCo, W3C)

### Takeaway
FIDO now hosts AP2 and Verifiable Intent work in two technical working groups (formed April 2026). EMVCo has a draft "EMV Agentic Payments – Framework for Specifications" centred on "Intent Services"; its comment window closed on 2026-09-30. Both focus on consumer card and agent commerce.

### Cited Findings
- **FIDO:** the Agentic Authentication TWG and the Payments TWG (chaired by Mastercard and Visa members) were formed on 2026-04-28 — [FIDO](https://www.fidoalliance.org/fido-alliance-to-develop-standards-for-trusted-ai-agent-interactions/); [PPC Land](https://ppc.land/fido-alliance-forms-working-groups-to-lock-down-ai-agent-payments/)
- **EMVCo:**
  - The Agentic Payments Task Force draft framework centres on "Intent Services", which allow participants to "register, reference, retrieve and manage a consumer's authorised purchasing intent".
  - Submissions closed on 2026-09-30.
  - EMVCo coordinates with FIDO, the OpenID Foundation and the OpenWallet Foundation.
  - Possible future items are "Know Your Agent" and Agentic Transaction Indicators.
  - Sources: [Digital Transactions](https://www.digitaltransactions.net/emvcos-effort-to-reinforce-trust-in-card-based-agentic-commerce/); [EMVCo](https://www.emvco.com/knowledge-hub/agentic-payments-and-digital-payment-credentials-highlights-from-the-emvco-board-of-advisors-meeting/)

### Gaps
- I found no evidence of W3C agent-payment authorisation work in this pass; the W3C VC data model underlies some of these efforts only indirectly. The FIDO working-group deliverables and their status after April 2026 are unknown.

## Cross-cutting: consumer checkout vs a company governing its own agents' money

### Takeaway
Every published specification here (AP2, ACP, x402, TAP, Verifiable Intent, the EMVCo draft) models **a consumer or agent paying a merchant or resource**. None specifies internal approval chains, segregation of duties, or delegated corporate authority over refunds, payouts or vendor payments. The B2B signals are product pilots (Visa + BILL/Billtrust, reported Ramp + Visa) and per-action confirmation in Stripe MCP, where approval logic stays proprietary.

### Cited Findings
- **ACP:** no B2B mention — [ACP repo](https://github.com/agentic-commerce-protocol/agentic-commerce-protocol)
- **AP2:** roles are user, agent, merchant, credential provider and processor, with no liability assignment — [ap2-protocol.org](https://ap2-protocol.org/)
- **EMVCo:** intent framed as "a consumer's authorised purchasing intent" — [Digital Transactions](https://www.digitaltransactions.net/emvcos-effort-to-reinforce-trust-in-card-based-agentic-commerce/)
- **Visa B2B pilot:** the agent executes buyer-approved invoices, but the approval is upstream in BILL — [Visa](https://usa.visa.com/about-visa/newsroom/press-releases.releaseId.22856.html)
- **Stripe MCP:** human confirmation for refunds and outbound payments — [Stripe MCP](https://docs.stripe.com/mcp)

### Inferences
- There is white space for a vendor-neutral layer that records corporate mandates, approval chains and evidence for internal agents, and that could emit AP2- or Verifiable-Intent-compatible SD-JWT mandates outward.
- Liability allocation for agent-initiated payments is unresolved in every published spec reviewed. Any evidence product should avoid claiming it settles disputes.

### Gaps
- Network rule changes (Visa and Mastercard operating regulations) on chargeback liability for agent transactions were not found in public sources.
