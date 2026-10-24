# Competitive saturation and incumbent coverage: controlling and auditing AI agents that move money (as of late Oct 2026)

Method note: about 19 searches and fetches on 2026-10-24. Many hits were secondary blogs, SEO guides or vendor marketing. Primary sources fetched: TechCrunch (Natural), Stripe's Sessions 2026 recap, Ramp's blog and the Crossmint announcement. Every vendor capability below is a vendor claim unless marked otherwise. Several sources look AI-generated (Bitget academy, review aggregators); they are flagged where used.

## Q1. Startups in agent payments authorization, governance and identity: funding, traction, pivots, focus

### Takeaway
The space is crowded and well funded at seed to Series A. At least 15 named startups, plus new YC entrants in nearly every 2025-2026 batch, are in it. Almost all funded players focus on rails, wallets or checkout: getting an agent able to pay, mostly for consumers or developers. Very few disclose real traction. Those closest to Cosigna's thesis (a company's internal governance of its own agents across systems, with exact-transaction approval and audit evidence) are small: Ralio (pre-seed, $2.5M), Locus (YC F25) and the pivoted Payman (bank-facing). No one has clearly won the "cross-system governance layer for business agents" position.

### Cited Findings
**Larger or well-funded players**
- **Natural**: $30M Series A led by Forerunner (Kirsten Green), announced 2026-07-20, over $40M raised in total. Founded 2025 by Kahlil Lalji, Eric Wang and Walt Leung. It is "an agent orchestration layer that lets AI agents move and store funds", supporting stablecoins and bank payments, and it is rebuilding dispute handling. It calls Stripe its main competitor. It was in beta until the raise, and the article gives **no customer, volume or revenue figures**. — [TechCrunch](https://techcrunch.com/2026/07/20/natural-raises-30m-to-reinvent-payments-for-ai-agents-and-take-on-stripe/); [PYMNTS](https://www.pymnts.com/news/investment-tracker/2026/natural-raises-30-million-to-build-payment-rails-for-ai-agents/)
- Natural's "Vaults" product reportedly lets agents move money into accounts but not withdraw it. This is a spend control built into its own rail. — [Fintech Futures](https://www.fintechfutures.com/venture-capital-funding/natural-bags-30m-launches-agentic-payments-infrastructure) (via search summary; not fetched)
- **Catena Labs** ("bank for AI agents", co-founded by Circle's Sean Neville): $30M Series A on 2026-05-20, led by Acrew Capital and a16z crypto, with Breyer Capital, General Catalyst and QED. Its $18M seed was in May 2025, for about $48M in total. Sources disagree on who led the seed. — [FinSMEs](https://www.finsmes.com/2026/05/catena-labs-raises-30m-in-series-a-funding.html); [TheSaaSNews](https://www.thesaasnews.com/news/catena-labs-raises-30m-in-series-a/)
- **Basis Theory**: $33M Series B led by Costanoa (Oct 2025), about $50M in total. Customers include Pinterest, Melio and MoneyGram. It leads an "Agentic Commerce Consortium" of 20+ members, Crossmint among them. It is a credential vault, i.e. developer infrastructure. — [Rye landscape](https://rye.com/blog/agentic-commerce-startups)
- **Kite** (agent payments chain): $33M Series A led by PayPal Ventures and General Catalyst, later extended by Coinbase Ventures (around 2025). Mainnet launched on Avalanche on 2026-05-12. No transaction volume was disclosed. — [BeInCrypto](https://beincrypto.com/kite-investment-from-coinbase-ventures/); [Edgen](https://www.edgen.tech/news/post/kite-mainnet-targets-33-trillion-market-with-ai-agent-payments-on-avalanche)
- **Crossmint**: launched an Agentic Cards API on 2026-06-02, built on Visa Intelligent Commerce, with Basis Theory holding the card credentials outside the agent. It works with eligible US Visa cards inside OpenClaw, Claude Code, Hermes, Zo Computer and other platforms. The announcement gives no funding or usage numbers. — [Crossmint announcement](https://www.crossmint.com/announcement/agentic-cards-api-launch-visa-basistheory); [PR Newswire](https://www.prnewswire.com/news-releases/crossmint-launches-agentic-cards-api-using-visa-intelligent-commerce-and-basis-theory-302786624.html)

**Smaller or older players**
- **Skyfire**: about $9.5M in total (Aug–Oct 2024; Neuberger Berman, a16z CSX, Coinbase Ventures; DCVC per TechCrunch). The 2024 launch release says $8.5M. I found **no 2026 round**. Its "Agent Checkout processes thousands of transactions daily" claim comes from Rye's landscape and review sites, and is unverified. It ran a KYAPay plus Visa Intelligent Commerce demo. — [Tracxn](https://tracxn.com/d/companies/skyfire/__-gSNwLdAbLR2EH3jQO5ja24BZ2dqjmKWC_BS3-4pf1s); [BusinessWire 2024](https://www.businesswire.com/news/home/20240821247203/en/Introducing-Skyfire-Payment-Rails-for-AI); [Rye](https://rye.com/blog/agentic-commerce-startups)
- **Nekuda**: $5M seed (May 2025) led by Madrona, with Amex Ventures and Visa Ventures. Its SDK provides a "Secure Agent Wallet" and "Agentic Mandates". It was a Visa Intelligent Commerce launch partner and powers Henry Labs' checkout. Its focus is consumer checkout and developer infrastructure. No 2026 round found. — [Rye](https://rye.com/blog/agentic-commerce-startups)
- **Payman**: **pivoted**. It began as a marketplace where AI pays humans for tasks, and now sells to banks and credit unions: AI carries out banking transactions by voice or text on the bank's rails, "with policy controls, approvals, and audit trails". Tracxn records a $13.8M Series A (2024; Visa, Boost VC among investors). It partnered with Citizens State Bank (2026-05-12) and was selected for the ICBA ThinkTECH Accelerator (2026-05-20). No 2026 raise found. A claimed Fifth Third partnership is unverified. — [BusinessWire](https://www.businesswire.com/news/home/20260520362081/en/Payman-AI-Selected-for-ICBA-ThinkTECH-Accelerator); [Tracxn](https://tracxn.com/d/companies/payman-ai/__NSTYOZtZdNiGZxC0Vkul0dzUfj3ZUgPqDRNO08pHBUE); [Dupple review](https://dupple.com/reviews/payman)
- **Prava**: consumer checkout with tokenized cards and passkey approval inside AI apps, integrated with Visa Intelligent Commerce. Funding not disclosed. — [Rye](https://rye.com/blog/agentic-commerce-startups)
- **Ralio** (Europe): $2.5M pre-seed led by Sure Valley Ventures (2026-04-14), billed as "Europe's largest agentic payments" round. Positioning: "guardrails needed for autonomous AI systems to transact across B2B financial workflows". This is the **closest positioning to Cosigna** found. — [Finextra](https://www.finextra.com/newsarticle/47569/ralio-raises-25m-for-agentic-payments-platform); [SVV](https://www.svv.ai/news/investmentralio)
- **AIsa**: $6.5M seed led by Alibaba and Tribe Capital (July 2026), for payments by "AI employees" and enterprise controls. — [Forbes](https://www.forbes.com/sites/elainepofeldt/2026/07/03/startup-raises-65-million-by-making-it-easier-for-ai-employees-to-make-payments-online/)

**YC companies** (see also Q4)
- **Allowance** (2026 batch): the user approves each purchase on an iPhone and can set limits, merchant restrictions and expiry. It mints scoped one-time credentials. Consumer focus. — [YC payments directory](https://www.ycombinator.com/companies/industry/payments)
- **Locus** (F25): connects agents to funds with per-transaction permissions and limits, and claims each payment is "traceable, auditable, and fully reconcilable with your existing financial stack". **Its positioning overlaps directly with Cosigna.** — [YC payments directory](https://www.ycombinator.com/companies/industry/payments)
- **Sponge** (W26: "financial infrastructure for AI agents"), **Agentcard** (S26: single-use virtual cards for agents) and **Blaze** (S24: agentic finance with approval, running on its own Brazilian-regulated institution). — [Extruct W26](https://www.extruct.ai/data-room/ycombinator-companies-w26/); [Extruct S26](https://www.extruct.ai/data-room/ycombinator-companies-s26/); [YC directory](https://www.ycombinator.com/companies/industry/payments)

**India**
- I found **no Indian startup raise** in agent payments governance in 2026. Activity is at the rail and incumbent level:
  - **NPCI** is reportedly building a UPI agent framework, the "Unified Agent Protocol", for small payments without per-transaction approval, plus a liability framework. This comes from Reuters' unnamed sources, and NPCI had not confirmed specifications as of 2026-09-01.
  - **Pine Labs** launched **P3P** in June 2026 (agent UPI payments after a single upfront authorisation, said to be live).
  - A Razorpay, NPCI and OpenAI pilot (2025) ran ChatGPT payments via UPI Circle and Reserve Pay.
  - **PayCentral** was launched by Phronetic AI (backed by AvenuesAI) in Oct 2025.
  - Sources: [The Paypers](https://thepaypers.com/payments/news/india-prepares-agentic-payments-rollout-on-upi-network); [Yahoo/Reuters](https://finance.yahoo.com/technology/ai/articles/india-eyes-ai-driven-upi-084800065.html); [Digitimes](https://www.digitimes.com/news/a20260902VL207.html); [AvenuesAI Wikipedia](https://en.wikipedia.org/wiki/AvenuesAI)

### Inferences
- Capital is concentrated in rails and wallets (Natural, Catena, Kite, Basis Theory, about $30-50M each). Governance-first B2B players are pre-seed or seed (Ralio, Locus, AIsa). Investors are funding "let agents pay" more than "let companies control agents that pay".
- Disclosed traction is close to zero across the category. Natural came out of beta with no numbers, and Skyfire's "thousands daily" is unverified. Revenue at scale is likely still unproven everywhere.
- Payman's pivot from agents paying humans to bank-embedded agents with policy, approvals and audit suggests that, at least for Payman, the governance features sold better through a regulated distributor (banks) than directly. This is an inference from one company.
- The Natural Vaults, Ramp and Stripe controls (Q2) show that "spend limits" is becoming a standard feature inside every rail. Cosigna's defensible piece would have to be cross-rail scope, exact-transaction approval and verifiable evidence, not limits as such.

### Gaps
- Nothing reliable found on **Proofpane, Paid, Lava** or **Prava's funding**. Paid (Manny Medina's agent billing company) is likely an agent monetization/billing company rather than a governance competitor, but I could not verify this.
- No disclosed customer counts or volumes for Natural, Catena, Kite, Crossmint or Nekuda.
- No confirmed shutdowns found. Absence of evidence is not proof.
- Indian funding data (e.g. any seed-stage agent-governance startups) not found.

## Q2. Incumbent coverage of a company's internal governance of its own agents

### Takeaway
Every major incumbent shipped or previewed agent controls in 2025-2026, but **each covers only its own platform**:
- Stripe: Stripe objects and cards.
- Ramp: Ramp cards and bill pay.
- Coupa and SAP: their own workflows.
- Okta, Entra and AWS: identity and tool access, but **not** money semantics (amounts, payees, exact-transaction approval).

I found no incumbent product that gives one company cross-rail spend mandates plus exact-transaction human approval plus audit-grade, verifiable evidence for all of its agents. The pieces exist and are converging fast. Several key ones are still preview-only.

### Cited Findings
**Stripe** (Sessions 2026, vendor claims)
- "Agent guardrails" are in **Preview**: "assign agent identities, enforce scope rules, and configure approval flows for sensitive actions". "Issuing for agents" is also **Preview**: programmatic virtual cards per agent. The Treasury "agent-ready financial accounts" come with human confirmation for sensitive actions (no status given). "Link agent wallet" is available, with the consumer approving each spend request. On the roadmap: "Privy (Q4, private preview)" agents with a preset policy set, and Privy "human review for sensitive actions". The recap does not mention restricted keys or audit export. — [Stripe Sessions 2026 recap](https://stripe.com/blog/everything-we-announced-at-sessions-2026)
- Stripe's docs label Issuing for agents a **private preview**, with per-agent cards, spend limits, merchant category controls and custom authorization rules. One third-party guide claims it is available to any business, which conflicts with the docs. — [Stripe docs](https://docs.stripe.com/issuing/agents); [agenticplug](https://agenticplug.ai/blog/what-is-stripe-link-wallet-for-ai-agents)

**Ramp**
- "Agent Cards": tokenized virtual cards issued to agents via API or MCP, single-use, scoped to a merchant and amount. Agents inherit the issuing user's limits and **approval chain**. Transaction visibility is in Ramp's dashboard, with no dedicated audit export described. The page does not say whether it works outside Ramp, and gives no availability status. It also offers "AI Spend Intelligence" for token-usage reconciliation. — [Ramp blog](https://ramp.com/blog/ai-agent-spending-controls)
- Per Forkast's coverage, each spending agent on Ramp's platform gets an owner, a budget, merchant restrictions and a human approver. The coverage also mentions a reported $60B round. — [Forkast](https://forkast.news/ramps-reported-60b-round-shows-corporate-spend-software-turning-into-an-agent-platform/)

**Brex**
- Expense assistant, audit agents and review agents that auto-approve low-risk expenses and escalate the rest. **No agent-card product found.** — [Layer3labs comparison](https://www.layer3labs.io/comparisons/ramp-vs-brex) (secondary)

**Okta and Auth0**
- Auth0 for AI Agents went GA around 2025-11-19 (secondary sources). It offers async human approval via CIBA push: the agent pauses until a person approves out of band. — [Auth0 blog](https://auth0.com/blog/securing-amazon-bedrock-agentcore-agents-auth0-for-ai-agents/); [AWS APN blog](https://aws.amazon.com/blogs/apn/securing-enterprise-ready-ai-agents-with-auth0-for-ai-agents-and-amazon-bedrock-agentcore/)
- Okta for AI Agents went GA around April 30 or May 2026 (sources conflict). It registers agents, including AgentCore agents, and manages their access and deactivation with any IdP. — [Okta newsroom](https://www.okta.com/newsroom/articles/okta-expands-ai-agent-security-to-any-idp/); [NeuralCoreTech](https://neuralcoretech.com/ai-agent-identity-governance-2026/)

**Microsoft Entra Agent ID**
- Reported GA in April or May 2026 (secondary sources conflict). The results showed no human-approval mechanism for transactions. — [NeuralCoreTech](https://neuralcoretech.com/ai-agent-identity-governance-2026/); [usecarly](https://www.usecarly.com/blog/agent-identity/)

**AWS Bedrock AgentCore Policy**
- Cedar policies evaluated on Gateway tool calls, in LOG_ONLY or ENFORCE mode. It reportedly **can require a recorded approval but does not collect it**: the approval step has to be built as a tool or in a gateway. I found no official GA announcement. — [Aperion guide](https://aperion.ai/blog/aws-bedrock-agentcore-governance-guide/) (secondary); [AWS blog on OAuth consent](https://aws.amazon.com/blogs/machine-learning/manage-end-user-oauth-consent-for-ai-agents-with-amazon-bedrock-agentcore/)

**Coupa**
- The Payment Batch Creation Agent prepares approved invoices but "does not authorize the release of funds". Agents work "within defined policies and access controls". Coverage raises open questions about credential scoping for external agents connecting via MCP. — [Let's Data Science](https://letsdatascience.com/news/coupa-deploys-agent-for-payment-batch-creation-638a3390); [Procurement Magazine](https://procurementmag.com/news/coupa-scales-agentic-procurement-with-mcp-tools)

**SAP Joule**
- SAP published a Joule Agents compliance brief (June 2026) on auditability and human oversight under the EU AI Act. ERP Today says finance and GRC leaders must still assess whether agent workflows produce audit evidence. — [SAP brief](https://www.sap.com/documents/2026/06/526001c1-567f-0010-bca6-c68f7e60039b.html); [ERP Today](https://erp.today/sap-autonomous-finance-cfo-governance/)

**Workday**
- Acquired C1.ai (around 2026-10-16) to add agent permission and access control to its "Agent System of Record". — [Agentic Ready](https://www.getreadyforagents.com/news/workday-acquires-c1-ai-agent-identity-governance/) (secondary)

### Inferences
Coverage matrix (vendor claims; P = preview, own = inside its own platform only):

| Vendor | Cross-system limits | Exact-transaction human approval | Audit-grade evidence |
|---|---|---|---|
| Stripe | own (P) | own (P; Link is consumer-only) | own logs |
| Ramp | own | inherits approval chain (own) | dashboard |
| Okta, Auth0, Entra | identity/tool access, no money semantics | Auth0 CIBA is generic, not amount-bound | identity logs |
| AWS AgentCore | tool-call policy, generic | must be built | logs |
| Coupa, SAP | own workflows | own | own |

- The gap Cosigna targets (one mandate spanning Stripe, bank, cards and ERP, approval bound to the exact payee and amount, and an independently verifiable evidence bundle) is not covered by any single incumbent today. The likely competitive response is integration: Okta or Auth0 plus Stripe guardrails plus AgentCore Policy, which could cover about 70% of the need for companies already standardised on those vendors.
- The biggest threat is Stripe guardrails going GA, and Ramp extending Agent Cards and approvals to all spend. A customer that keeps its agents inside Stripe or Ramp has less reason to buy a separate layer.

### Gaps
- Visa and Mastercard agent tokens and spend controls are not re-researched (covered in the earlier report). I found no new evidence that either offers enterprise cross-system governance.
- Google Agentspace, Oracle, Salesforce Agentforce and Microsoft Copilot Studio: **no relevant results** on payment approval or governance. Their status is unknown.
- No official AWS GA date for AgentCore Policy. Okta and Entra GA dates come from secondary sources only.
- Agent-tagged or restricted keys in Stripe for agents: not confirmed in the 2026 recap.

## Q3. Acquisitions in agent security, identity and governance (2025-2026) as exit signals

### Takeaway
There is a strong, active M&A market for agent and non-human identity security: roughly $100M-$1.2B per deal, with acquirers including Cisco, CrowdStrike, SailPoint, Okta, ServiceNow, Palo Alto, Cyera and Workday. I found **no acquisition of an agent payments or financial governance startup**. The exit path for Cosigna would most plausibly run through identity and security buyers, or through fintech or spend platforms. The latter is not yet demonstrated.

### Cited Findings
Deal ledger compiled by guptadeepak.com, with the filings it cites. One page counts 30 deals between Apr 2025 and Aug 2026. — [guptadeepak.com](https://guptadeepak.com/identity-giants-bought-ai-agent-security/)

| Acquirer | Target | Dates | Price | Basis |
|---|---|---|---|---|
| CrowdStrike | SGNL | announced 2026-01-08 | about $740M (headline); about $628M net cash in 10-Q | [CNBC](https://www.cnbc.com/2026/01/08/crowdstrike-ai-cybersecurity-sgnl-acquisition.html); [CrowdStrike PR](https://www.crowdstrike.com/en-us/press-releases/crowdstrike-to-acquire-sgnl-to-transform-identity-security-for-ai-era/) |
| Cisco | Astrix Security | announced 2026-05-04, closed 2026-06-29 | about $400M per Calcalist; undisclosed by Cisco | guptadeepak.com |
| SailPoint | Entro Security | closed 2026-06-29 | reported $200M (SiliconANGLE) vs about $122.6M cash in 10-Q: **conflict** | [SiliconANGLE](https://siliconangle.com/2026/06/15/sailpoint-acquires-ai-agent-security-startup-entro-reported-200m/) |
| ServiceNow | Veza | closed Mar 2026 | about $1.2B (10-Q) | guptadeepak.com |
| Cyera | Oasis Security (non-human identity) | announced 2026-07-28, closed 2026-09-03 | "valued at $1B" | guptadeepak.com |
| Okta | Permiso | closed 2026-08-26 | undisclosed; about $200M per another source | guptadeepak.com |
| Palo Alto Networks | CyberArk | closed 2026-02-11 | $21.1B | guptadeepak.com |
| Palo Alto Networks | Protect AI | Jul 2025 | $634.5M | guptadeepak.com |
| Palo Alto Networks | Koi | — | $231M | guptadeepak.com |
| Palo Alto Networks | Portkey (AI gateway) | — | $117M | guptadeepak.com |
| SentinelOne | Prompt Security | — | $160.2M | guptadeepak.com |
| Tenable | Apex Security | — | $47.8M | guptadeepak.com |

- Other deals with undisclosed prices: Snowflake–Natoma, OpenAI–Promptfoo, F5–CalypsoAI, Cato–Aim Security. — [guptadeepak.com](https://guptadeepak.com/identity-giants-bought-ai-agent-security/)
- Workday acquired C1.ai for agent identity and access (around Oct 2026). — [Agentic Ready](https://www.getreadyforagents.com/news/workday-acquires-c1-ai-agent-identity-governance/)

### Inferences
- The buyers are paying for "agent/NHI identity and access governance", and Cosigna's credential brokering and mandate features sit adjacent to it. Portkey ($117M) shows that even AI gateways get bought. The fact that the large identity players have now acquired their agent-identity pieces also means **they will bundle agent access governance**, which narrows the room for a standalone generic agent-identity startup. A money-specific layer (amounts, payees, evidence) is less covered by these deals.

### Gaps
- No agent payments M&A found: no acquisitions of Skyfire, Nekuda, Payman or others.
- The guptadeepak.com ledger is a single compiler. I did not independently verify each 10-K and 10-Q figure.

## Q4. YC companies in the area, and YC partner views

### Takeaway
YC funds agent payments companies in nearly every batch: Locus (F25), Sponge (W26), Allowance (2026), Agentcard (S26), Blaze (S24). Most do consumer checkout or single-use cards; Locus is the most B2B and audit-oriented. YC's 2026 Requests for Startups reportedly name "identity, permissions, payments" for agent-native software. That is a demand signal and also a sign that more YC competitors are coming.

### Cited Findings
- Allowance, Locus and Blaze are listed in the YC payments directory, with the descriptions given in Q1. — [YC payments](https://www.ycombinator.com/companies/industry/payments); [YC finance](https://www.ycombinator.com/companies/industry/finance)
- Sponge (W26) and Agentcard (S26) appear in third-party batch lists only. — [Extruct W26](https://www.extruct.ai/data-room/ycombinator-companies-w26/); [Extruct S26](https://www.extruct.ai/data-room/ycombinator-companies-s26/)
- The YC W26 batch was reportedly "packed with AI agents". — [Alatirok](https://alatirok.com/why-yc-w26-was-packed-with-ai-agents/) (secondary)
- The Summer 2026 RFS reportedly covers "APIs, MCPs, CLIs, machine-readable documentation, identity, permissions, payments and agent-native software". Stablecoin financial services and agentic commerce also appear. — [VC Cafe](https://www.vccafe.com/2026/04/28/requests-for-startups-summer-2026-edition/); [VC Lens](https://vclens.substack.com/p/yc-requests-for-startups-2026-explained); [TechStartups](https://techstartups.com/2026/02/04/y-combinator-is-out-with-its-2026-request-for-startups/)

### Inferences
- YC's thesis supports agent permissions and payments as a category. Expect two to five new YC entrants per batch, which lowers barriers and increases noise, but most will target consumer or developer checkout rather than enterprise finance governance.

### Gaps
- No named YC partner quote on agent payments or governance was found or verified. The RFS summaries are third-party and should be checked against ycombinator.com/rfs.
- No funding or traction figures for the YC companies.
