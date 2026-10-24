# Adoption of AI agents that move money or touch financial systems (as of late October 2026)

Method note: about 17 web searches and fetches, run 2026-10-24. Many figures come from search-result summaries of primary pages. Where I fetched the page myself (OvationCXM, Ramp announcement), I say so. Vendor claims are labelled **[vendor claim]**. Results from secondary or competitor blogs are labelled **[secondary]**. Small, self-selected or vendor-sponsored samples are flagged.

## 1. What do 2025-2026 adoption surveys say?

### Takeaway
"Agent" adoption looks high in broad enterprise surveys: 50-80% of organizations say they are building, deploying or "adopting" agents. Finance-specific adoption is much lower, and nearly all of it is experimentation or assistive use. Almost no independent survey defines "agent" as software that executes payments or postings. The headline numbers therefore mostly measure copilots and pilots, not money-moving autonomy.

### Cited Findings
**Cross-functional enterprise surveys**
- **KPMG AI Quarterly Pulse, Q3 2026 (US, released early Oct 2026):**
  - 62% of organizations report building, deploying or developing AI agents, up from 53% the prior quarter.
  - Organizations actively developing or implementing multi-agent systems rose to 25%, from 6% two quarters earlier.
  - "Significant workforce adoption" rose to 44%, from 23%.
  - [KPMG Q3 2026](https://kpmg.com/us/en/media/news/q3-ai-pulse-2026.html)
  - KPMG's own quarterly figures don't match each other. The Q2 release says deployment was 53% vs 55%, and a Q1 blog says 54% (vs 12% in 2024). [KPMG Q2 2026](https://kpmg.com/us/en/media/news/q2-ai-pulse-2026.html); [KPMG Q1 blog](https://kpmg.com/us/en/media/blogs/2026/q1-ai-pulse-2.html)
  - KPMG's global Q1 2026 survey (20 countries): only 11% are "gaining edge" through agents that are scaled across functions and coordinated across workflows. [KPMG Global AI Pulse, Mar 2026](https://kpmg.com/xx/en/media/press-releases/2026/03/kpmg-global-ai-pulse-survey.html)
- **McKinsey State of AI 2025 (published 2025; the year counts as late 2025):**
  - 62% of respondents say their organizations are at least experimenting with AI agents.
  - 23% report scaling an agentic system somewhere in the enterprise.
  - In any single business function, no more than 10% say they are scaling agents. Agent use is most common in IT and knowledge management. I could not confirm a finance-specific figure or the sample size.
  - [McKinsey](https://www.mckinsey.com/capabilities/quantumblack/our-insights/the-state-of-ai)
- **PwC AI Agent Survey (May 2025, n=300 US senior execs):**
  - 79% say AI agents are already being adopted in their companies, and 88% plan to raise AI budgets because of agentic AI.
  - 68% say half or fewer of their employees interact with agents day to day.
  - PwC itself notes that broad adoption is not the same as deep transformation.
  - [PwC](https://www.pwc.com/us/en/tech-effect/ai-analytics/ai-agent-survey.html)
- **PwC 2025 Trust & Safety Outlook (>300 senior execs):** only 20% would trust AI agents with financial transactions, vs 38% for data analysis. This is a key data point for Cosigna's thesis. [Techstrong.ai summarizing PwC](https://techstrong.ai/features/ai-agents-arent-entirely-trusted-with-high-stake-use-cases-pwc-survey/)
- **Gartner webinar poll (Jan 2025, n=3,412 attendees; self-selected, older data):** 19% reported significant investment in agentic AI and 42% conservative investment. [Gartner PR, 25 Jun 2025](https://www.gartner.com/en/newsroom/press-releases/2025-06-25-gartner-predicts-over-40-percent-of-agentic-ai-projects-will-be-canceled-by-end-of-2027)

**Finance-specific surveys**
- **Deloitte CFO Signals Q4 2025 (North America):**
  - 54% of CFOs say integrating AI agents into finance is a top transformation priority for 2026. That was the top response, ahead of data quality at 52%.
  - 87% expect AI to be extremely or very important to finance operations in 2026.
  - [Deloitte Q4 2025 press release](https://www.deloitte.com/us/en/about/press-room/deloitte-q4-2025-cfo-signals-survey.html); [Deloitte 4Q 2025 Insights](https://www.deloitte.com/us/en/insights/topics/business-strategy-growth/4q-2025-cfo-signals-survey.html)
  - This measures priority, not deployment.
- **Deloitte Finance Trends 2026:** 48% of respondents in a "leading strategy role" say they have already deployed specific AI agents within finance. I did not obtain the sample or the definition of agent. [Deloitte via search summary; see CFO Dive coverage](https://www.cfodive.com/news/cfos-face-expanded-mandate-pressures-volatile-2026-deloitte-ai/816558/)
- **Deloitte CFO Signals Q2 2026 (n=200 North American CFOs, companies with ≥$1B revenue):**
  - Only 43% feel confident in their current AI governance, and 53.5% feel only somewhat confident.
  - 59% say their biggest governance challenge is balancing pressure to deploy fast against managing risk.
  - [Deloitte 2Q 2026](https://www.deloitte.com/us/en/insights/topics/business-strategy-growth/2q-2026-cfo-signals-survey.html)
- **Gartner:**
  - A June 2025 survey of 183 CFOs (presented June 2026) found 84% of finance organizations have implemented or plan to implement AI, but only 7% report high or very high impact. [Gartner Jun 2026](https://www.gartner.com/en/newsroom/press-releases/2026-06-08-gartner-says-cfos-need-structured-finance-ai-roadmaps)
  - A March 2026 survey of 204 finance leaders found 45% of finance AI investment leans toward productivity and 20% toward decision quality. [Gartner Jul 2026](https://www.gartner.com/en/newsroom/press-releases/2026-07-20-gartner-survey-shows-45-percent-of-cfos-say-their-ai-investments-lean-towwards-productivity-while-20-percent-say-these-investments-lean-towards-decision-quality); [CFO Dive](https://www.cfodive.com/news/finance-ai-spending-stuck-efficiency-gains-gartner-warns/825736/)
  - Nearly 60% of CFOs plan to raise finance AI investment by ≥10% in 2026. [Gartner Feb 2026](https://www.gartner.com/en/newsroom/press-releases/2026-02-10-gartner-research-reveals-cfos-budget-plans-prioritize-grotwth-functions-tech-and-ai-in-2026)
  - Gartner's often-cited "58% of finance functions use AI" is a **2024** figure. [Gartner 2024 PR](https://www.gartner.com/en/newsroom/press-releases/2024-09-12-gartner-predicts-that-90-percent-of-finance-functions-will-deploy-at-least-one-ai-enabled-tech-solution-by-2026)
  - I found no Gartner finance survey that measures agent execution of payments.
- **Wolters Kluwer / CCH Tagetik (21 May 2025, n=392 finance leaders polled at the vendor's own user conference; self-selected, vendor-run):**
  - 6% currently use agentic AI, and 38% plan to within 12 months.
  - Agentic AI is defined as systems that "act autonomously to perform tasks and make decisions."
  - [Wolters Kluwer PR](https://www.wolterskluwer.com/en/news/pr-2025-wolters-kluwer-survey-increasing-adoption-agentic-ai); [CPA Practice Advisor](https://www.cpapracticeadvisor.com/2025/05/28/finance-leaders-plan-to-increase-use-of-agentic-ai-in-next-year/161729/)
  - A widely repeated "14% have fully integrated AI agents into finance" figure attributed to Wolters Kluwer could **not** be verified. Don't use it.
- **AFP 2026 Treasury Benchmarking Survey (released 15 Sep 2026, >400 practitioners, sponsored by PNC):** AI/automation is among treasury's top-five priorities, and "managing AI opportunities and risks" is among its biggest challenges. Cash forecasting is still the top priority. [AFP PR](https://www.financialprofessionals.org/about/learn-more/press-releases/Details/afp-survey-ai-priorities-rise-across-treasury-teams-while-ai-related-challenges-grow)
- **AFP 2026 Payments Fraud and Control Survey (n=465):** the first edition to examine AI-enabled fraud and deepfakes, with rising concern about voice and video impersonation of executives and vendors. [AFP](https://www.financialprofessionals.org/training-resources/resources/survey-research-economic-data/details/payments-fraud)
- **OvationCXM (n=520 US finance leaders, companies from <$5M to $1B+ revenue; vendor-sponsored, page dated 29 Sep 2026; fetched directly):**
  - Nearly 75% use or pilot agents: ~30% in multiple workflows, 28% in one or more, and 17% piloting.
  - The tools they use are mostly general LLMs: ChatGPT 39%, Microsoft Copilot 36%, Excel/Sheets 32%, Gemini 28%. Only 13% use a bank portal and 12% a direct bank API.
  - Planned use in the next 12 months: ACH payments 28%, wire transfers 23%, scheduled disbursements 23%.
  - "Agent" is not defined.
  - [OvationCXM](https://www.ovationcxm.com/blog/ai-agents-in-banking-new-research-shows-agents-taking-action)

### Inferences
- Headline "agent adoption" (50-80%) mostly measures experimentation and LLM chat tools. The OvationCXM tool mix (ChatGPT and Copilot far ahead of bank APIs) shows that most "finance agent use" is not wired into money rails yet.
- Wolters Kluwer's 6% → 44% intent figure (2025) and Deloitte's 54% priority figure (2026) suggest finance is in the move from pilot to production during 2026. Investing in controls before scale fits that timing, though actual scaled execution is still a minority.

### Gaps
- No Bain, IDC, EY or IOFM agent-in-finance figures were retrieved in this pass.
- No Ramp, Brex or Mercury spend data on agent-initiated transactions was found.
- No Stripe or Adyen report quantifying agent-initiated payment volume was found.
- McKinsey's finance-function agent-scaling figure and sample size were not confirmed.
- Deloitte's Finance Trends 2026 sample and definitions were not obtained.

## 2. What share of finance teams let agents execute payments, refunds or postings without a human per transaction?

### Takeaway
Very few. The only survey I found that measures autonomy levels in money movement (OvationCXM, vendor-sponsored) puts fully automatic execution in single digits: 9% for approving or releasing third-party payments. About half would let an agent *initiate* a payment if a human approves it. Trust surveys agree: 20% of execs would trust agents with financial transactions (PwC 2025).

### Cited Findings
- **OvationCXM (n=520 US finance leaders; vendor-sponsored, fetched directly):**
  - Willing to give agents authority to at least *initiate* money movement:
    - payments, ACH, wires or transfers: 52%
    - moving money between company accounts: 51%
    - approving or releasing third-party payments: ~49%
  - **Fully automatic execution** is in single digits overall: 9% for approving or releasing a third-party payment, and 17% for the low-risk task of requesting a bank statement.
  - When human approval is part of the process, leaders are nearly as willing to let agents initiate transact-level tasks as read-only ones (50% vs 54%).
  - The survey explicitly did **not** ask whether agents independently log into banks and move money.
  - [OvationCXM](https://www.ovationcxm.com/blog/ai-agents-in-banking-new-research-shows-agents-taking-action)
- **Controls OvationCXM respondents need before they trust agents** (this list closely matches a control/audit layer):
  - role-based permissions and limits: 39%
  - ability to suspend or revoke access: 38.1%
  - real-time status visibility: 36.5%
  - confirmation and audit history: 35.6%
  - identity and authorization verification: 34.4%
  - [OvationCXM](https://www.ovationcxm.com/blog/ai-agents-in-banking-new-research-shows-agents-taking-action)
- **If their bank can't support agent workflows:** ~43% would find a workaround (22% build integrations in-house, 22% use a third-party platform), about a third would stay manual, and 10% would move business to an agent-capable provider. [OvationCXM](https://www.ovationcxm.com/blog/54-of-finance-leaders-using-ai-agents-would-seek-a-workaround-if-their-bank-cant-support-them)
- **PwC Trust & Safety Outlook 2025:** 20% would trust agents with financial transactions. [Techstrong.ai](https://techstrong.ai/features/ai-agents-arent-entirely-trusted-with-high-stake-use-cases-pwc-survey/)
- **Practitioner and vendor guidance [secondary]:**
  - Tiered models: autonomous below a dollar or risk threshold, human sign-off above it.
  - Recommendations that ERP postings always need human sign-off.
  - Advice to look for approval tiers rather than a single auto-approve toggle.
  - [Anrok](https://www.anrok.com/resources/governance-frameworks-for-agentic-ai-in-finance); [Safebooks](https://safebooks.ai/resources/agentic-finance/ai-agents-for-accounts-payable-the-controllers-guide-to-autonomous-ap/)
- **Stripe:** its ChatGPT checkout (Agentic Commerce Protocol, live since Sep 2025) uses Shared Payment Tokens, so the agent initiates a payment without seeing the card details. This is a consumer-side, buyer-authorized pattern. [secondary: Whitesight](https://newsletter.whitesight.net/p/agentic-finance-visa-and-stripes); [ATXP comparison](https://atxp.ai/blog/mastercard-agent-pay-stripe-acp-visa-x402-comparison-2026)

### Inferences
- The realistic present state:
  - agents prepare and initiate
  - humans approve each transaction or batch
  - full autonomy only for low-value or low-risk actions (card payments inside policy, small refunds)
- The ~50% "initiate if a human approves" group is the near-term market for approval routing, mandates and evidence. The single-digit "fully automatic" group is where unbounded-risk incidents would come from.

### Gaps
- No independent (non-vendor) survey measuring per-transaction autonomy for payments, refunds or ERP postings was found.
- OvationCXM's methodology (field dates, panel) is not published on the page.

## 3. Named, verifiable deployments where an AI agent initiates refunds, payouts, AP payments or ERP postings

### Takeaway
Real autonomous execution exists today mainly in three narrow places:
- card payments and coding inside AP platforms (Ramp, BILL)
- support-agent refunds through Stripe or Shopify connectors (Intercom Fin, Decagon, Gorgias-type tools)
- early consumer agentic-commerce rails (Stripe ACP, Visa, Mastercard)

ERP vendors (SAP, Oracle, Microsoft) ship finance agents, but the evidence I found shows them reconciling, flagging and routing, not autonomously releasing payments. Almost all deployment metrics are vendor claims.

### Cited Findings
**AP / spend platforms**
- **Ramp Agents for AP** (announced 7 Oct 2025; announcement fetched directly) **[vendor claim]**:
  - Line-item GL coding, fraud screening on 60+ signals, finding card-eligible bills and **entering card details in vendor payment portals**. For QuickBooks Online bills, "payment is automatically processed."
  - Approvals are *recommended*, not granted; decision-makers still approve.
  - Accuracy and outcomes: 85% of accounting fields coded right first time; 90% acceptance on 90,000 approval recommendations; $1M+ fraud flagged.
  - No spending caps or approval thresholds are described.
  - Availability: Early Access/waitlist, and some features only on Ramp Plus.
  - Named customers (Olipop, Furey) are quoted on AP in general, not on agents.
  - [Ramp blog](https://ramp.com/blog/ramp-ap-agents-announcement); [PR Newswire](https://www.prnewswire.com/news-releases/ramp-launches-agents-for-ap-to-automate-accounts-payable-302576975.html)
- **Ramp + Visa** agents for corporate bill pay (reportedly for Ramp Enterprise from Q1 2026): only a single third-party blog reports this, so it is unverified. [happycapyguide](https://happycapyguide.com/blog/visa-ramp-ai-corporate-bill-pay-agents-2026)
- **BILL** (FY Q4 2026 earnings call, late Aug 2026) **[vendor claim, investor materials]**:
  - Over 175,000 businesses use BILL agents across spend & expense (S&E) and AP.
  - W-9 agent: more than 40,000 organizations (tripled quarter over quarter); collected over 240,000 W-9s "with zero work from our customers," including IRS validation.
  - Invoice-coding agent: over 60,000 companies.
  - A payments-related agent completed over 30,000 card transactions in Q4. The name is garbled in the transcript as "pay 40", so verify.
  - Pricing is moving from per-seat to platform plus usage-based fees, with agents tiered by value.
  - [Motley Fool transcript](https://www.fool.com/earnings/call-transcripts/2026/08/26/bill-bill-q4-2026-earnings-call-transcript/); [Yahoo summary](https://finance.yahoo.com/technology/ai/articles/bill-q4-earnings-call-centers-140000985.html)
  - Note: most BILL agents named are data and compliance tasks (W-9, coding). Only the card-transaction agent moves money, and its approval model was not described.
- **Tipalti and Stampli:** no specifics on autonomous payment execution found in this pass. They are listed generically as shipping AP agents in 2025-26. [Safebooks](https://safebooks.ai/resources/agentic-finance/ai-agents-for-accounts-payable-the-controllers-guide-to-autonomous-ap/)

**ERP vendors [mostly secondary or competitor sources]**
- **SAP Joule:** reportedly 40+ specialized agents. A Cash Management Agent, said to be GA since Q1 2026, reconciles bank statements and flags anomalies. Joule Studio is reportedly GA in Q1 2026. [Savic](https://www.savictech.com/insights/sap-joule-agentic-ai-2026/); [Innobu](https://www.innobu.com/en/articles/sap-joule-2026-agentic-enterprise-ai.html)
- **Oracle Fusion:** ledger, payables, expenses and payments agents are reportedly GA in Release 26B and bundled into subscriptions. They are described as assistive, with approval and exception routing. The source is a competing vendor (ChatFin). [ChatFin](https://chatfin.ai/blog/oracle-fusion-ai-agents-finance-2026/)
- **Microsoft Dynamics 365 / Copilot for Finance:** reconciliation and variance analysis, and escalating exceptions to clerks. I found no reliable evidence of autonomous payment execution. [mybusinessfuture](https://mybusinessfuture.com/en/ai-agents-erp-sap-microsoft-oracle-autonomous-processes/)

**Payment networks (consumer agentic commerce)**
- **Visa:** in early 2026, said "hundreds" of agent-initiated transactions had been completed with partners. [Digital Transactions](https://www.digitaltransactions.net/?p=1331495)
- **Mastercard Agent Pay:** a Santander transaction on live bank infrastructure, and a first live agentic transaction in Hong Kong in April 2026. [secondary: stellagent](https://stellagent.ai/insights/visa-mastercard-agentic-commerce-strategy)
- No official 2026 volume figures from Stripe, Visa or Mastercard were found.

### Inferences
- Where agents already move money (Ramp card payments, support refunds), the control model is set by the vendor and lives inside one platform:
  - the platform's own policy engine
  - recommend-then-approve for anything significant
- A buyer running agents across several systems (ERP + bank + Stripe + support desk) has no cross-system mandate or evidence trail. That is the gap a neutral control layer would fill.
- Visa's "hundreds of transactions" in early 2026 shows agent-initiated network payments are still at pilot scale.

### Gaps
- No independently verified customer deployment, with the customer named, where an agent releases AP payments or posts journal entries without a human per transaction.
- No primary SAP, Oracle or Microsoft release notes were confirmed in this pass.
- Shopify agent tooling and Adyen were not researched in depth.

## 4. How common are support agents that can issue refunds autonomously, and with what caps?

### Takeaway
Refund execution is a standard, shipped capability in the leading AI support platforms (Intercom Fin, Decagon, Sierra, Gorgias-type tools). The controls are configurable policies plus optional human approval, not published default caps. The only concrete dollar cap I found is an example threshold in a smaller vendor (Fini: auto-refund under $50). I found no survey of how many deployments enable autonomous refunds.

### Cited Findings
- **Intercom Fin:**
  - Fin "Tasks" and "Procedures" can execute refunds by calling Shopify or Stripe through data connectors. On Stripe, the customer builds the connector.
  - A "Human in the Loop" step lets a teammate review and approve high-risk or high-value actions, refunds included. If the reviewer declines, the customer gets a decline message. With no reviewer named, it falls back to the default assignee.
  - [Intercom help: HITL approvals](https://www.intercom.com/help/en/articles/14468561-human-in-the-loop-approvals-for-fin-procedures); [Intercom: AI agents that take action](https://www.intercom.com/learning-center/ai-agents-that-take-action)
  - A third-party analysis reports that a timeout escalates rather than counting as approval, and that a Procedure ending in a handoff still bills as an outcome ($0.99). [drufball.com](https://www.drufball.com/notes/agentic-coding/intercom-fin-closure-metrics-2026)
  - Named customer: Nuuly uses Fin for subscription and refund requests, with no metrics published. [Intercom customers](https://www.intercom.com/customers)
- **Decagon [secondary/competitor sources]:**
  - Refund rules are written as natural-language procedures, e.g. "auto-process refunds within 30 days for customers with no prior refunds, escalate the rest."
  - Sensitive validation steps run in code rather than at the model's discretion.
  - It integrates with Stripe, Zendesk and Salesforce.
  - [getmacha](https://www.getmacha.com/blog/decagon-ai-complete-guide); [Decagon blog](https://decagon.ai/blog/ai-customer-service)
  - Named customers: Duolingo (~80% of tickets with no human, a deflection measure), Substack and Rippling. The figures are not refund-specific. [eesel](https://www.eesel.ai/blog/decagon-vs-sierra)
- **Sierra:**
  - Reportedly added PCI-compliant payments in April 2026, so agents can handle financial transactions directly.
  - Its WeightWatchers deployment handles ~70% of sessions **[vendor claim via competitor blogs]**.
  - [eesel](https://www.eesel.ai/blog/decagon-vs-sierra)
- **Gorgias:**
  - Sources conflict. One says its AI can cancel unfulfilled orders with a full refund; another says its order management cannot trigger automatic refunds.
  - A third-party playbook claims an "Autonomous Refunds" toggle for orders under a dollar threshold (e.g. <$50) when tracking shows return-to-sender. This is unverified in Gorgias docs.
  - [getmacha](https://www.getmacha.com/blog/gorgias-for-shopify); [stormy.ai](https://stormy.ai/blog/shopify-support-automation-playbook-2026)
- **Fini (smaller vendor):** native Shopify `refundCreate` writes, with configurable thresholds: auto-refund under $50, agent approval above. [Fini](https://www.usefini.com/guides/ai-chatbots-cancel-subscriptions-refunds-shopify-plus-gorgias)
- **Klarna** (the landmark case; 2024 data):
  - Its AI assistant handled ~75% of chats and did the work of ~700 agents, covering refunds, returns and payments.
  - In May 2025 the CEO told Bloomberg that Klarna was rehiring humans, because replacing them with AI "led to lower quality."
  - [Maginative](https://www.maginative.com/article/klarna-dials-back-its-ai-customer-service-strategy-now-its-hiring-humans-again/); [Entrepreneur](https://www.entrepreneur.com/business-news/klarna-ceo-reverses-course-by-hiring-more-humans-not-ai/491396)
  - I found no data on whether Klarna's bot executes refunds itself or only informs customers.
- **Consumer friction:** CNBC (1 Apr 2026) reported that the "consumer-AI refund relationship is off to a rocky start." The article returned 403, so its details are unverified. [CNBC](https://www.cnbc.com/2026/04/01/ai-chatbot-customer-service-complaints-refunds.html)

### Inferences
- Support refunds are probably the highest-volume autonomous money movement by AI agents today. They are low-value and frequent, and vendors bill per resolution.
- Each platform implements caps in its own way: policy text, code checks, HITL steps. Merchants running several tools therefore have no unified limit or audit trail.
- Refund abuse (prompt manipulation to trigger refunds) is a plausible risk, but I found no documented incident in this pass.

### Gaps
- No survey on the share of AI support deployments with autonomous refunds turned on.
- No published default caps from Intercom, Decagon or Sierra.
- No refund-volume metrics from named customers.

## 5. Where is adoption clearly NOT happening yet, and why?

### Takeaway
Autonomous execution is not happening at scale in these areas:
- high-value B2B payments (wires, ACH releases)
- treasury moves
- ERP journal postings and close sign-off

The stated blockers are trust, governance and risk controls, low realized value, and audit and accountability gaps, rather than model capability alone.

### Cited Findings
- **Gartner (June 2025):** over 40% of agentic AI projects will be canceled by end of 2027, due to escalating costs, unclear business value or **inadequate risk controls**. Gartner also estimates only ~130 of thousands of "agentic" vendors are genuine ("agent washing"). [Gartner PR](https://www.gartner.com/en/newsroom/press-releases/2025-06-25-gartner-predicts-over-40-percent-of-agentic-ai-projects-will-be-canceled-by-end-of-2027)
- **Trust:** only 20% of execs would trust agents with financial transactions. [PwC via Techstrong](https://techstrong.ai/features/ai-agents-arent-entirely-trusted-with-high-stake-use-cases-pwc-survey/)
- **Fully automatic third-party payment release:** 9%. Respondents' top requirements are limits, revocation, visibility, audit history and identity verification. [OvationCXM](https://www.ovationcxm.com/blog/ai-agents-in-banking-new-research-shows-agents-taking-action)
- **Governance confidence:** only 43% of $1B+ CFOs feel confident in their AI governance. [Deloitte 2Q 2026](https://www.deloitte.com/us/en/insights/topics/business-strategy-growth/2q-2026-cfo-signals-survey.html)
- **Value:** 7% of finance organizations report high impact from AI. [Gartner Jun 2026](https://www.gartner.com/en/newsroom/press-releases/2026-06-08-gartner-says-cfos-need-structured-finance-ai-roadmaps)
- **Scaling:** no more than 10% scale agents in any one function. [McKinsey](https://www.mckinsey.com/capabilities/quantumblack/our-insights/the-state-of-ai)
- **Treasury:** the 2026 AFP benchmarking ranks managing AI risks as a top challenge, and cash forecasting (analytics) is still the priority, not autonomous execution. [AFP](https://www.financialprofessionals.org/about/learn-more/press-releases/Details/afp-survey-ai-priorities-rise-across-treasury-teams-while-ai-related-challenges-grow)
- **Fraud environment:** AFP 2026 shows growing deepfake and impersonation fraud. That raises the bar for any agent that can release payments. [AFP Payments Fraud](https://www.financialprofessionals.org/training-resources/resources/survey-research-economic-data/details/payments-fraud)
- **ERP:** evidence shows finance agents reconcile, flag and route exceptions to humans. Practitioner guidance says ERP postings should keep human sign-off. [mybusinessfuture](https://mybusinessfuture.com/en/ai-agents-erp-sap-microsoft-oracle-autonomous-processes/); [Anrok](https://www.anrok.com/resources/governance-frameworks-for-agentic-ai-in-finance)
- **AP:** 72% of finance leaders see AP as the entry point for agentic AI, but 61% are "experimenting without direction." The source is a vendor blog and its primary survey is unknown. [Safebooks](https://safebooks.ai/resources/agentic-finance/ai-agents-for-accounts-payable-the-controllers-guide-to-autonomous-ap/)
- **Klarna:** its reversal (May 2025) is the best-known public case of an AI-first support strategy being dialled back on quality grounds. [Maginative](https://www.maginative.com/article/klarna-dials-back-its-ai-customer-service-strategy-now-its-hiring-humans-again/)
- **Agentic payments in Europe:** a DACH merchant survey (n=152; small sample) finds readiness lags, and liability and risk ownership are unresolved. [banking.vision](https://banking.vision/en/agentic-payments-readiness-report)

### Inferences
For Cosigna, the evidence points both ways.

**Against "urgent now":**
- Autonomous money movement by agents is still narrow (card AP payments, small refunds, pilot consumer checkouts).
- Most finance "agent" use is copilots.

**For "needed now":**
- About half of finance leaders would let agents initiate payments with human approval.
- The controls they ask for (limits, revocation, audit history, identity) are exactly a control/audit layer's features.
- Governance confidence is low, and Gartner names inadequate risk controls as a cancellation driver.
- Adoption intent is rising fast (KPMG 53% → 62% in one quarter, Deloitte 54% priority).

On balance, a human-in-the-loop approval-and-evidence layer sells into a present need. A fully autonomous-execution control layer sells into a 2027+ need.

### Gaps
- No public incident data (losses, erroneous agent payments or refunds) was found.
- No regulator guidance on agent-initiated payments (e.g. EBA, CFPB, PSD3) was researched in this pass.
- No auditor or Big Four guidance on SOX controls for agent-initiated postings was found.
