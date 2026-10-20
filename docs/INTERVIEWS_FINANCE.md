# Interview script: AI agents under finance controls

**For:** the co-founder · **Goal:** ~15 interviews in two weeks · **Length:** 25 minutes each

Ask about what people **do now**, not whether they would like Fabric. Do not pitch until the last
five minutes, and only if they ask. Record exact words; they go into the pitch.

## Who

| Group | How many | Where to find them |
|---|---|---|
| Engineering leads at SOX-subject fintechs, payments companies, pre-IPO startups | 5 | LinkedIn, fintech Slack communities, YC alumni companies |
| Controllers, finance-operations or accounting leads using AI | 5 | LinkedIn, CFO and controller communities, accounting firm contacts |
| Internal or external IT auditors (ITGC / SOX) | 3–5 | Big Four and mid-tier audit firms, ISACA chapters |

## Opening (all groups)

"We're researching how companies handle AI agents under financial controls. No sales pitch;
we'd like to learn how it works for you today."

## Engineering leads

1. Do your engineers use coding agents (Claude Code, Codex, Cursor, Copilot)? On which systems?
2. Are any of those systems in SOX scope (billing, ledger, revenue, payments)?
3. **How do agent-written changes go through your change-management controls today?**
4. Who is recorded as the author when an agent wrote the change? Who approves it?
5. Has an auditor asked about AI-written changes yet? What did they ask for?
6. Tell me about the last time an agent changed something it shouldn't have. How did you find
   out, and how did you undo it?
7. Are there systems where agents are banned? Why?
8. What evidence do you hand auditors for a change today? How long does it take to assemble?
9. If you could change one thing about this, what would it be?

## Controllers and finance operations

1. Which AI tools or agents touch your close, journal entries, reconciliations or reports?
2. Can an AI post or change anything in the ledger, or only suggest?
3. **How do you show an auditor what the AI did and who reviewed it?**
4. Has an AI output ever been wrong in a way that reached the books? What happened next?
5. What stops you letting AI do more of the work today?
6. Who owns the risk if an AI-assisted control fails?

## Auditors

1. Are clients using AI agents in ICFR-relevant processes or on in-scope systems?
2. **What evidence do you ask for when an AI agent changed code or data?**
3. What would make that evidence insufficient? Have you seen a finding or deficiency from it?
4. How do you treat segregation of duties when an agent wrote a change?
5. Which format of evidence do you trust: logs, screenshots, signed exports, tickets?
6. What are you expecting to ask about AI in this year's audit cycle that you didn't last year?

## Closing (all groups)

- "Who else should we talk to about this?" (always ask for two names)
- "Can we follow up when we have something to show?"
- Only if asked: Fabric records every agent change with its full before and after, enforces
  approval rules, lets you undo a single change, and exports evidence for auditors.

## After each interview

Write within an hour, in a shared sheet:

- role, company type, SOX-subject or not;
- do they use agents on in-scope systems (yes / no / banned);
- the pain in their words (quote);
- what they do today and what it costs (time, people, tools);
- has an auditor asked (yes / no / expecting);
- would they pilot (yes / maybe / no);
- referrals.

The stop rules are in `PIVOT_FINANCE.md`; read the sheet against them after two weeks, not
before.
