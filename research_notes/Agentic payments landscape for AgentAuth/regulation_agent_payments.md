# Regulation of AI-agent-initiated payments (as of 21 October 2026)

Method note: about 18 tool calls. Primary regulator documents I read directly: FCA Payments Regulatory Priorities (March 2026), the MAS SAFR media release (3 July 2026), and the CBA/Davis Wright Tremaine white paper (January 2026, full text). Law-firm items are marked **[interpretation]**. Items marked **[statutory text, from knowledge]** restate well-established provisions of the PSD2, the SCA RTS, the AI Act, Reg E and Reg Z. I did not re-fetch the eur-lex text for them in this session, so the report writer should treat their article numbers as reliable but unverified here.

---

## 1. EU: PSD2, the SCA RTS, PSD3/PSR and the EBA on agent-initiated payments

### Takeaway
No EU payments law, and no EBA opinion or Q&A, deals specifically with AI agents as of October 2026. Agent-initiated payments fall under PSD2 and the SCA RTS (Delegated Regulation 2018/389) with no special regime. The legal question is whether the user's consent and SCA cover the specific payee, amount and timing. PSD3/PSR was politically agreed in November 2025 and the final texts were published in April 2026, but neither text has an agent-specific provision. It formalises delegated SCA as outsourcing and would not apply until about 2027-2028.

### Cited findings
**PSD2 / RTS baseline [statutory text, from knowledge]**: [PSD2, Directive 2015/2366](https://eur-lex.europa.eu/eli/dir/2015/2366/oj); [RTS SCA, Delegated Regulation 2018/389](https://eur-lex.europa.eu/eli/reg_del/2018/389/oj)
- PSD2 Art. 64: a payment transaction is authorised only if the payer has consented to it in the form agreed with the PSP. Art. 97: the PSP must apply SCA when the payer accesses the account online, initiates an electronic payment, or does anything remotely that implies a risk of fraud. For remote electronic payments, Art. 97(2) adds dynamic linking.
- RTS Art. 5 (dynamic linking): the payer must be made aware of the amount and the payee. The authentication code must be specific to that amount and payee. Any change to either invalidates the code.
- RTS exemptions: Art. 13 trusted beneficiaries (the payer sets up the list with SCA, after which payments to listed payees can be exempt). Art. 14 recurring transactions (same amount and same payee, with SCA on the first one). Art. 15 transfers between accounts held by the same person. Art. 16 low-value remote payments (≤ €30, with SCA required once cumulative spend exceeds €100 or after 5 consecutive transactions). Art. 17 secure corporate payment processes and protocols that are not available to consumers, if the authority is satisfied they give equivalent security. Art. 18 transaction risk analysis, with thresholds of €100/€250/€500 tied to the PSP's fraud rates.
- Merchant-initiated transactions (MITs) are treated as payee-initiated and outside SCA, provided the mandate was set up with SCA. This position comes from the EBA's 2018 SCA Opinion and its Q&As. **Verify**: I did not re-fetch the specific EBA Q&A ID in this session.
- Liability under PSD2 Art. 73-74: the payer's PSP must refund unauthorised transactions. The payer bears up to €50 unless they acted fraudulently or with gross negligence. If the payer's PSP did not require SCA, the payer bears no loss except in cases of fraud (Art. 74(2)).

**Agentic application (commentary)**
- Agent-based payment models "remain subject to PSD2 and the RTS requirements concerning Strong Customer Authentication (SCA)", and "there is currently no special 'extra-regulatory' regime in this area". The open questions are who provides the payment service, who controls the funds, and "what constitutes valid authorisation and meets the SCA requirements". **[interpretation]** — [Osborne Clarke, 6 Mar 2026](https://www.osborneclarke.com/insights/agentic-payments-new-challenge-europes-payments-ecosystem)
- Osborne Clarke expects disputes to turn on "whether the user authorised all elements of the transaction (the specific payee, amount and timing)" and whether that can be shown in an auditable way. It recommends audit trails that record who granted consent, what the mandate covered, the limits, whether step-up/SCA was triggered, and what the agent did. Its proposed controls are agent-instance authentication, monetary and frequency limits, payee allow/deny lists, and step-up conditions. It also notes that current EU implementations are pilots, with production deployments targeted for 2026. **[interpretation]** — [Osborne Clarke](https://www.osborneclarke.com/insights/agentic-payments-new-challenge-europes-payments-ecosystem)
- SCA ties authentication to a human, and delegation to an agent does not fit that model. No EBA position has been published, and later guidance "cannot be ruled out". **[interpretation, secondary]** — [wiki.private.law](https://wiki.private.law/en/agentic-payments.md); [Osborne Clarke (PL)](https://www.osborneclarke.com/pl/insights/agentic-payments-nowe-wyzwanie-dla-europejskiego-ekosystemu-platniczego)
- An AI agent cannot itself supply some SCA elements (knowledge, possession, inherence), and the existing exemptions give "only limited alternatives". The authors float time- and scope-limited agent tokens tied to the user, or new SCA standards for delegated agent authority. **[interpretation, written on UK PSRs, which mirror PSD2]** — [Pinsent Masons, 8 Jul 2026](https://www.pinsentmasons.com/out-law/analysis/what-new-rules-agentic-ai-payments-look-like)

**PSD3 / PSR status**
- Provisional political agreement on 27 Nov 2025. On 23 Apr 2026 the Council published the agreed final texts. Formal adoption and Official Journal publication are expected mid-2026 or in H2 2026. — [William Fry](https://www.williamfry.com/knowledge/psr-psd3-eu-payment-services-legislation-is-agreed/); [Arthur Cox](https://www.arthurcox.com/insights/psd3-and-psr-final-compromise-texts-published/); [Matheson](https://www.matheson.com/insights/psd3-and-psr-published-whats-changing-for-eu-payment-services/)
- Reported application: the main PSR obligations (SCA, fraud, open banking) from late 2027, and the verification-of-payee articles (50, 57) 27 months after entry into force. Sources differ on the exact dates. — [Linklaters](https://techinsights.linklaters.com/post/102lw95/psd3-breakthrough-eu-legislators-agree-payments-regulation-reforms); [PwC CZ](https://www.pwc.com/cz/cs/blog/rizeni-rizik/psd-3-the-psr-payments-reform.html)
- The PSR moves the conduct rules, including SCA, into a directly applicable regulation. It reportedly extends SCA to cover logins, beneficiary-list changes, mandate setup and device recovery. — [Dudkowiak](https://www.dudkowiak.com/blog/psd3-and-psr-final-texts-agreed-new-obligations-for-payment-institutions/)
- The PSR explicitly allows a PSP to delegate SCA to a third party (a wallet, gateway or trust service provider). This is classified as outsourcing, which brings in the EBA outsourcing guidelines and DORA. The PSP remains liable. Sources cite the article as 85 or 87, and one source allocates liability differently. — [Norton Rose Fulbright](https://www.nortonrosefulbright.com/en/knowledge/publications/cedd39c6/psd3-and-psr-from-provisional-agreement-to-2026-readiness); [Corbado](https://www.corbado.com/blog/delegated-sca-psd3-passkeys); [KPMG Law](https://kpmg-law.de/en/psd3-and-psr-new-payment-regulation-for-payment-service-providers-and-banks/)
- I found no source saying the PSD3/PSR texts contain AI-agent-specific provisions. One commentary predicts that agentic rules may enter EU law through PSD3/PSR around 2028. That is a forecast, not an official timeline. — [wiki.private.law](https://wiki.private.law/en/agentic-payments.md)

### Inferences
- Under current rules, the most defensible EU pattern is to do SCA at setup or mandate creation and then run agent payments inside a pre-authorised scope: an MIT or recurring mandate, a trusted-beneficiary list, or low-value limits. For B2B, the Art. 17 corporate exemption may apply, but only if the competent authority accepts it. Every exemption is narrow, and none was designed with agents in mind.
- Dynamic linking (amount and payee) means an open-ended "spend as needed" grant does not satisfy SCA for each remote payment. Authorisation has to be specific, or the payment has to fit an exemption.
- Under Art. 74(2), a PSP that skips SCA bears the loss (absent payer fraud). That gives PSPs an incentive to demand mandate evidence from agent platforms.

### Gaps
- I did not retrieve the adopted PSR text, so I could not confirm whether it says anything about automated or delegated initiation beyond delegated SCA. I also could not confirm the article numbers or the Official Journal publication date.
- I found no EBA opinion, Q&A or speech on agentic payments. The exact MIT Q&A ID is unverified in this session.

---

## 2. United States: Reg E, Reg Z, UCC 4A, CFPB, money transmission

### Takeaway
The US has no rule, interpretation or CFPB statement on agentic payments as of October 2026. Whether a payment counts as "authorized" depends on Reg E's definition of an unauthorized EFT, including its exclusion for a person the consumer furnished with an access device, and on Reg Z's "actual, implied, or apparent authority". Both may leave the consumer bearing losses from an agent's mistakes, though the experts disagree on how settled this is. Whether a platform needs a state money-transmission licence turns on whether it controls funds.

### Cited findings
- Reg E caps consumer liability for unauthorised EFTs at $50 if reported within 2 days of discovery and $500 if reported within 60 days, with no cap after that (12 CFR 1005.6). The institution bears the burden of proving authorisation (15 U.S.C. 1693g(b)). A transfer by someone the consumer gave an access device to, who then exceeded their authority, falls outside the definition of "unauthorized" (12 CFR 1005.2(m)) unless the consumer has notified the institution. The CBA asks: "If a consumer authorizes an AI agent to access the consumer's bank account and initiate transactions, has the consumer 'provided' an access device that triggers the exception? What if the agent exceeds its authorization? What if the agent is compromised...?" It calls applicability to agentic AI "unclear, at best" and warns that "consumers may be liable for mistakes their agents make." — [CBA / Davis Wright Tremaine white paper, Jan 2026](https://consumerbankers.com/wp-content/uploads/2026/01/CBA-Agentic-Symposium-White-Paper-2026-01v2.pdf)
- Reg Z limits cardholder liability to the lesser of $50 or the amount charged (12 CFR 1026.12(b)). Use by someone with "actual, implied, or apparent authority" is not unauthorised, and the burden of proving authorisation is on the issuer (15 U.S.C. 1643(b)). It is "at best, unclear" whether authorising an agent creates such authority. — [CBA white paper](https://consumerbankers.com/wp-content/uploads/2026/01/CBA-Agentic-Symposium-White-Paper-2026-01v2.pdf)
- Contrary view: "a transfer is presumed authorized if a consumer furnishes credentials to authorize an agent to make payments", even when the agent goes beyond what the consumer intended, "unless the financial institution was notified of a revocation or breached its own security procedures." Goodwin calls this "largely settled" and says the real disputes are between users and platforms. On Reg Z, it says apparent authority could cover an agent acting beyond its actual authority, and that "not accepted / not delivered as agreed" claims usually push the loss onto merchants. Under UCC 4A (commercial wires and orders), a payment order binds the customer if the bank followed an agreed, commercially reasonable security procedure in good faith. **[interpretation]** — [Goodwin, 11 Jun 2026](https://www.goodwinlaw.com/en/insights/publications/2026/06/insights-technology-aiml-authorizing-agentic-payments)
- Fenwick, by contrast, says it is "unresolved" whether giving an agent account access or credentials satisfies Reg E's authorisation requirement, and notes that Reg E covers deposit accounts but not crypto-native rails. **[interpretation]** — [Fenwick, 22 Apr 2026](https://www.fenwick.com/insights/publications/is-2026-the-year-of-agentic-payments). Astraea says "No rule resolves this yet." **[interpretation]** — [Astraea Counsel](https://astraea.law/insights/know-your-agent-kya-compliance-standard)
- Card network rules: Visa requires agentic payment providers to register and to keep a "defensible chain of authorization". Complying with network rules "does not preclude customer claims against platforms". **[interpretation]** — [Goodwin](https://www.goodwinlaw.com/en/insights/publications/2026/06/insights-technology-aiml-authorizing-agentic-payments); see also [FBT Gibbons](https://fbtgibbons.com/the-payment-infrastructure-layer-how-network-rules-already-govern-agentic-commerce/)
- CFPB: I found no 2026 CFPB statement, circular or rule on agentic payments. The CFPB's Jan 2025 proposed interpretive rule extending EFTA/Reg E to emerging digital payment mechanisms was rescinded on 15 May 2025. Some AI-related guidance was withdrawn in 2025. — [Troutman (regulatoryoversight.com), Jan 2026](https://www.regulatoryoversight.com/2026/01/how-payments-law-landscape-will-evolve-in-2026/); [Riddle Compliance](https://riddlecompliance.com/cfpb-compliance-2026-enforcement-changes/) (secondary). As of January 2026 the CFPB was still revising the Section 1033 open-banking rules. — [CBA white paper](https://consumerbankers.com/wp-content/uploads/2026/01/CBA-Agentic-Symposium-White-Paper-2026-01v2.pdf)
- Money transmission: the trigger is control of customer funds ("do you control customer funds?"). A non-custodial agent that only instructs the user's own bank may fall outside the definition. Agent-of-payee exemptions vary by state and are "narrow and fact-specific". FinCEN MSB registration (31 CFR 1022.380) and a BSA/AML programme run in parallel with state licensing. The GENIUS Act adds a federal regime for stablecoin rails but does not remove state licensing. **[interpretation]** — [Astraea, 1 Jul 2026](https://astraea.law/insights/agentic-payments-money-transmitter-license). Platforms that hold wallet keys, can pause or override disbursements, or pool funds face more scrutiny. Fenwick frames the key question as "whether the platform underlying that system has retained meaningful authority over payment flows". **[interpretation]** — [Fenwick](https://www.fenwick.com/insights/publications/is-2026-the-year-of-agentic-payments)
- State and federal AI policy: a December 2025 executive order and a March 2026 federal policy framework aim to prevent a "fragmented patchwork" of state AI laws. None of these targets agentic payments specifically. — [Fenwick](https://www.fenwick.com/insights/publications/is-2026-the-year-of-agentic-payments)

### Inferences
- Firms are treating consumer Reg E/Reg Z exposure as unsettled. The two leading firm views conflict (Goodwin says "largely settled", the CBA says "unclear at best"). Mandate records of scope, limits and revocation are central evidence under either reading, because the access-device exclusion stops applying once the consumer notifies the institution, and both views turn on the scope of authority.
- Payouts and vendor payments that a company makes from its own business accounts are commercial transfers. They fall under UCC 4A and the bank agreement's security procedures, not Reg E. Agent misuse there is mainly the customer's risk if the bank followed the agreed procedure.

### Gaps
- No CFPB UDAAP statement specific to agentic payments was found. The CFPB chatbot research (2023) is not specific to payments, and I did not re-fetch it.
- The state-by-state agent-of-payee positions and the status of MTMA adoption were not researched.

---

## 3. EU AI Act: obligations relevant to agents in financial processes

### Takeaway
An AI agent that executes payments is not, as such, a listed high-risk use. Annex III point 5(b) covers creditworthiness and credit scoring of natural persons, with fraud detection excluded. That means Arts. 12, 14 and 26 bind payment agents only where they fall within Annex III (for example, credit decisions). Under the provisional Digital Omnibus deal, the Annex III obligations move from 2 Aug 2026 to 2 Dec 2027. That deal binds only once it is published in the Official Journal, which I could not confirm.

### Cited findings
- **[statutory text, from knowledge]** [Regulation (EU) 2024/1689](https://eur-lex.europa.eu/eli/reg/2024/1689/oj):
  - Art. 12: high-risk systems must technically allow automatic event logging over their lifetime.
  - Art. 14: high-risk systems must be designed for effective human oversight, including the ability to interpret output, override it, and interrupt the system ("stop" button).
  - Art. 26 (deployers): use the system per its instructions, assign competent human oversight, monitor it, keep the automatically generated logs for at least 6 months, and inform workers. Under Art. 26(5), financial institutions can meet monitoring duties through their internal-governance rules under financial services law.
  - Art. 50: transparency duties.
  - Art. 4: AI literacy.
- Already applicable: the Art. 5 prohibitions and the Art. 4 AI literacy duty since 2 Feb 2025, and the GPAI obligations since 2 Aug 2025. — [Gibson Dunn, 27 May 2026](https://www.gibsondunn.com/eu-ai-act-omnibus-agreement-postponed-high-risk-deadlines-and-other-key-changes/)
- The Digital Omnibus on AI was provisionally agreed on 6-7 May 2026 and confirmed by Council representatives on 13 May. It moves Annex III high-risk systems (including credit scoring) to 2 Dec 2027 and Annex I systems to 2 Aug 2028. The new dates bind only once the Omnibus is published in the Official Journal. — [Gibson Dunn](https://www.gibsondunn.com/eu-ai-act-omnibus-agreement-postponed-high-risk-deadlines-and-other-key-changes/); [Sidley, 22 Jun 2026](https://datamatters.sidley.com/2026/06/22/eu-lawmakers-reach-provisional-agreement-to-delay-key-eu-ai-act-obligations/). Parliament reportedly voted to adopt on 16 Jun 2026, with Council approval still pending at that time. — [quality.org](https://www.quality.org/article/eu-reaches-provisional-digital-omnibus-deal-reshaping-ai-act-timeline-and-transparency). Sources conflict on whether the Art. 50(2) watermarking duty moves to 2 Dec 2026.

### Inferences
- For most agentic payment use cases (refunds, payouts, AP), the AI Act adds relatively little legal obligation in 2026 beyond AI literacy and transparency. Arts. 12 and 14 still describe controls that payments regulators (FCA, MAS) are asking for anyway.

### Gaps
- I could not confirm whether the Omnibus was published in the Official Journal before 2 Aug 2026.
- I found no guidance classifying payment-execution agents under the AI Act.

---

## 4. SOX / ITGC, PCAOB AS 2201 and Big 4 views

### Takeaway
There is no SOX or PCAOB rule specific to AI agents. Companies extend their existing COSO 2013 and ITGC domains (access, change management, segregation of duties) to cover agent identities. I found no primary Big 4 publication on SoD for agents that execute payments. Most of the material found is vendor commentary.

### Cited findings
- PCAOB AS 2201 governs the integrated audit of ICFR. Paragraphs .16-.19 cover using the work of others. — [PCAOB AS 2201](https://pcaobus.org/oversight/standards/auditing-standards/details/AS2201) (paragraph reference via secondary source [onbex blog](https://agentmarketcap-1.onbex.co/blog/2026/07/11/ai-agents-big-four-audit-assurance))
- No standalone "SOX for AI" exists. Companies map agent activity onto COSO 2013 and SEC requirements. — [Concourse](https://www.concourse.ai/insights/sox-compliance) (vendor)
- Agent identities should follow the same lifecycle controls as human ones: request, approval, recertification and deprovisioning. — [Sofy](https://sofy.ai/blog/sox-testing-checklist-ai-agents-erp/) (vendor)
- One agent holding a persistent service credential can perform every step of a transaction unless the architecture prevents it, which breaks role-based SoD assumptions. — [CloudEagle](https://www.cloudeagle.ai/blogs/segregation-of-duties-ai-agents); [Lespricore](https://www.lespricore.com/blog/ai-agents-internal-controls) (vendors)
- KPMG's SOX webcast series covers AI and agentic AI as tools for continuous control monitoring. — [KPMG Future of SOX](https://kpmg.com/us/en/insights-by-topic/future-of-sox-insights.html). Deloitte says GenAI in SOX should operate "with rigorous oversight from internal control and reporting professionals". — [Deloitte](https://www.deloitte.com/us/en/services/audit-assurance/blogs/accounting-finance/ai-finance-reporting-automation-public-companies.html)

### Inferences
- An agent that both proposes and executes a payment, or that changes vendor master data and then pays, is the classic SoD conflict. Independent human or second-system approval and immutable logs map directly onto existing ITGC expectations.

### Gaps
- I found no primary Big 4 or PCAOB publication on AI agents executing financial transactions. The claims that Deloitte Omnia added agents and about KPMG's 2028 timeline come from unverified blogs and are excluded.

---

## 5. UK FCA, Bank of England, MAS, RBI/India

### Takeaway
The UK has moved furthest towards legislating. The FCA (March 2026) committed to consider whether regulation needs to change for agentic AI payments. HM Treasury's consultation (14 Jul to 6 Oct 2026) puts the questions of mandate, consent, SCA and liability on the table but proposes no detailed rules. MAS published the industry SAFR white paper (July 2026) and finalised its AI risk guidelines (Oct 2026), with agentic-specific consultation planned for 2027. I found no RBI guidance specific to agentic payments.

### Cited findings
- FCA: "Work with the Treasury to modernise and future-proof the regulation of payment services and electronic money... This will include considering whether change or development of regulation is needed to support agentic AI payments." — [FCA Payments Regulatory Priorities, March 2026](https://www.fca.org.uk/publication/regulatory-priorities/payments-report.pdf) (primary, text extracted)
- FCA CEO Nikhil Rathi said accountability "must remain clear" where agentic systems are deployed. Bank of England Deputy Governor Sarah Breeden: "Our frameworks were not built to contemplate autonomous agents" and "relying on a human in the loop for all agent actions is unlikely to be realistic." The Mills review recommends an "agentic supervisory model" and an FCA perimeter review. — [Pinsent Masons, 8 Jul 2026](https://www.pinsentmasons.com/out-law/analysis/what-new-rules-agentic-ai-payments-look-like)
- HM Treasury's consultation (published 14 Jul 2026, closed 6 Oct 2026) covers AI agents that initiate payments and the scope and duration of consent, revocation, when an automated transaction is authorised, how SCA applies, and liability for faulty code or an agent acting outside its mandate. HMT "does not yet propose detailed rules". Skadden expects the outcome to stress mandates, transaction parameters, audit trails, revocation and allocation of responsibility. **[Skadden interpretation]** — [Skadden, Jul 2026](https://www.skadden.com/insights/publications/2026/07/hm-treasury-proposes-major-overhaul)
- UK PSRs 2017 reg 67 requires the payer's consent to each payment transaction, and an agent that decides the transaction strains this model. **[interpretation]** — [bratby.law](https://bratby.law/agentic-ai-payments-consent-regulation/) (snippet only, page returned 403)
- On 19 Oct 2026 the FCA published AI good and poor practice examples covering consumer outcomes, accountability and resilience. These are not static guidance. — [The Paypers](https://thepaypers.com/regulations/news/fca-publishes-ai-good-and-poor-practice-for-financial-firms) (secondary, not fetched)
- MAS SAFR (3 Jul 2026) is an industry white paper under BuildFin.ai. Its payments use case is "agent-assisted payments and treasury operations" in which agents "execute routine transactions within predefined mandates". Its safeguards are policy-bound execution, governance checkpoints that "verifies and records an AI agent's proposed actions before the execution", real-time validation and auditability. It is not described as binding. — [MAS media release](https://www.mas.gov.sg/news/media-releases/2026/mas-partners-industry-to-develop-safeguards-for-ai-agents-in-finance)
- MAS Guidelines on AI Risk Management were issued on 7 Oct 2026. Sections 3-4 apply from 7 Oct 2027 and sections 5-6 from 7 Oct 2028. Further agentic-AI consultation is planned for 2027. — [MAS](https://www.mas.gov.sg/news/media-releases/2026/mas-sets-out-supervisory-expectations-on-responsible-ai-adoption-by-financial-institutions); [LeapRate](https://www.leaprate.com/regulation/mas-finalises-ai-risk-guidelines-for-financial-firms/); [Blockhead](https://www.blockhead.co/2026/10/08/mas-finalises-ai-risk-rules-for-financial-institutions-with-agentic-ai-guidance-to-follow-in-2027/). A parliamentary reply on 5 Aug 2026 reportedly placed autonomous agents within MAS's supervisory guidelines (unverified secondary source). — [TechTimes](https://www.techtimes.com/articles/323283/20260806/mas-confirms-agentic-ai-inside-binding-bank-rules-us-eu-fall-behind.htm)
- India: RBI's FREE-AI framework centres on accountability, fairness, resilience and trust. RBI model-risk guidance is in draft. NPCI is examining safeguards for AI agents on UPI. CERT-In has proposed human-in-the-loop controls for agentic actions above financial thresholds. — [MediaNama, Sep 2026](https://www.medianama.com/2026/09/223-singapore-safr-agentic-ai-payments-india-gff/) (secondary)

### Inferences
- Across the UK, Singapore and the EU commentary, the vocabulary is converging: mandates, parameters or limits, pre-execution checks, audit records, revocation, and allocation of accountability. None of it is binding law yet.

### Gaps
- The FCA's 19 Oct 2026 publication and the HMT consultation document were not read in primary form. I found no primary RBI or NPCI documents.

---

## 6. Liability: who bears losses for unauthorised or mistaken agent payments

### Takeaway
No jurisdiction has allocated liability for agent errors specifically. The default rules are as follows:
- **EU/UK:** the PSP refunds unauthorised transactions unless the payer was grossly negligent or fraudulent. Whether the payment was authorised turns on the scope of consent.
- **US consumer:** possibly the consumer, if they furnished credentials (Reg E access-device exclusion, Reg Z apparent authority), though this is disputed. Merchants often bear card chargebacks.
- **US commercial (UCC 4A):** the customer, if the bank followed agreed security procedures.

Common-law agency principles fill the gaps. Contract allocates risk among the platform, PSP and merchant.

### Cited findings
- EU PSD2 Art. 73-74 (PSP refund, €50 payer cap, no payer liability where SCA was not required). **[statutory text, from knowledge]** — [PSD2](https://eur-lex.europa.eu/eli/dir/2015/2366/oj)
- UK: PSPs generally bear liability for unauthorised transactions. Pinsent Masons floats a two-stage model (user-set parameters, then a per-transaction check, with the deployer responsible for out-of-parameter payments) and possibly extending APP-fraud reimbursement to prompt-injection losses. **[interpretation]** — [Pinsent Masons](https://www.pinsentmasons.com/out-law/analysis/what-new-rules-agentic-ai-payments-look-like)
- US: the competing readings are set out in section 2. The [CBA paper](https://consumerbankers.com/wp-content/uploads/2026/01/CBA-Agentic-Symposium-White-Paper-2026-01v2.pdf) says the consumer may be exposed and banks face more disputes. [Goodwin](https://www.goodwinlaw.com/en/insights/publications/2026/06/insights-technology-aiml-authorizing-agentic-payments) says transfers are presumed authorised and merchants usually bear Reg Z losses. The CBA also flags a shift of chargeback liability to merchants.
- AP2-style signed mandates record user intent, but this "is still untested in courts and before regulators". **[interpretation]** — [Fenwick](https://www.fenwick.com/insights/publications/is-2026-the-year-of-agentic-payments)
- Without federal guidance, principal-agent doctrine places liability on whoever deployed the agent. **[secondary commentary]** — [Chargeflow](https://www.chargeflow.io/blog/agentic-commerce-regulation-what-merchants-need-to-know)

### Inferences
- In every regime, the scope of authority the user granted is the decisive fact. Contemporaneous, tamper-evident records of the mandate, the limits, step-up events and revocation are the evidence the parties will argue over. Per Fenwick, no court or regulator has yet said that such records establish authorisation.

### Gaps
- I found no case law on agent-initiated payments. I found no regulator-issued allocation of liability among AI vendors and PSPs in any jurisdiction.
