# Cosigna: Visa and Mastercard opportunity review

28 October 2026. Review of the two Gemini screenshots and suggested partner routes. Public
announcements establish what vendors advertise, not independently tested capabilities or
interest in Cosigna. No partner has been contacted by this work.

## Assessment

The programs are real and worth investigating. The claim that the participants have no
deterministic authorization layer is contradicted by several primary sources. Cosigna should
approach them with a concrete workflow and integration question, rather than claiming to
provide a universally missing component.

## Checked claims

| Claim | Primary-source finding | Implication |
|---|---|---|
| Visa Agentic Ready expanded to APAC in April 2026 | Visa's 30 April announcement names more than 50 partners and describes phase one as issuer readiness. It explicitly includes tokens, identity, risk and controls. The participation route given is a Visa account executive. [Visa APAC announcement](https://www.visa.com.sg/about-visa/newsroom/press-releases/visa-launches-agentic-ready-program-in-asia-pacific-with-over-50-partners-advancing-agentic-commerce.html) | Real ecosystem signal. This is not an open funding cohort or evidence that Visa lacks controls. Public availability of startup intake today was not established. |
| Non-bank participants provide an obvious entry route | The Singapore announcement calls its 13 participants issuers, including fintechs. UQPAY itself describes policy-driven execution. [Visa Singapore announcement](https://www.visa.com.sg/about-visa/newsroom/press-releases/visa-launches-agentic-ready-programme-in-singapore-with-13-banks-and-fintech-partners.html) | A fintech name does not imply it is an agent platform, a buyer, or missing policy enforcement. StraitsX and UQPAY are exploratory leads, not qualified prospects. |
| Mastercard AP4M has 30-plus participants | The June announcement lists more than 30 participants and explicitly describes programmatic authorization rules and spending limits. [Mastercard launch](https://www.mastercard.com/us/en/news-and-trends/press/2026/june/mastercard-launches-agent-pay-for-machines.html) | There is overlap with Cosigna's basic controls. Avoid a comparison claiming Mastercard only handles identity/tokenization. |
| Mastercard is seeking a Commerce Logic Layer | The phrase comes from Nevermined's partner quotation in that same announcement. It is not a published vacancy, procurement request or statement that the role is unfilled. [Partner quotation](https://www.mastercard.com/us/en/news-and-trends/press/2026/june/mastercard-launches-agent-pay-for-machines.html) | Nevermined already occupies part of that positioning. Treat it as a potential collaborator or competitor requiring investigation. |
| AP4M is a practical developer entry point | Mastercard opened early access on 20 October, describing buyer/seller APIs, a toolkit, sandbox and documentation for agent purchases of digital services. [October update](https://www.mastercard.com/global/en/news-and-trends/press/2026/october/Mastercard-Agent-Pay-for-Machines-in-action.html) Its signup page explicitly says pilot, not commercial availability. [Early-access page](https://www.mastercard.com/global/en/business/payments/consumer-payments/next-gen-payments/digital-asset-solutions/cloudflare-connect-2026.html) | This is the more concrete near-term access request. Admission, pricing and compatibility with Cosigna are not established. Participation is not funding. |
| BILL/Billtrust's pilot has no business authorization | The 20 October announcement describes invoice-specific payment paths tied to buyer-approved instructions. Its FAQ says the agent executes eligible payments already approved by the buyer. [BILL announcement](https://www.bill.com/press-release/visa-bill-and-billtrust-launch-first-live-pilot) | It demonstrates demand for controlled execution and also an existing implementation. It does not establish the claimed authorization gap. |
| AWS supplies a runtime but lacks deterministic payment policy | AWS explicitly documents infrastructure-enforced session caps/expiry and Cedar-based tool authorization outside the model. [AWS guardrails explanation](https://aws.amazon.com/blogs/machine-learning/enable-safe-agentic-payments-with-built-in-guardrails-using-amazon-bedrock-agentcore-payments/) | Deterministic controls alone are not a defensible competitive claim. Compare workflow coverage and customer operating effort. |

The other exact startup names, dates and feature comparisons in the screenshots were not all
substantiated by this review. Do not copy the whole Gemini table into an investor deck as a
verified competitive analysis. A capability absent from a press release is not proven absent
from the vendor's product.

## Where Cosigna could fit

Our hypothesis is a self-hosted workflow that conditions a refund on a trusted business
record, applies exact approval rules, preserves uncertain outcomes for recovery, and supplies
evidence the customer can inspect independently. The value to test is the integration and
operating work saved by that package. The underlying primitives can be implemented by others.

That hypothesis fits the existing Stripe/Zendesk refund work more closely than AP4M's digital
service purchases and vouchers. Cosigna has no Visa or Mastercard connector today. Its external
payouts are not built, and it has not tested the networks' products. A new network adapter
would require actual access and a defined use case, not only a positioning change.

## Recommended sequence

1. Publish the reviewed refund benchmark with model errors, legitimate completion and its
   direct-tool baseline clearly identified. Keep the earlier incomplete attempts available.
2. Finish a short recording and find a business willing to supervise a sandbox refund pilot.
   Measure setup time, manual interventions and resolution work against its current approach.
3. Request AP4M early access to learn its authorization/settlement contract. Seek one technical
   discussion about integrating business-record checks and approval evidence. This is a
   recommendation for the founder, not an action already taken.
4. Explore Visa through an issuer/fintech introduction or account executive with a concrete
   use case. Do not treat an issuer readiness program as a generic SaaS accelerator.
5. Build a new connector only after access and partner requirements justify it. Keep the
   Antler pitch focused on the current product and use the programs as market context.

## Technical questions for an access discussion

- Where can an external business-policy check run before credentials or vouchers are used?
- How are task scope, payee, amount, expiry and revocation represented and enforced?
- Which events establish acceptance, settlement, failure or an uncertain result?
- What lookup/idempotency guarantees and windows can a gateway rely on?
- Can a business attach its approval/case references and export evidence?
- Are refunds supported, or is the pilot limited to purchases of metered digital services?

## Draft positioning for founder review

Cosigna is a self-hosted control gateway for AI-initiated financial workflows, starting with
support refunds. We currently exercise a Rust gateway with case checks, scoped agents,
exact-proposal approvals and Stripe test payments. We are exploring whether this workflow
can integrate with your agent-payment interfaces, and would like to understand the policy
hooks, payment-status contract and sandbox access requirements.

Do not claim Visa/Mastercard partnerships, endorsement, readiness certification, funding or
customer interest without direct evidence. Current Northwind tests compare with a direct
provider tool, not these programs or a competent rules wrapper. Stronger evidence would include
under-cap but wrong-case requests and measured customer integration/operating savings.
