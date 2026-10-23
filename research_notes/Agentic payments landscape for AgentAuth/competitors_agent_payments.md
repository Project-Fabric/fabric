# Competitor map: agent payments, spending controls, wallets and governance (as of 2026-10-21)

Method note: 15 web searches and fetches done on 2026-10-21. Many funding facts come from aggregators (Tracxn, Rye's blog, Proxy's blog) rather than primary press releases; these are flagged. "Claim" means the figure is self-reported by the company or comes from a single unverified source. Dates are the publication or event dates of the sources.

## What each company does, who it serves, how it works, and how it overlaps with AgentAuth

### Takeaway
The field splits into (a) consumer and merchant checkout rails (Skyfire, Nekuda, Prava, Crossmint, Rye, Basis Theory's consortium, Visa/Mastercard programs), (b) agent wallets and bank-like accounts (Natural, Catena Labs, Crossmint, Payman), (c) developer spend and metering layers (Sapiom), (d) corporate cards for agents (Ramp Agent Cards, Lithic, Marqeta, Proxy), and (e) agent governance and audit gateways (Proofpane, Runlayer and the AI agent security cohort). The closest overlaps with AgentAuth (mandates, approvals, signed evidence, a gateway in front of tools) are Catena Labs (human-set limits, approved recipients, audit trails), Nekuda (agentic mandates), Sapiom (spend caps and rules on agent purchases), Ramp Agent Cards (limits and approval chains), and Proofpane (signed, offline-verifiable audit evidence, though not money-specific).

### Cited Findings

**Natural** (agent payments orchestration; enterprise and developer)
- Founded 2025. Describes itself as an "agent orchestration layer" that lets AI agents move and store funds, pay vendors, collect funds and transact with humans and other agents. It also aims to rethink how disputes are handled. — [TechCrunch, 2026-07-20](https://techcrunch.com/2026/07/20/natural-raises-30m-to-reinvent-payments-for-ai-agents-and-take-on-stripe/)
- Rails: plans to support USD-backed stablecoins and is building traditional bank payments. Was in beta until the raise. — [TechCrunch, 2026-07-20](https://techcrunch.com/2026/07/20/natural-raises-30m-to-reinvent-payments-for-ai-agents-and-take-on-stripe/)
- Funding: $30M Series A led by Forerunner (Kirsten Green), July 2026; total above $40M. — [PR Newswire, Jul 2026](https://www.prnewswire.com/news-releases/natural-raises-30m-series-a-to-build-payments-infrastructure-for-ai-agents-302829855.html); [TechCrunch](https://techcrunch.com/2026/07/20/natural-raises-30m-to-reinvent-payments-for-ai-agents-and-take-on-stripe/)
- Team: CEO Kahlil Lalji and co-founder Eric Wang, both from Ivella (YC; sold to Earnin in 2023); Walt Leung (ex-Nextdoor). Staff include people formerly at Stripe, Ramp and Square. It names Stripe as its main competitor. No customers or volumes disclosed. — [TechCrunch](https://techcrunch.com/2026/07/20/natural-raises-30m-to-reinvent-payments-for-ai-agents-and-take-on-stripe/)

**Catena Labs** (regulated "AI-native bank" and governance layer; businesses and individuals)
- Building banking infrastructure for AI agents. It describes itself as a governance layer where humans set spending limits, approved recipients and holding caps, and audit trails are established before agents execute payments. — [The Block, 2026-05-20](https://www.theblock.co/post/402029/catena-labs-lands-30-million-series-a-files-for-national-trust-bank-charter-to-underpin-agentic-finance)
- Its application for an OCC National Trust Bank charter has been accepted for filing; the charter is not approved. If granted, Catena could hold customer funds as a regulated fiduciary for agents. — [The Block, 2026-05-20](https://www.theblock.co/post/402029/catena-labs-lands-30-million-series-a-files-for-national-trust-bank-charter-to-underpin-agentic-finance)
- Funding: $18M seed in 2025, led by a16z crypto; $30M Series A on 2026-05-20, co-led by a16z crypto and Acrew Capital. Co-founder Sean Neville is a Circle co-founder. No customers named. — [The Block](https://www.theblock.co/post/402029/catena-labs-lands-30-million-series-a-files-for-national-trust-bank-charter-to-underpin-agentic-finance). CB Insights shows $18M total, which is stale. — [CB Insights](https://www.cbinsights.com/company/catena-labs)
- Open-source "Agent Commerce Kit" covering agent identity and payments (aggregator source). — [Neuronfeed](https://neuronfeed.com/startups/catena-labs)

**Skyfire** (agent identity and payments, "Know Your Agent"; merchants and agent developers)
- KYAPay protocol: agent identity through signed JWTs, plus Agent Checkout and SMS approval for high-value transactions. — [Proxy blog, 2026-01-14](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026); [Rye blog, 2026](https://rye.com/blog/agentic-commerce-startups)
- December 2025: demonstrated an agent buying headphones on Bose.com using KYAPay with Visa Intelligent Commerce and Trusted Agent Protocol. This was a prototype. — [BusinessWire, 2025-12-18](https://www.businesswire.com/news/home/20251218520399/en/Skyfire-Demonstrates-Secure-Agentic-Commerce-Purchase-Using-the-KYAPay-Protocol-and-Visa-Intelligent-Commerce)
- Enterprise partnership with F5 (integration available by April 2026). — [F5 press release](https://www.f5.com/company/news/press-releases/f5-skyfire-secure-agentic-commerce)
- Funding: $8.5M announced August 2024 (Neuberger Berman, Brevan Howard Digital, Intersection Growth Partners); about $9.5M total, with a16z CSX and Coinbase Ventures also named. No Series A found. — [Finextra, 2024](https://www.finextra.com/newsarticle/44621/skyfire-raises-85m-to-bring-autonomous-payments-to-ai-agents); [Tracxn](https://tracxn.com/d/companies/skyfire/__-gSNwLdAbLR2EH3jQO5ja24BZ2dqjmKWC_BS3-4pf1s). TechCrunch calls it DCVC-backed and stablecoin-based. — [TechCrunch, 2026-07-20](https://techcrunch.com/2026/07/20/natural-raises-30m-to-reinvent-payments-for-ai-agents-and-take-on-stripe/)
- Claim: "processes thousands of transactions daily" (self-reported, via Rye's blog). — [Rye blog](https://rye.com/blog/agentic-commerce-startups)
- Conflict: one developer-tools blog says Skyfire shut down in 2026. — [Arch Tools](https://archtools.dev/blog-skyfire-migration.html). That is unverified, and TechCrunch still referred to Skyfire as a competitor in July 2026.

**Nekuda** (agent checkout SDK with "agentic mandates"; developers and AI shopping apps)
- Secure Agent Wallet and "Agentic Mandates" that capture user intent: what an agent may buy, its spending limits and when approval is required. Card-credential based. Launch partner for Visa Intelligent Commerce. — [Proxy blog, 2026-01-14](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026); [Rye blog](https://rye.com/blog/agentic-commerce-startups)
- Funding: $5M seed on 2025-05-14, led by Madrona with Amex Ventures and Visa Ventures; angels include Paul Klein (Browserbase). No later round found as of August 2026. — [VCA Online, 2025-05-14](https://www.vcaonline.com/news/2025051415/nekuda-raises-5m-led-by-madrona-together-with-amex-ventures-and-visa-ventures-to-power-agentic-payments/); [startupim, updated 2026-08-18](https://startupim.com/company/nekuda-ai)
- Partners: Henry Labs (its one-click checkout runs on Nekuda's SDK) and a Visa pilot using Rye checkout. — [Rye blog](https://rye.com/blog/agentic-commerce-startups)

**Payman** (AI-to-human payments; later banking automation)
- Original product: a marketplace where agents pay humans for tasks, with spending limits and manual sign-off above set thresholds. USD custody at Fifth Third Bank. — [Proxy blog, 2026-01-14](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)
- Funding: $3M pre-seed in 2024 (Visa and Coinbase Ventures reported). — [ExamineCrypto](https://www.examinecrypto.com/post/payman-ai-an-ai-to-human-payments-platform-secures-3m-in-pre-seed-funding-round). Tracxn reports $13.8M over 3 rounds. — [Tracxn](https://tracxn.com/d/companies/payman-ai/__NSTYOZtZdNiGZxC0Vkul0dzUfj3ZUgPqDRNO08pHBUE/funding-and-investors). Fundz reports $9.57M on 2024-11-05. — [Fundz](https://app.fundz.net/fundings/payman-ai-funding-round-87d84f). These amounts conflict.
- Tracxn now describes it as an AI platform for banking transaction automation, which suggests a pivot toward banks. Claim: $770K 2025 revenue with a 7-person team (third-party, unverified). — [Tracxn](https://tracxn.com/d/companies/payman-ai/__NSTYOZtZdNiGZxC0Vkul0dzUfj3ZUgPqDRNO08pHBUE); [GetLatka](https://getlatka.com/companies/paymanai.com)

**Crossmint** (wallets, stablecoin rails and agent cards; developers and enterprises)
- Agent wallets: user-owned smart wallets with a delegated server signer for USDC spending, running on Base. Supports x402. — [Stablecoin Insider, Sep 2026](https://stablecoininsider.org/how-to-use-crossmint-agent-wallets-for-usdc/); [Crossmint](https://www.crossmint.com/solutions/agentic-payments)
- June 2026: launched an agentic cards API on Visa Intelligent Commerce and Basis Theory for eligible US Visa cardholders. — [PR Newswire, 2026](https://www.prnewswire.com/news-releases/crossmint-launches-agentic-cards-api-using-visa-intelligent-commerce-and-basis-theory-302786624.html)
- Launch partner of the Solana Developer Platform (March 2026) alongside Mastercard, Worldpay and Western Union. — [Solana](https://solana.com/news/case-study-crossmint)
- Funding: $23.6M led by Ribbit Capital (March 2025). No 2026 round found. — [Crossmint](https://www.crossmint.com/announcement/crossmint-raises-23-6m-led-by-ribbit-capital)

**Sapiom** (spend, routing and billing layer for agents buying software, APIs and compute; developers)
- One account for agents: the agent calls paid services (LLMs, search, compute, email), Sapiom pays the vendors, and the builder gets one metered bill with spend caps. It also controls which models, tools and purchases an agent may use, and has a cost-cutting model router. — [X post summarizing the round](https://x.com/jbahrdestefano/status/2085077703867474187?lang=en); [Quasa](https://quasa.io/media/sapiom-raises-35m-to-give-ai-agents-budgets-rules-and-buying-power)
- Funding: $15M seed led by Accel (February 2026); $35M Series A led by Dragonfly (2026-08-05), for $50M total. Anthropic is reported as a backer. — [The Next Web](https://thenextweb.com/news/sapiom-35m-series-a-ai-agent-cost-routing); [Dealroom](https://app.dealroom.co/news/note/sapiom-raises-35m-series-a-to-cut-the-cost-of-running-ai-agents)
- Claim: cuts agent runtime cost "sometimes tenfold" (company claim). — [The Next Web](https://thenextweb.com/news/sapiom-35m-series-a-ai-agent-cost-routing)

**Prava** (payments API for AI apps; consumer-facing apps)
- Payments API and SDK for tokenized card purchases with passkey user approval; integrates Visa Intelligent Commerce in the US (announced 2026-02-25). Targets AI shopping, travel and assistant apps. — [Prava blog, 2026-02-25](https://www.prava.space/blog/prava-integrates-visa-intelligent-commerce-to-enable-secure-card-based-agentic-payments-across-ai-applications); [Rye blog](https://rye.com/blog/agentic-commerce-startups)
- Also described as offering prepaid agent wallets, with one developer wallet funding many agents. — [Proxy blog](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)
- Funding: selected for WTFund Cohort 3 (Nikhil Kamath); amount not found. Its X account says it is available in the US and Southeast Asia, while the press release says US only. — [Prava blog](https://www.prava.space/blog); [X](https://x.com/pravapayments?lang=en)

**Proofpane** (runtime governance gateway with signed audit evidence; enterprise compliance)
- Runtime governance gateway for AI coding agents and MCP clients. Enforces allow, deny and human-in-the-loop policy and DLP redaction in the execution path. Writes a hash-chained audit exported as an Ed25519-signed, offline-verifiable evidence pack mapped to NIST AI RMF, ISO 42001, the EU AI Act, GDPR and SOC 2. Covers Claude Code, Cursor, Codex, n8n, Zapier, Make, UiPath and Power Automate. — [Proofpane](https://proofpane.com/); [GitHub releases](https://github.com/Proofpane/releases)
- The daemon is closed source; the reference architecture is CC BY 4.0. A community note says it uses no hardware attestation or TEE, so its evidence is tamper-evident in software only. — [awesome-ai-governance PR #44](https://github.com/agentrust-io/awesome-ai-governance/pull/44)
- No funding, customers or money-movement features found.

**Basis Theory** (card vault and tokenization; leads the Agentic Commerce Consortium)
- PCI-compliant, processor-agnostic vault. Formed the Agentic Commerce Consortium in September 2025 with Lithic, Skyfire, Rye and Crossmint; now 20+ members. Customers include Pinterest, Melio and MoneyGram. — [Rye blog](https://rye.com/blog/agentic-commerce-startups); [Proxy blog](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)
- Funding: $33M Series B (October 2025) led by Costanoa; about $50M total (secondary source). — [Rye blog](https://rye.com/blog/agentic-commerce-startups)

**Lithic and Marqeta** (card issuers adding agent features)
- Lithic: programmable virtual cards with Authorization Stream Access for real-time per-transaction rules. Partnered with Arcade.dev (August 2025); founding consortium member; Rye case study. — [Proxy blog](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)
- Marqeta: an MCP server that lets agents provision cards, operate within policies and handle disputes. Over $200B annual TPV cited. Appointed a Chief AI Officer in 2025. — [Proxy blog](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)

**Ramp and Brex** (corporate spend; enterprise finance)
- Ramp Agent Cards (early access): corporate cards issued to AI agents using Visa Intelligent Commerce tokenized credentials. Ramp says they support one-time use, a single merchant and amount, and that agents inherit the issuing user's limits and approval chains. These are vendor claims, and no launch date was found. — [Ramp blog](https://ramp.com/blog/virtual-cards-for-ai-agents); [Ramp blog](https://ramp.com/blog/ai-agent-spending-controls)
- Ramp has applied Visa Intelligent Commerce to automate B2B bill payments. — [Rye blog](https://rye.com/blog/agentic-commerce-startups)
- Brex: AI agents for receipts, policy checks and ERP sync; Fifth Third Commercial Card partnership (December 2025). No dated Brex agent-card launch found. — [Proxy blog](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)

**Other agent payments companies found**
- Proxy: virtual card per agent or workflow, with network-level limits and attestation before access; targets regulated industries and procurement. The author's own company, so all claims are self-reported. — [Proxy blog](https://www.useproxy.ai/blog/ai-agent-payments-landscape-2026)
- Causa Prima: buyers' and suppliers' AI agents negotiate payment terms and resolve billing disputes. $20M seed led by Base10 (October 2026); $30M total. — [Fortune, 2026-10-13](https://fortune.com/2026/10/13/startup-causa-prima-20-million-billing-disputes-ai-agents/)
- Rye: Universal Checkout API; Firmly ($5.2M; FJ Labs, Ark, Mastercard Start Path); Henry Labs; Zinc. All are checkout execution players. — [Rye blog](https://rye.com/blog/agentic-commerce-startups)
- Networks and platforms: Visa Intelligent Commerce (100+ partners; TAP); Mastercard Agent Pay (US cardholders, November 2025); Stripe and OpenAI's ACP and Stripe's Agentic Commerce Suite (December 2025); Google AP2; PayPal. — [Rye blog](https://rye.com/blog/agentic-commerce-startups)

**Adjacent agent governance and security (not payments-specific)**
- Runlayer: MCP security gateway tied to Okta and Entra. $11M seed (Khosla, Felicis; November 2025); a reported $30M Series A in June 2026 rests on a single source. — [Ventureburn](https://ventureburn.com/runlayer-seed-11m/); [Gravity tracker](https://gravity.fast/blog/ai-agent-funding-tracker-q3-2026/)
- Token Security (agent and non-human identity): $20M Series A led by Notable (January 2026), $28M total; company blog only. — [Token Security](https://www.token.security/blog/2025-a-year-of-momentum-and-innovation-for-token-security)
- Pluto Security: $42M total (October 2026), runtime enforcement on employee-built agents. — [Yahoo Finance](https://finance.yahoo.com/technology/ai/articles/pluto-security-raises-42m-enforce-171019132.html)
- Reco: $55M (September 2026), $140M total. TechCrunch notes that many agent security startups make very similar promises to discover and govern agents. — [TechCrunch, 2026-09-29](https://techcrunch.com/2026/09/29/reco-raises-55m-as-ai-agent-security-startups-crowd-the-market/)
- Trust and fraud for agent traffic: Forter (Trusted Agentic Commerce Protocol), HUMAN with Riskified (AgenticTrust), Cloudflare and Akamai (Visa TAP partner). — [Rye blog](https://rye.com/blog/agentic-commerce-startups)

### Inferences
- Most funded startups sell the payment rail or wallet (Natural, Catena, Crossmint, Skyfire) or checkout (Nekuda, Prava, Rye). Few sell a rail-agnostic consent, mandate and evidence layer that sits in front of existing tools like Stripe. AgentAuth's gateway-plus-evidence position looks less crowded, but Catena (governance as part of a bank), Sapiom (caps and rules as part of billing) and Ramp (limits inherited from existing approval chains) each bundle part of it.
- Card networks are absorbing the mandate and identity primitives (Visa Intelligent Commerce tokens, Mastercard Agentic Tokens, Visa TAP). Checkout-focused startups therefore face platform risk. An enterprise internal-governance layer that records and proves approvals across rails is less directly exposed.
- Proofpane is the nearest analogue to AgentAuth's signed, offline-verifiable evidence, but it targets coding agents and AI compliance frameworks, not money movement or finance controls.
- Rails split. Stablecoin or crypto-leaning: Crossmint (USDC on Base, x402), Skyfire (per TechCrunch), Natural (stablecoins plus bank), Catena (founded by a Circle co-founder, a16z crypto lead). Card and bank: Nekuda, Prava, Ramp, Brex, Lithic, Marqeta, Basis Theory, Proxy, Payman (Fifth Third).
- Audience split. Consumer and merchant checkout: Skyfire, Nekuda, Prava, Crossmint cards, Rye. Developer spend: Sapiom, Crossmint wallets, Natural. Enterprise finance and internal governance: Ramp, Brex, Catena, Proxy, Causa Prima, Proofpane (compliance).

### Gaps
- Traction: almost no named paying customers or volumes for Natural, Catena, Nekuda, Prava or Sapiom. Skyfire's "thousands of transactions daily" and Payman's $770K revenue are unverified claims.
- Skyfire's status in 2026 is unclear: one source reports a shutdown and others show 2026 partnerships.
- Payman's funding totals conflict ($3M, $9.57M, $13.8M), and its current product focus is unclear.
- Exact launch dates and pricing for Ramp Agent Cards were not found, and a Brex agent-card product was not confirmed.
- Lithic and Marqeta agent product launch dates came only from secondary blogs.
- Keycard (agent identity) funding was not found. Stripe's own agent products (Agent Toolkit, Issuing for agents, ACP) are the most direct platform threat to a "gateway in front of Stripe" but were not researched in depth.
- Proofpane's funding, team and customers were not found.
- Prava's funding amount was not found.
- PitchBook, Crunchbase and The Information were not accessed directly. Several funding figures rely on aggregators.
