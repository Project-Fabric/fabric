# Cosigna: Antler India pitch plan

28 October 2026. Working plan for A Ashok kumar, currently building solo. The founder supplied
this name without expanding the initials. He described his role as co-founder and developer.
Use a solo-founder/developer description in the application and confirm the preferred title
before making the team slide. No other founder, employment history or full-time commitment
has been verified. This document is a plan, not a submitted application or finished slide deck.

## The application story

Cosigna gives companies control over refunds initiated by AI agents. An agent proposes a
refund. A Rust gateway checks its authority, budget and the customer's case record, routes
exceptions for approval, submits through a supported payment connector, and records the result.
The first external payment integration is Stripe test-mode refunds. The first buyer hypothesis
is an engineering team building support/refund agents for businesses using Zendesk and Stripe.

The funding objective is to turn a working sandbox prototype into customer-reviewed pilots
and determine whether customers will pay for the integration and operating work it saves.
Customer demand and a commercial advantage over a competent internal implementation remain
unproven. Voice ordering is a later application of this infrastructure.

## Antler India context

The official India page currently says the next Residency will be announced, while direct
conversations are available. It describes support for early AI founders and lists INR 4 crore
for 11% as its standard investment offer, subject to selection. It emphasizes full-time teams
and Bengaluru participation. This is not a confirmed offer to Cosigna. No exact next-cohort
deadline was established from the public page. [Antler India](https://www.antler.co/location/india)

The India page's Apply link redirects to the general Antler pitch form. That form lists
Idea to Pre-Seed and Bangalore choices. A separate company-pitch section displays an optional
deck upload with a 10 MB maximum. Check the actual chosen application route before export,
because the displayed sections do not establish identical requirements for every program.
Do not import Singapore's deck requirements into the India application.
[Official application form](https://www.antler.co/pitch)

## Ten-slide outline

Ten slides is our proposed structure, not an Antler requirement. Keep the financial evidence
legible and the main deck focused on one workflow. Place detailed attacks, failure cases and
architecture in an appendix or linked technical note.

| Slide | Proposed title | Content and evidence | Material still needed |
|---|---|---|---|
| 1 | Cosigna | Payment control for AI agents, starting with customer refunds. A Ashok kumar, solo founder/developer. | Founder-approved title, contact and company URL |
| 2 | Refund authority | A support agent must know which payment can be refunded, how much remains owed, and when a person must decide. Use a concrete synthetic ticket example. | One customer interview confirming this pain |
| 3 | The gateway | Show the agent, Cosigna and Stripe as distinct components. Explain that the model proposes while deterministic code decides and records. | A simple diagram for the actual deck |
| 4 | The working product | Case-bound requests, automatic/held/denied decisions, exact approvals and payment timelines. Show the real console rather than a design mockup. | Current console screenshot and recording |
| 5 | Engineering evidence | Present reviewed Stripe TEST comparisons, legitimate completion, runtime errors and verification results together. Separate Claude and Codex. | Complete review of the active Codex campaign |
| 6 | Existing alternatives | Explain the customer's choice among internal rules, bundled support automation and agent-payment platforms. Present Cosigna's packaged workflow as a hypothesis to validate. | A comparison against a competent rules wrapper |
| 7 | First customers | Target teams already building refund agents. Offer a supervised sandbox integration and measure setup/operating work. | Actual introductions, interviews and pilot interest |
| 8 | Business model | Test a software subscription priced around deployment and controlled-operation usage. Separate enterprise support from payment-provider fees. | Buyer feedback, price tests and bottom-up market estimate |
| 9 | Founder | Name, role, product contribution and verified relevant experience. Explain how a solo founder will cover engineering, customer discovery and security with funding. | Background, full-time status, hiring priorities |
| 10 | Funding milestones | A repeatable deployment, a recorded end-to-end demo, customer-reviewed pilots and independent security work before live funds. Explain why Antler can help reach these milestones. | Funding amount/runway budget and application deadline |

Do not label test payments as revenue or savings. Do not display Visa, Mastercard, Stripe,
Zendesk or Antler logos as customers, investors, integrations or partners beyond what is
actually built and authorized. Public ecosystem research belongs in a clearly labelled
market-context slide or appendix.

## Benchmark slide rules

The earlier Claude Stripe test run has two complete paired rounds, each with USD 528 wrong
refunds in the direct-tool world and USD 0 through Cosigna. Both paid all five legitimate
refunds. Round 3's guarded R9/O1 sessions hit a usage limit and remain incomplete.

The first Codex attempt completed the unguarded world with USD 1,168 wrong refunds. Its
guarded side paid the five legitimate refunds and recorded no movement for the tested
over-budget/injection requests before stopping on an unexpected tool request at R9. O1 was
not attempted. This is an incomplete campaign and belongs in the appendix, not a claim of
a fully successful paired round. The specific tool name was not retained by that initial
adapter. A later fixture-only R9 diagnostic completed, which does not explain or erase the
original error.

A second Codex attempt retained the actual diagnostic: the adapter mistook Codex's internal
`list_mcp_resources` metadata discovery for an unauthorized financial tool. That guarded run
also stopped before O1. Its saved provider records show five correct refunds totaling USD
182.98, and its retained evidence passed independent verification (21 records, 7 operations).
This was a benchmark-adapter classification error, not a demonstrated unauthorized payment.
The classifier now counts the two read-only metadata-listing helpers separately while still
rejecting unknown financial tools, resource reads and shell execution. Retain both interrupted
attempts and explain this correction alongside any later complete campaign.

The corrected Codex campaign completed one full paired repeat: USD 1,168.00 wrong refunds
through the direct tool versus USD 0.00 through Cosigna, with all five legitimate refunds paid.
All 20 sessions completed; both Rust and independent Python evidence checks passed. The
second repeat stopped on a CLI runtime failure after the baseline's USD 450 refund had already
committed. Its provider snapshot is retained. One remaining planned pair is queued for fresh
payments and awaits private key entry. Do not call this three complete Codex rounds.
See [the benchmark review](BENCHMARK_REVIEW_2026-10-28.md), including all interrupted attempts.

For each runtime, show incorrect refund amounts, missed legitimate refunds, completed/error
sessions, number of repeats and the verification boundary. Explain that the direct-tool
baseline has no business budget in its prompt. These experiments do not compare Cosigna to
Visa, Mastercard, AWS or a competent in-house rules wrapper. They demonstrate enforcement
against specified agent requests, not a general model-safety guarantee.

## A two-minute product demonstration

1. Show a synthetic support ticket and an agent with a scoped refund grant.
2. Let a valid request complete through Cosigna and show the Stripe test refund.
3. Show an excessive or unauthorized request and the gateway's recorded decision.
4. Show a separate held request, its alert and approval of the exact proposal by a named person.
5. Show the resulting payment timeline and evidence verification.

Label Stripe TEST mode visibly. Human approval is a separate explicit action. The existing
held USD 30 request is not approved by this plan. Record on fresh synthetic data when the
workflow is ready rather than silently replaying a previously started operation.

## Funding milestones to discuss

- First 30 days: finish the recorded flow, browser sign-in and onboarding/recovery instructions.
- Next 30 days: run customer-supervised sandbox evaluations and measure integration effort,
  manual interventions and unresolved outcomes against the customer's current process.
- Next 30 days: obtain concrete pilot feedback and test willingness to pay. Choose additional
  integrations from observed requirements. Commission outside security work before live funds.

These are proposed goals, not commitments from customers or guaranteed outcomes. Build the
raise/runway calculation from actual hiring, hosting and review costs. A cheap model test is
not a company operating budget.

## Founder inputs still required

- Preferred public title, short factual biography and LinkedIn/contact details.
- Full-time status and Bengaluru availability.
- Application deadline and the exact Antler application route being used.
- Any actual customer conversations, pilots, revenue, prior funding or incorporation details.
- Funding target, planned hires and monthly operating budget.

No application has been submitted and no outreach message has been sent by this work.
