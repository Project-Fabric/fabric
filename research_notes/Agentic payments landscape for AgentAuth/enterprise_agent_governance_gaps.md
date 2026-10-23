# Governing a company's own AI agents that move its money (finance ops), as of October 2026

Research date: 2026-10-21. Every product claim is dated and marked as a vendor claim where it comes from the vendor. About 18 search/fetch calls were made. Several primary documents (COSO's original PDF, IIA/AICPA standards text, Coupa/Tipalti/Navan agent pages, the IETF interim slide deck) could not be read directly and are listed under Gaps.

## 1. How enterprises control money movement by people today (DoA, approvals, dual control, SoD, vendor master)

### Takeaway
Human disbursement control rests on segregation of duties, approval routing by amount and approver limit, dual control on sensitive vendor-master and bank-detail changes, and change logs that auditors test. These controls live in system permissions and workflow configuration inside each ERP or AP tool. Nothing portable sits across systems. COSO's February 2026 GenAI guidance maps AI onto the existing 2013 framework rather than defining agent-specific disbursement controls.

### Cited Findings
- SoD in AP: practitioner guidance says nobody who can create or change a vendor record should also be able to release a payment to that vendor. The aim is that fraud needs collusion, not just access. — [Ramp, "Segregation of duties in AP", 25 May 2026](https://ramp.com/blog/accounts-payable/segregation-of-duties-in-accounts-payable); [Corpay](https://www.corpay.com/resources/blog/segregation-of-duties-accounts-payable)
- Bank-account changes on vendor records should go through documented change control with dual authorization. Auditors review the vendor master change log for who changed which fields, when, and whether the change was authorized. — [Corpay AP audit checklist](https://www.corpay.com/en-GB/resources/blog/accounts-payable-audit)
- Auditors test system permissions, not the org chart. Small finance teams cannot reach textbook SoD, and compensating controls only partly substitute. — [Corpay](https://www.corpay.com/resources/blog/segregation-of-duties-accounts-payable)
- Fraud base rate: Corpay, citing the ACFE 2026 Report to the Nations, says more than half of 2,402 occupational fraud cases involved an internal control failure. — [Corpay](https://www.corpay.com/resources/blog/segregation-of-duties-accounts-payable) (secondary citation of ACFE)
- SAP ERP "dual control": changes to sensitive fields in customer and vendor master records must be confirmed by a second authorized user. The article notes this is hard to apply in low-headcount organizations. — [SAPinsider](https://sapinsider.org/monitor-sensitive-fields-with-r-3s-dual-control-functionality/)
- NetSuite SuiteApprovals (Oracle docs):
  - A rule's Record Amount Limit auto-approves records below that amount.
  - Employee approval limits can drive auto-approval.
  - Approval chains can be built by approver type, for example "Employee Supervisor" with "Approval Limit: 10000".
  - Vendor bills that don't need approval under the exception set are auto-approved.
  - Cap: 18,000 approver records per account.
  — [Oracle NetSuite Help: Automatic Approvals](https://docs.oracle.com/en/cloud/saas/netsuite/ns-online-help/bridgehead_1544019693.html); [Creating Your First Approval Rule](https://docs.oracle.com/en/cloud/saas/netsuite/ns-online-help/section_90132659089.html)
- BILL exposes an Approvals API for approval policies and approver assignments on bills and vendor credits. Actions include approve, deny, set-approvers and clear-approvers. This is documented by a third party, not BILL's own docs. — [Jentic (third-party)](https://jentic.com/apis/bill.com/bill-approvals)
- COSO, *Achieving Effective Internal Control Over Generative AI* (released about 23–26 Feb 2026):
  - It applies the 2013 Internal Control–Integrated Framework (5 components, 17 principles) to GenAI and does not create a new framework.
  - It defines 8 capability types: ingestion, transformation, **posting**, **orchestration**, judgment, monitoring, regulatory intelligence and human-AI interaction.
  - It gives a 6-step roadmap: govern, inventory, assess, design, implement, monitor.
  - It includes "illustrative metrics to support both operational monitoring and audit evidence collection".
  - The Journal of Accountancy write-up does not mention agents/autonomous AI or segregation of duties.
  — [Journal of Accountancy, 26 Feb 2026](https://www.journalofaccountancy.com/news/2026/feb/coso-creates-audit-ready-guidance-for-governing-generative-ai/); [Deloitte DART Heads Up 2026](https://dart.deloitte.com/USDART/home/publications/deloitte/heads-up/2026/coso-internal-controls-generative-ai); [RSM](https://rsmus.com/insights/services/risk-fraud-cybersecurity/coso-aligns-ai-governance-with-internal-control-guidance.html)
- Advisory reading of COSO: treat AI outputs as assertions that need validation. External auditors are expected to reference this guidance more and more when evaluating AI-related controls (this is a forecast, not a fact). — [EisnerAmper](https://www.eisneramper.com/insights/artificial-intelligence-insights/coso-ai-governance-internal-control-framework-0526/); [RSM](https://rsmus.com/insights/services/risk-fraud-cybersecurity/coso-aligns-ai-governance-with-internal-control-guidance.html)

### Inferences
- The human control model has three parts: (1) identity-bound authority limits (approver limit per employee or role, the DoA), (2) a second person for high-risk actions (dual control on payment release and on vendor bank changes), and (3) a reviewable change and approval log. An agent needs the equivalent of all three. Today each lives inside a separate system's configuration: NetSuite approver records, SAP dual-control fields, BILL approval policies.
- COSO's "posting" and "orchestration" capability types are the closest official hooks for agent money movement. From the coverage available, COSO does not prescribe per-agent spending authority or agent SoD. A product could position itself as implementing COSO controls for those capability types, but should not claim COSO "requires" any specific mechanism.
- If an agent can both create or edit a vendor and release a payment, that breaks the core AP SoD rule. Agent SoD (one agent identity must not hold both permissions) is a direct, explainable requirement.

### Gaps
- I did not retrieve primary IIA Global Internal Audit Standards text, AICPA material on disbursement controls, or a primary DoA-matrix design reference. The SoD and dual-control findings above come from practitioner and vendor sources.
- I did not read the original COSO document. Whether it addresses autonomous agents, SoD for AI or human-approval thresholds is unverified.
- SAP S/4HANA and Oracle Fusion payment-release approval (for example SAP Bank Communication Management release strategies) was not researched in detail.

## 2. AI agents in finance operations: deployments, vendor controls, survey data

### Takeaway
By late 2026 every major AP/spend platform and ERP ships finance agents. Most agent features recommend, code, match or route. Some now execute: BILL says its pay-for-you agent makes card payments with no human involved, and Ramp has Agent Cards in alpha. Published guardrails are mostly the vendor's in-product policy engine (card limits, merchant controls, existing approval chains). None of these vendors publishes a cross-system, per-agent authority model or auditor-verifiable evidence of agent actions. Surveys show high intent, low realized value, and human oversight named as a top challenge.

### Cited Findings: vendors (all vendor claims unless noted)
- **Ramp**
  - Oct 7, 2025: AP agents pull contracts, email history, prior payments and POs. They issued 90,000 approval recommendations with a 90% acceptance rate (vendor claim) and use 60+ fraud signals before routing. They *recommend*; humans approve. — [Ramp blog, 7 Oct 2025](https://ramp.com/blog/ramp-ap-agents-announcement)
  - Dec 2025: AP agents surface each bill's context with an approve/reject recommendation. — [Ramp 2025 release notes, 16 Dec 2025](https://ramp.com/blog/2025-release-notes)
  - Apr 16, 2026: "Ramp Agent Cards are corporate cards issued directly to AI agents with real spend limits, merchant controls, and full transaction visibility". Status is alpha, early-access sign-up only. Ramp says the "same governance that applies to every employee now applies to every agent." Ramp also offers MCP/CLI connectors with 50+ finance tools, with approvals, bill creation and invoice tools planned. A controller profiled in the post does *not* approve or release payments via AI and keeps that step in Ramp's product, "where there is a full audit trail". — [Ramp blog, 16 Apr 2026](https://ramp.com/blog/ramp-customers-are-running-month-end-with-ai-assistants)
  - I found no published dollar thresholds or approval-limit rules for Ramp's AP agents.
- **BILL**
  - FY2026 10-K (FY ended 30 Jun 2026): launched "BILL AI, including our first suite of AI agents" covering W-9s, support, touchless transactions, auto-reconciliation and invoice coding. — [BILL 10-K FY2026, SEC](https://www.sec.gov/Archives/edgar/data/0001786352/000162828026058238/bill-20260630.htm)
  - Q3 FY26 earnings call (third-party transcript, unverified): a "pay-for-you" agent "autonomously executes card payments based on each supplier's preferences" and has completed "tens of thousands of card transactions without any human interaction". BILL says "well over 100,000 customers" use its agents. — [StatementDog transcript (third-party)](https://statementdog.com/analysis/BILL/earnings_calls/313379)
  - Apr–Jun 2026: a Transaction Agent is available to all Spend & Expense customers. The W-9 workflow auto-approves when validation confidence is high. — [BILL blog, Apr–Jun 2026](https://www.bill.com/blog/new-at-bill-april-june-2026)
  - I found no BILL source on whether agent-initiated payments are gated by the customer's existing approval policies.
- **Brex**
  - Brex says its agents are "trained on your policy and SOPs" and monitor transactions in real time for anomalies. — [Brex Intelligence page](https://brex.com/intelligence)
  - Mar 2026: nearly 70% of expenses on Brex are handled entirely by automation (vendor claim). A separate Anthropic case study says 75% of transactions are automated with 94% compliance (vendor claim; the two figures differ). — [Brex press](https://www.brex.com/journal/press/brex-helps-power-open-ai-global-spend-and-financial-operations); [Claude customer story](https://claude.com/customers/brex)
  - A third-party guide (Aug 2026) lists Brex Expense, Audit and Review agents. — [Rework](https://resources.rework.com/tools/ai-agents/best-ai-agents-for-finance-2026)
- **Oracle Fusion (Release 26B, 2026)**
  - The Payables Agent ingests invoices from email, portals, EDI and PDF. It matches to POs and receipts, applies "tax, policy, and fraud checks", then "route[s] for approval and payment".
  - The Payments Agent manages early pay, virtual cards and financing, handles bank interactions, and monitors acknowledgements and exceptions.
  - Oracle's blog says these agents are generally available. Version 1 says some advanced Payments features need a Service Request. — [Oracle Fusion Insider blog](https://blogs.oracle.com/fusioninsider/agentic-ai-in-erp-four-agents-you-can-use-today); [Version 1, 7 Jun 2026](https://www.version1.com/?p=7996)
  - A secondary source claims "configurable deterministic guardrails and flexible approval frameworks, including human-in-the-loop approval controls". This is not Oracle documentation. — [Evoke Technologies](https://www.evoketechnologies.com/?p=70607)
  - Forrester: Fusion agents are "shifting from assist to decide". It advises defining guardrails before adoption. — [Forrester blog](https://www.forrester.com/blogs/oracle-applications-analyst-summit-fusion-agents-are-shifting-from-assist-to-decide/)
- **SAP**
  - Joule finance agents (including an Accounting Accruals Agent) were announced at Sapphire 2025. SAP's finance page lists an "Accounts Payable Assistant". — [SAP finance AI page](https://www.sap.com/products/artificial-intelligence/ai-assistant/finance.html)
  - SAP Concur Fusion 2026 announced expense-report generation and receipt-validation agents, plus payments partnerships with Amex, Amex GBT and Visa. — [ERP Today](https://erp.today/sap-concur-fusion-2026-sap-embeds-joule-ai-across-travel-expense-and-payments-workflows/); [SAPinsider](https://sapinsider.org/blogs/sap-unveils-joule-ai-and-payments-integrations-at-concur-fusion-2026/)
  - I found no SAP AP agent that releases payments.

### Cited Findings: surveys
- Deloitte Q4 2025 CFO Signals:
  - 87% of CFOs say AI will be extremely or very important to finance operations in 2026.
  - 54% say integrating AI agents into finance is a top 2026 transformation priority, the top response, ahead of data quality at 52%.
  — [Deloitte Q4 2025 CFO Signals](https://www.deloitte.com/us/en/insights/topics/business-strategy-growth/4q-2025-cfo-signals-survey.html)
- Deloitte Finance Trends 2026: among the 63% who say AI is fully deployed, only 21% see tangible value and 14% have fully integrated AI agents into finance. — [CFO.com / Deloitte coverage](https://www.cfo.com/news/enterprise-cfos-see-sharp-rise-in-confidence-ai-use-deloitte-2026-cfo-signals-survey-/809580/); [Accounting Today](https://accountingtoday.com/news/cfos-prioritize-digital-transformation-in-2026)
- Gartner, 28 May 2026 (survey of 204 finance leaders, Mar 2026): finance is moving AI into production, but CFOs struggle to turn deployment into value. — [Gartner press release](https://www.gartner.com/en/newsroom/press-releases/2026-05-28-gartner-says-cfos-must-stop-mistaking-finance-ai-deployment-for-value-creation)
- Gartner, 23 Mar 2026 (100 CFOs): acquiring and developing AI talent is CFOs' top near-term challenge. — [Gartner press release](https://www.gartner.com/en/newsroom/press-releases/2026-03-23-gartner-survey-reveals-acquiring-and-developing-ai-and-digital-talent-is-cfos-top-near-term-challenge)
- Gartner's January 2026 budget benchmarks (survey of 300+ CFOs, October 2025): nearly 60% of CFOs plan to raise finance AI investment by at least 10% in 2026. — [CPA Practice Advisor](https://www.cpapracticeadvisor.com/2026/02/10/cfos-budget-plans-prioritize-growth-functions-technology-and-ai-in-2026/177774)
- Gartner use-case shares, unverified (secondary only): knowledge management 49%, AP automation 37%, anomaly detection 34%. — [Miles Education (secondary)](https://www.mileseducation.com/blog/accounting/ai-agents-in-accounting)
- Gartner predicts more than 40% of agentic AI projects will be canceled by end of 2027, citing unclear value and inadequate risk controls. Seen via a secondary source here. — [Rework (secondary)](https://resources.rework.com/tools/ai-agents/best-ai-agents-for-finance-2026)
- KPMG:
  - Global AI Pulse (31 Mar 2026): only 11% have reached deploying and scaling agents for enterprise-wide outcomes. — [Enterprise DNA summary](https://enterprisedna.co/resources/news/kpmg-global-ai-pulse-q1-2026/)
  - US Q2 2026: 67% cite human-oversight skills (human-in-the-loop judgment, escalation) as a top challenge in deploying agents. — [KPMG pulse Q2 PDF](https://kpmg.com/kpmg-us/content/dam/kpmg/pdf/2026/tech-aipulsesurvey-q2.pdf)
  - US Q3 2026 (asset management/PE, fielded 10 Aug–14 Sep): chart labels suggest agents are deployed in risk/legal/compliance (92%) and finance (89%). The extraction was hard to read, so verify before citing. — [KPMG AMPE Q3 PDF](https://kpmg.com/kpmg-us/content/dam/kpmg/pdf/2026/ampe-aipulsesurvey-q3.pdf)
  - Q4 2025: leaders segment sensitive data to keep agent actions auditable. — [KPMG Q4 2025 PDF](https://kpmg.com/kpmg-us/content/dam/kpmg/pdf/2026/ai-quarterly-pulse-survey-q4-2025.pdf)

### Inferences
- The industry has moved from agents that recommend to agents that execute: BILL card payments, Oracle's Payments Agent, Ramp Agent Cards. Controllers still keep payment *release* in the vendor's native approval UI because that is where the audit trail lives (Ramp April 2026 anecdote). The open need is an audit trail and authority model *for the agent itself*.
- Each vendor's guardrails stop at its own boundary. Ramp limits apply to Ramp cards, BILL policies to BILL payments, NetSuite approver limits to NetSuite records. A company running agents across Stripe, a bank portal, an ERP and a spend tool has no single per-agent budget or approval policy.
- The surveys show demand (54% of CFOs prioritize agents), a value gap, and oversight concerns (KPMG's 67% figure, Gartner's cancellation prediction tied to risk controls). Together that supports a "controls before scale" pitch, but none of these surveys directly measures demand for an agent-authority product.

### Gaps
- I did not obtain primary information on Coupa (Navi/Navigator agents), Tipalti or Navan agent features and guardrails in 2026.
- I found no vendor documentation of numeric agent approval limits, per-agent SoD, or how agent-initiated payments interact with existing approval chains, for BILL, Ramp AP or Oracle.
- I found no McKinsey or PwC 2026 finance-agent data in this pass.
- I found no public incident or loss data for finance agents.

## 3. Agent identity and authorization standards: what they cover and miss for money movement

### Takeaway
Identity products and standards now cover who the agent is, which human sponsors it, delegation chains (RFC 8693 token exchange and on-behalf-of drafts), out-of-band human consent (CIBA plus RAR), and resource-bound tokens for MCP. They do not define cumulative spending authority per agent, enterprise DoA thresholds, dual approval, SoD between agents, reversal, or audit evidence a finance auditor can verify independently. The one partial exception is an individual July 2026 draft that adds signed "authorization evidence" and Rego policies.

### Cited Findings
- **MCP authorization spec (2025-11-25 revision)**:
  - Authorization is OPTIONAL. Where implemented over HTTP it uses OAuth 2.1, Protected Resource Metadata (RFC 9728), Resource Indicators (RFC 8707, mandatory `resource` parameter), Client ID Metadata Documents and PKCE.
  - Scopes are the permission unit, with step-up via `insufficient_scope`.
  - Token passthrough is forbidden.
  - STDIO servers should take credentials from the environment instead.
  - The spec does not define amount limits, transaction-level authorization, approval workflows or audit logging. Extensions live in a separate ext-auth repo.
  — [MCP spec: Authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
- **IETF OAuth drafts on agents**:
  - draft-oauth-ai-agents-on-behalf-of-user-02 (WSO2, Aug 2025, Informational individual draft, expiry Feb 2026). It adds `requested_actor` and `actor_token` and records the user-to-agent delegation chain in token claims. — [datatracker](https://datatracker.ietf.org/doc/html/draft-oauth-ai-agents-on-behalf-of-user-02)
  - A Huawei draft (Nov 2025) lets a lead agent request tokens for sub-agents. — [draft-song-oauth-ai-agent-collaborate-authz-00](https://www.ietf.org/archive/id/draft-song-oauth-ai-agent-collaborate-authz-00.html)
  - An IETF OAuth interim (2026-oauth-03, slides dated about 1 Jun 2026) analyzed individual drafts on AI agent authorization. The search summary says it counted 32 drafts in about 5 clusters. I could not decode the PDF to verify this. — [interim slides](https://datatracker.ietf.org/meeting/interim-2026-oauth-03/materials/slides-interim-2026-oauth-03-sessa-ietf-individual-draft-analysis-00)
  - **draft-liu-ai-agent-authorization-integration-00** (6 Jul 2026, Informational individual draft; authors from Alibaba, Cisco, and Okta (Aaron Parecki)):
    - It combines ID-JAG (cross-domain identity grant), OAuth SPIFFE client authentication (agent workload identity), RAR (RFC 9396) with a Rego policy type and an "authorization evidence" type, Token Exchange (RFC 8693), CIBA (MAY), chain delegation and the JWT access token profile.
    - Its policy examples include per-request caps such as "allow add_to_cart if amount <= 50".
    - It does **not** define cumulative budgets, running totals, or how a resource server tracks spend across requests.
    - It does define AS-signed authorization evidence: what was displayed, how the user confirmed, an audit-trail reference, and signed delegation hops.
    — [datatracker](https://datatracker.ietf.org/doc/draft-liu-ai-agent-authorization-integration/)
- **Auth0 for AI Agents (Okta)**:
  - Features: user login for agents, Token Vault for third-party API tokens, async approval via OpenID CIBA with optional RAR so the consent prompt can show transaction details, fine-grained authorization, and MCP server authorization.
  - CIBA approval supports only Guardian push, with an email fallback.
  - Cross App Access (XAA) lets a tenant expose APIs and MCP servers to trusted external agents. XAA underlies MCP's Enterprise-Managed Authorization extension. Okta Integration Network partner integrations were expected from 24 Aug 2026.
  — [Auth0 async authorization docs](https://auth0.com/ai/docs/intro/asynchronous-authorization); [Auth0 XAA blog](https://auth0.com/blog/xaa-protocol-auth0-ai-agents/)
- **Microsoft Entra Agent ID**:
  - Agent identities are Entra accounts. Each agent has an accountable **sponsor**, and its access expires.
  - Blueprints template many agents.
  - Access packages cover both on-behalf-of and autonomous agents.
  - Third-party agents can join via an SDK sidecar or workload identity federation.
  - The registry is moving under Microsoft Agent 365.
  - Licensing differs across doc versions, and parts are in preview.
  — [Microsoft Learn: Agent ID governance](https://learn.microsoft.com/en-us/entra/id-governance/agent-id-governance-overview); [What's new in Agent ID](https://learn.microsoft.com/en-us/entra/agent-id/whats-new-agent-id)
- **Card-network and consumer agentic-commerce protocols**:
  - Mastercard Agent Pay sets rules once, at Agentic Token creation. Visa's Trusted Agent Protocol uses a fresh signed mandate per purchase. Source is a vendor blog.
  - AP2 mandates (Intent/Cart) cover amount limits, merchant categories, time window and single-use vs recurring. Sources conflict on AP2 governance: one says it moved to FIDO in 2026, others call it a Google launch from Sep 2025.
  - B2B coverage is thin. Mastercard is working with IBM watsonx Orchestrate on B2B. I found no source on how corporate approval hierarchies map onto mandates.
  — [eco.com comparison (vendor)](https://eco.com/support/en/articles/15192003-mastercard-agent-pay-vs-visa-trusted-agent-2026-compared); [RisingWave comparison](https://risingwave.com/blog/mastercard-agent-pay-vs-visa-vs-stripe-agentic-commerce/); [Digital Commerce 360](https://www.digitalcommerce360.com/2025/05/06/visa-mastercard-ai-agentic-commerce/)

### Inferences
- **What the standards cover:** agent identity and lifecycle (Entra, SPIFFE via the draft), delegation chains (RFC 8693, OBO drafts), one-off human consent (CIBA plus RAR in Auth0), token audience binding (MCP plus RFC 8707), and per-request amount caps expressed as policy (Liu draft, AP2 Intent mandate).
- **What they don't cover:** the concrete gaps for internal finance agents are below. Each is a cross-check of the controls in section 1 against the standards above. Each is an inference from what the sources do *not* contain, not a sourced statement that the gap exists.
  1. **Per-agent spending authority as a DoA:** cumulative, per-period, per-counterparty and per-cost-center limits tied to the human DoA of the sponsor. Scopes and per-request caps are not budgets (the Liu draft says it does not define running totals). A sponsor should not be able to grant authority above their own limit. No standard here expresses that ceiling.
  2. **Exception approval with dual control:** CIBA gives one user's consent. Finance needs threshold-based routing, two distinct approvers, approver-is-not-initiator checks, and expiry. That is the NetSuite/SAP model, not a consent screen.
  3. **Agent SoD:** no standard prevents one agent, or two agents under one sponsor, from both changing vendor bank details and releasing payment to that vendor.
  4. **Evidence an auditor can verify:** IdP logs and vendor audit trails sit in separate systems, owned by the vendor and mutable by admins. Apart from the individual Liu draft's signed evidence, nothing produces a tamper-evident, exportable record linking mandate, policy decision, approval and executed payment. COSO wants "audit evidence collection" but does not specify a format.
  5. **Reversal:** none of the identity or MCP standards define what happens after execution (recall, refund, void) or link a reversal to the original authorization. Card protocols lean on network disputes, and ACH, wire and RTP payouts have weaker or no recall.
  6. **Cross-system enforcement:** each vendor's limits stop at its own boundary (Ramp cards, BILL, NetSuite). An agent calling Stripe refunds, a bank API and the ERP needs one policy enforcement point, such as a gateway or credential broker, so that a single budget and approval state applies across all of them. MCP forbids token passthrough, which pushes toward that broker pattern.
  7. **Consumer protocols are the wrong shape:** AP2, Visa and Mastercard mandates model a consumer delegating a purchase to a shopping agent at a merchant. They do not model a company's DoA, multi-approver chains, vendor-master trust, non-card rails (ACH, wire, payouts) or month-end accounting effects.
- For AgentAuth positioning: local, self-hostable mandates, approvals and tamper-evident evidence line up with gaps 1–4 and 6. Per AGENTS.md, product claims must stay limited to what its verifier checks. Reversal (gap 5) depends on the rail, and no source here suggests anyone has solved it generally.

### Gaps
- I did not check the RFC 8693 text, SPIFFE docs, Okta's own "Okta for AI Agents" docs, or the IETF Transaction Tokens draft directly in this pass. They are named only through the Liu draft and the Auth0 blog.
- I could not decode the IETF interim slide PDF, so the 32-draft/5-cluster count rests on a search summary.
- I found no evidence of any standard or vendor offering cumulative per-agent budgets across systems, multi-party (four-eyes) approval for agent actions, or auditor-verifiable agent evidence as a shipped product. This is absence of evidence from a limited search, not proof that none exist.
- I found no source on audit firms' or PCAOB positions on evidence for agent-executed disbursements.
