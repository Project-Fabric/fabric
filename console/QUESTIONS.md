# Console contract questions

1. **Live approval amount units.** Requests include raw tool input but no unit/currency metadata.
   The synthetic `stripe/issue_refund` inbox uses USD cents (45000 = $450.00 in the brief),
   but it has no matching payment declaration. Fixtures use that explicit example; live mode
   requires a matching declaration from `fabric payments list --json`. When units cannot be
   determined, show the raw input and an unavailable amount; do not silently assume cents.
   Should the engine add normalized payment context to approval requests?
2. **Mandate currency.** Terms and uses have no currency. Uses are scaled by 10^6, per
   `mandates.rs`. Display them as units, not dollars. Should mandates gain a currency field?
3. **Evidence export destination.** No destination-selection contract is specified. Live export
   is disabled until a person configures `CONSOLE_EXPORT_DIR`; then use a unique child directory
   and show its absolute path. Should the console offer a native folder picker later?
4. **Consent mutations.** The brief specifies listing API consents but only grant/revoke actions
   for engine mandates. Consent creation/revocation is not added without an API action contract.
5. **Activity completeness.** `calls -n 200` and the consent fixture are bounded samples.
   Spending totals are labelled as recorded in the loaded activity, not a complete ledger.
   What pagination/range contract should provide complete daily/weekly totals?
6. **Blocked payment context.** The captured three blocked Stripe calls have `payment: null`.
   Their raw gateway refusal remains visible, but the console does not invent an AgentAuth
   decision, normalized amount or authorization code from that prose. Should `evidence calls`
   join the related payment-authorization event for blocked calls as it does for allowed ones?

Fixture actions simulate changes in per-session memory so the inbox can update, without
writing fixtures or invoking the engine. They are always labelled as simulated. This resolves
the brief's no-op actions / updated-list flow without presenting a fixture decision as real.

## Verification status

Dependencies were installed from the person's terminal. Typecheck, lint, 17 component/command
tests, four session/CSRF/contrast tests, production build and the client-bundle secret scan
pass. The production HTTP tests exercise both approval outcomes, session/CSRF rejection,
stale-decision refusal, per-session fixture changes, one-time token handling, the other action
simulations and every page route. No engine command is run by these fixture tests.

Actual browser rendering at 375 px and screen-reader inspection remain pending: the browser
controller reports no available browsers. The CSS has a phone layout and tested AA text
colors, but those checks do not establish the rendered layout or the full accessibility
experience. A read-only live console API check loaded Northwind's 100 tool calls, including
five successful money-tool calls and all three blocked ones. Other live sections reported
their actual configuration/permission errors in this agent session (consents, approvals,
mandates and verification). Live financial decisions have not been run by this agent.
