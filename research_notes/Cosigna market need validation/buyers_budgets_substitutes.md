# Buyers, budgets and substitutes for an agent money-movement control/audit layer (Cosigna), as of Oct 2026

Method note: about 25 searches and fetches, run 2026-10-24. Many results were vendor pages or secondary aggregators, labelled as such below. Primary documents I fetched in full: Stripe two-party approvals docs, Ramp's agent spend-controls blog, the Medius Financial Census page, and The Hacker News summary of the Gartner Guardian Agents Market Guide (itself vendor-contributed). Everything else comes from search snippets, so the original wording was not checked.

## Q1. Who owns the problem and the budget?

### Takeaway
No single owner is established. Analyst categories that come close ("guardian agents" and "agent identity") are written for security and IAM leaders, who have the budgets. Finance surveys show finance leaders feel the control gap, but they split accountability for an AI-caused financial loss roughly evenly three ways. The likely economic buyer depends on how the product is framed: security/IAM if it is "credential brokering and agent authorization", controller/finance ops if it is "approvals and audit evidence over money movement". That split is a real go-to-market risk.

### Cited Findings
- Gartner published its first Market Guide for Guardian Agents on 2026-02-25 (Avivah Litan, Daryl Plummer). It is distributed to "enterprise security and identity leaders", and the summary names "security, compliance, and infrastructure teams" as the stakeholders. — [The Hacker News (vendor-contributed by Orchid Security)](https://thehackernews.com/2026/03/5-learnings-from-first-ever-gartner.html)
- Per that summary, Gartner's mandatory guardian-agent capabilities are AI visibility and traceability, continuous assurance and evaluation, and runtime inspection and enforcement. Gartner calls for a "neutral, trusted guardian agent layer" and says "enterprises will require independent guardian agent layers that operate across clouds, platforms" and identity systems. It also says deployments are "mainly prototypes or pilots". — [The Hacker News](https://thehackernews.com/2026/03/5-learnings-from-first-ever-gartner.html)
- Astrix is listed in the Market Guide's "Agent Identity" category, and Cato Networks under "IT or security platform vendors". Polygraf AI says it is a representative vendor. These are vendor self-reports. — [Astrix blog](https://astrix.security/learn/blog/market-guide-guardian-agents/); [Polygraf](https://polygraf.ai/news/polygraf-ai-recognized-as-a-representative-vendor-in-the-2026-gartner-market-guide-for-guardian-agents/)
- Secondary blog claims: Gartner made "agentic AI oversight" the #1 cybersecurity trend for 2026. Its trends report lists IAM adaptation for AI agents (identity registration, credential automation, policy-driven authorization for machine actors) as a top-six trend. Guardian agents are projected at 10-15% of the agentic AI market by 2030. 2026 infosec spend is forecast at $244.2B, up 13.3%. — [softwarestrategiesblog.com](https://softwarestrategiesblog.com/2026/03/24/information-security-spending-2026/) (secondary; not checked against Gartner)
- Medius Financial Census 2026 (updated 2026-08-25, sample size not stated): 90% require mandatory human approval for AI decisions, and 46% say approval thresholds are "understood but not formally documented". Asked who is responsible for an AI-caused financial loss: tech team 28%, finance leader 27%, the employee who acted 27%. 23% believe staff use unapproved AI tools. — [Medius (vendor survey)](https://www.medius.com/financial-census/ai-governance/)
- Avalara "Agents of Change" (more than 1,500 CFOs and senior finance leaders in the US, UK, India and Australia): 76% lack in-house finance expertise to understand how their AI agents work. 30% have not updated their internal control framework in the past year to cover agents taking actions. 46% have AI incident response plans that are untested or still in development. — [ERP Today](https://erp.today/avalara-agentic-ai-finance-governance-cfo-survey/) (vendor-sponsored survey)
- Deloitte Q2 2026 CFO Signals: only 43% of CFOs feel fully confident in their AI governance, and 59% name balancing speed against risk as the top challenge. — reported in search results via [ERP Today](https://erp.today/avalara-agentic-ai-finance-governance-cfo-survey/) (not checked against Deloitte's own release)
- Maximor 2026 survey of 100 mid-market finance leaders: 66% call human oversight of agentic AI extremely or very critical. — [Anrok (secondary)](https://www.anrok.com/resources/governance-frameworks-for-agentic-ai-in-finance)
- Team8 2025 CISO Village survey: nearly 70% of enterprises run AI agents in production, and two-thirds build agents in-house. — cited in [The Hacker News](https://thehackernews.com/2026/03/5-learnings-from-first-ever-gartner.html)
- Ramp's agent spend-controls post gives finance budgets, thresholds and merchant restrictions, and gives IT token quotas and model choice. — [Ramp blog, 2026-05-29](https://ramp.com/blog/ai-agent-spending-controls)

### Inferences
- The best-funded categories next to this one (agent security, agent identity) sell to the CISO and IAM. Finance tools (Ramp, Coupa, Medius) sell to the CFO and controller. Cosigna combines credential brokering (a security buy) with approvals and audit evidence (a finance and audit buy), so the sale may need two stakeholders. Expect longer cycles, or one of the two to dismiss it as "the other team's problem".
- Two findings point to a controller/internal-control wedge: 46% of finance teams have undocumented thresholds, and 30% have not updated control frameworks for agents. A product that writes mandates down as enforceable, auditable policy speaks directly to SOX-style control documentation. That is an inference; I found no source naming internal audit as a buyer.
- Gartner's "neutral, independent layer" language supports the cross-rail framing analytically. But Gartner also calls the market prototype and pilot stage, so budgets are probably still experimental or innovation money, not standing line items.

### Gaps
- No job postings found for roles like "AI agent finance controls" or "agentic AI controller". Not searched in depth.
- No Forrester or IDC material retrieved. Gartner AI TRiSM was not retrieved directly.
- No survey found that asks directly who holds the budget for agent payment controls.

## Q2. Pricing and willingness-to-pay signals from adjacent products

### Takeaway
Adjacent agent identity products publish per-agent or per-MAU prices around $15-20 per unit per month, or a 50% uplift on the base identity plan. Agent security vendors are raising very large rounds, which shows CISO budget appetite. AI governance platforms sell custom enterprise contracts with no public prices. For Cosigna, per-agent identity pricing is a low anchor. Value-based pricing (on money controlled, or per approved transaction) would need its own evidence.

### Cited Findings
- Aembit IAM for Agentic AI is GA. Teams plan is $20 per agent or client per month; free tier up to 10 workloads or AI agents (the FAQ says 3 AI agents); Enterprise is custom and unlimited. — [Aembit pricing (vendor)](https://aembit.io/pricing); [Aembit FAQ](https://aembit.io/faq/); [GA blog](https://aembit.io/blog/aembit-iam-for-agentic-ai-is-now-generally-available/)
- Auth0 for AI Agents add-on: +50% on the base price, including unlimited Token Vault and all CIBA forms. Example from the German pricing page: 500 MAU costs $18 on Essentials and $120 on Professional. Okta's service terms say the add-on is limited to Enterprise and Growth plans, which conflicts with the Essentials listing. — [Auth0 pricing (DE)](https://auth0.com/de/pricing.md); [Okta service terms](https://www.okta.com/content/dam/okta---digital/en_us/legal/okta-service-specific-terms-06-12-2026.pdf)
- Microsoft Agent 365 is reported GA on 2026-05-01 at $15 per user per month, with agents acting for a licensed user covered. Each agent gets an Entra Agent ID. — [Licensing School (secondary)](https://www.licensingschool.co.uk/licensing-blog/2026/03/23/agent-365-ga-announcement/). An earlier estimate of $10-50 per agent per month is not authoritative. — [Redress Compliance](https://redresscompliance.com/agent-365-licensing-guide.html). Entra Conditional Access for Agents and ID Protection for Agents service plans were set to roll out July-August 2026 for Agent 365 / M365 E7 tenants. — [MC1395007](https://mc.merill.net/message/MC1395007)
- Credo AI has no public pricing. It sells custom enterprise contracts priced by number of governed AI use cases or systems, with no free tier. — [Capterra](https://www.capterra.com/p/10050777/Credo-AI/); [AWS Marketplace](https://aws.amazon.com/marketplace/pp/prodview-x67krdatcdday)
- Astrix: enterprise contact-sales pricing, about $70M raised (aggregator, unverified). — [neuronfeed](https://neuronfeed.com/compare/astrix-security-vs-lumia-security)
- Zenity raised a $125M Series C led by Norwest on 2026-08-03, about $185M in total (another aggregator says $115M and $175M). It is reportedly named by Gartner as a frontrunner in AI agent governance (April 2026), with Fortune 500 bank and retail customers. — [SiliconANGLE](https://siliconangle.com/?p=802199); [Dealroom](https://dealroom.co/news/142790-zenity-raises-125m-to-secure-enterprises-ai-agents); [FinTech Global](https://fintech.global/2026/08/04/zenity-lands-125m-as-ai-agent-security-race-heats-up/)
- Noma Security raised a $100M Series B (Evolution Equity) around mid-2025. — [press repost](https://curatedtechnologynews.substack.com/p/ea471920bca2585a6fdb9bb9ee54189c)
- Ramp reportedly raised $1.85B at a $60B valuation, positioned partly around AI spend management. — [KuCoin blog (secondary)](https://www.kucoin.com/blog/ramp-1-85-billion-60-billion-valuation)
- Datarails (2024): companies at about $800M revenue spent an average of $63,598 per year on the finance stack. — [Datarails](https://datarails.com/?p=26824)

### Inferences
- Agent identity is becoming a cheap, bundled commodity. Microsoft is folding it into the M365 user licence, and Okta into a percentage uplift. A standalone "credential brokering" pitch therefore has weak pricing power on its own. Cosigna's pricing has to rest on the finance-control and audit-evidence parts.
- The Datarails figure suggests mid-market finance-stack budgets are modest: tens of thousands of dollars per year across all tools. A finance-bought control layer likely competes for a low-five-figure annual contract, unless it is sold into the security budget.

### Gaps
- No public prices found for Zenity, Noma, Lakera, Protect AI (acquired by Palo Alto Networks in 2025, from my prior knowledge; not verified here), Holistic AI or Oasis.
- No direct willingness-to-pay data, such as a survey, for agent payment controls.

## Q3. Substitutes and their coverage gaps

### Takeaway
The strongest substitute is native rail-level controls, and they have moved fast in 2026. Stripe now ships agent-tagged API keys with two-party approval rules. These cover refunds, payouts, outbound payments and transfers, with amount, actor and rate conditions, and Stripe enables them by default for agent keys. Card issuers (Ramp, Mercury, Visa and Mastercard agent tokens) cover agent purchasing with per-transaction scoping. For a Stripe-only or cards-only company, these probably cover most of the need for free, which is a real reason Cosigna could fail. The remaining gaps are cross-rail policy, one approval inbox across rails, evidence that leaves the vendor and can be verified independently, and credential isolation for non-Stripe rails such as bank APIs and ERPs.

### Cited Findings

Stripe (the most direct substitute for refund and payout use cases)
- Approval rules can gate: bank account add/update/delete, admin invite, payment intent creation, refund creation, subscription creation or cancellation, credit notes, customer balance transactions. **Only for agent-tagged API keys:** currency conversion, outbound payment, outbound transfer, payout. — [Stripe docs: two-party approvals](https://docs.stripe.com/account/approvals)
- Conditions: amount threshold, actor (team member or agent-tagged key), rate limit, combinable with and/or. Controls: "Require approval" (paused until approved) or "Block". Reviewers can be named people or roles. A user can't approve their own request. Requests expire after 14 days. Only one active rule per action. — [Stripe docs](https://docs.stripe.com/account/approvals)
- Default rules for agent-tagged keys: refund, subscription cancellation, outbound payment, outbound transfer, payout, currency conversion. An agent key that hits a rule gets an `approval_required` error, and an approval request is created automatically. The agent can attach a justification. — [Stripe docs](https://docs.stripe.com/account/approvals)
- Audit: approval events are logged in Dashboard Security history and in Workbench logs, and v2 webhooks are emitted (created, approved, rejected, expired, succeeded, failed). The API example uses version `2026-09-30.preview`, so parts may still be in preview. — [Stripe docs](https://docs.stripe.com/account/approvals)
- Restricted API keys let you set per-resource Read, Write or None permissions. — [Stripe restricted keys](https://docs.stripe.com/keys/restricted-api-keys); [permissions reference](https://docs.stripe.com/keys/permissions-reference). A third-party guide says Stripe's finest grouping bundles charges and refunds. — [Gravitee docs](https://documentation.gravitee.io/agent-management/build/mcp-proxies/third-party-mcp-servers/connect-and-secure-stripe-mcp-server) (not verified against Stripe)
- Gaps against Cosigna: Stripe-only. The agent still holds a Stripe key, albeit restricted and tagged. Approval scope is a rule on an action class with conditions, though each request concerns a specific action. Evidence lives in Stripe's dashboard and logs, not in an exportable tamper-evident form. I found no per-agent cumulative budget or mandate across a period, only rate limits and amount per action (inferred from the docs as listed).

Corporate and virtual cards
- Ramp Agent Cards are single-use, scoped to one merchant and the requested amount, and expire after first authorization or 12 hours. The ceiling is set by a Ramp fund, and agents connect via MCP or CLI. — [Ramp agent docs](https://agents.ramp.com/docs/agent-cards/overview)
- "Agents inherit the issuing user's approval chain". Full transaction visibility in Ramp's dashboard. The post covers only card purchasing, with no mention of bill pay or ACH for agents. AI Spend Intelligence reconciles token usage against invoices. — [Ramp blog 2026-05-29](https://ramp.com/blog/ai-agent-spending-controls)
- Mercury Agent Cards: transactions outside the rules are declined, and the agent cannot raise its own limit. — per search snippet via [PYMNTS](https://www.pymnts.com/?p=4071149) / Ramp results (not fetched)
- Mastercard Agent Pay Agentic Tokens are bound to an agent identity and cardholder mandates (spend ceilings, MCCs, time windows). Visa Intelligent Commerce / Trusted Agent Protocol uses per-transaction signed intents. — [eco.com comparison (secondary)](https://eco.com/support/en/articles/15192003-mastercard-agent-pay-vs-visa-trusted-agent-2026-compared); [Crossmint](https://crossmint.com/learn/agent-card-payments-compared). Reported US rollout of Agent Pay to all Mastercard cardholders by Nov 2025 (secondary; launch timing conflicts across sources).
- Gaps against Cosigna: these control outbound purchasing on card rails, which is spend. They do not cover refunds the business issues, payouts to third parties, bank transfers, or ERP postings. Cards give strong credential isolation, since the agent never sees a reusable PAN.

Identity and authorization platforms
- Auth0 for AI Agents: Token Vault "manages the access token lifecycle so agents never need to handle credentials". CIBA provides out-of-band human consent for high-stakes actions. — [Auth0 blog](https://auth0.com/blog/secure-ai-stop-managing-api-keys-by-hand/); [Auth0 AI docs](https://auth0.com/ai/docs/fr-ca/overview)
- Microsoft Entra Agent ID gives each agent an identity with lifecycle and access management. Conditional Access and ID Protection for agents are rolling out. — [Licensing School](https://www.licensingschool.co.uk/licensing-blog/2026/03/23/agent-365-ga-announcement/); [MC1395007](https://mc.merill.net/message/MC1395007)
- Aembit: an MCP Authorization Server and MCP Identity Gateway, i.e. credential brokering for agents. — [Aembit](https://aembit.io/blog/aembit-iam-for-agentic-ai-is-now-generally-available/)
- Gaps against Cosigna (inferred): these platforms authorize access to an API, but they don't understand payment semantics such as amount, beneficiary, cumulative per-agent budget or idempotency. CIBA gives a generic consent prompt. It could show the exact transaction, but the developer has to build that and the policy logic. Audit stays as identity logs, not finance-grade evidence. Credential brokering is largely covered here, which weakens that part of Cosigna's value.

ERP and AP automation
- Coupa says its September 2026 release brings agents that handle payment and sourcing workflows autonomously. Coupa Navi agents "accelerate approvals". — [Coupa (vendor)](https://www.coupa.com/?p=68343)
- NetSuite AP automation: AI bill capture and scheduling payments inside NetSuite. Third-party AP agents say "approvals remain with humans; the agent handles the preparation and routing." — [Oracle NetSuite community](https://community.oracle.com/netsuite/english/events/11995-ap-on-autopilot-let-ai-do-the-heavy-lifting); [Hyperbots](https://blog.hyperbots.com/how-agentic-ai-is-changing-finance-accounting-operations-on-netsuite)
- Gaps against Cosigna (inferred): ERP approval chains are mature and auditor-accepted for AP inside the ERP. If an AI agent works through the ERP's own workflow, the existing approval matrix already applies, which is a strong substitute for AP. They do not govern actions outside the ERP, such as Stripe refunds or direct bank API payouts. No SAP-specific data was found.

Agent security / guardian-agent platforms (Zenity, Noma, etc.)
- Runtime inspection and enforcement, visibility and traceability. — [The Hacker News on the Gartner guide](https://thehackernews.com/2026/03/5-learnings-from-first-ever-gartner.html)
- Gaps (inferred): their policies are generic (data leakage, prompt injection, tool-call policy). I found nothing showing financial-mandate semantics or exact-transaction approval. They are a likely future competitor if they add finance policy packs.

Keeping a human in the loop (the status quo)
- 90% of finance orgs already require mandatory human approval for AI decisions. — [Medius](https://www.medius.com/financial-census/ai-governance/)
- Inference: the status quo is "agents prepare, humans execute". This substitute caps risk at zero, and the cost is the automation benefit lost. Cosigna's value only appears once firms want agents to execute below thresholds. Medius also reports 45% "often" acting on AI recommendations without human intervention, which conflicts with the 90% figure and suggests the formal policy and actual practice diverge.

### Inferences
- Where a substitute fully covers the need:
  - (a) A Stripe-only business that wants refund and payout approvals for agents. Stripe's native approvals now do this for free, with defaults on.
  - (b) Agent purchasing on cards, covered by Ramp, Mercury and Mastercard agent tokens.
  - (c) AP payments routed through the ERP's own approval matrix.
- Cosigna's defensible gap is narrower:
  - one mandate and approval inbox across several rails;
  - cumulative per-agent budgets across rails;
  - evidence auditors can verify outside the vendor's dashboard;
  - brokering credentials for rails that lack native agent controls, such as bank APIs and smaller PSPs.
- Gartner's "independent guardian layer" thesis supports this, but it remains a thesis.
- Risk: other PSPs (Adyen, PayPal) and banks may copy Stripe's agent-key-plus-approvals pattern, which would further shrink the gap.

### Gaps
- Not covered in depth: Okta Fine-Grained Authorization specifics, UiPath governance for agentic automation, Brex agent features, Stripe Issuing agent features, Adyen or PayPal agent controls.
- Not checked: whether Stripe approvals are GA or preview for all accounts, and whether Stripe has per-agent cumulative spend limits.
- Visa and Mastercard details come only from secondary comparison sites.

## Q4. Is the "neutral cross-rail control layer" framing supported by multi-system usage?

### Takeaway
Partly. Merchants use multiple PSPs and gateways (typically 2-4), and enterprise CFOs report many financial systems. A 2026 survey of mostly European finance teams, though, finds most use six or fewer finance tools, and small firms one to three. Cross-rail fragmentation is real at mid-market and enterprise. For startups and SMBs, often on Stripe plus one card provider, native controls may be enough.

### Cited Findings
- Ecommpay survey (around Feb 2025, vendor sample): 70.12% of merchants use 2-3 payment providers, 26.29% use 4-5, and 3.19% use one. — [Ecommpay press room](https://ecommpay.com/press-room/merchants-focus-on-functionality-reliability-and-speed-of-payment-providers/); [CFOtech UK](https://cfotech.co.uk/story/ecommpay-study-reveals-factors-merchants-prioritise-most) (date label inconsistent)
- Visa Acceptance / MRC 2025 Global Fraud and Payments Report (per search results): merchants typically use about 3-4 gateways and acquirers. The 2024 edition put the average at four gateways or processors and three acquiring banks. — [MRC 2025 report](https://info.merchantriskcouncil.org/hubfs/Documents/Reports/Fraud%20Reports/2025_Global_Fraud_and_Payments_Report.pdf); [Verifi 2024 report](https://www.retail-systems.com/rs/images/VERIFI-2024-Global-Fraud-Payments-Report.pdf) (PDFs not fetched; summary from search)
- 451 Research Merchant Study 2022: 59% prefer a multi-provider approach. — [S&P Global](https://www.spglobal.com/marketintelligence/en/news-insights/research/considering-the-total-cost-of-payments)
- Stripe 2023 survey of 1,700 CFOs: 63% juggle more than 10 systems to track financials, and a quarter of enterprises use more than 50. — [Accountants Daily (secondary)](https://www.accountantsdaily.com.au/technology/19127-software-overload-means-most-cfos-stick-to-manual-processes)
- CFO Connect 2026 (215 finance leaders in France, UK, Germany and US; sponsored by Spendesk, NetSuite and Remote): more than 90% use six or fewer finance tools (41% use 1-3, 50% use 4-6). Firms under 20 employees: 65% use 1-3. Firms with 50-249 employees: 61% use 4-6. — [Spendesk blog](https://www.spendesk.com/blog/finance-tech-stack-evaluation/); [IT Brief UK](https://itbrief.co.uk/story/finance-teams-trim-software-stacks-as-ai-use-rises)

### Inferences
- Even mid-market finance teams run four to six finance tools: ERP, AP/spend, cards, PSP, bank. An agent that touches more than one of them needs either several native control planes, each with its own logs, or one neutral layer. That supports the framing for mid-market and enterprise.
- The multi-PSP data concerns acceptance (pay-ins), not agent-initiated outbound actions. The overlap with Cosigna's use cases (refunds, payouts) is indirect. Firms with multiple PSPs do issue refunds on each, though.
- For the earliest likely adopters, AI-native startups, stacks are small and often Stripe-centric. Stripe's native approvals make the neutral-layer pitch weakest exactly where adoption may start first.

### Gaps
- No data found on how many distinct rails an AI agent actually touches in production deployments.
- No data on multi-bank API usage for outbound payouts in the mid-market.
