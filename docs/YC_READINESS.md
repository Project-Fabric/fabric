# Cosigna: YC readiness

> **Current delivery update, 23 October 2026:** the gateway-first plan is tracked in
> [ROADMAP.md](ROADMAP.md) and G-series issues #70–#76 on the private board. The implementation
> and production gaps are documented in [the gateway guide](COSIGNA_GATEWAY.md). The scorecard
> below is the historical 21 October snapshot; its test counts and M-series priorities are
> not the current gateway completion criteria. Test-mode payments remain engineering evidence.

> **Name:** the product is **Cosigna** (renamed from AgentAuth on 21 October 2026). "AgentAuth" below refers to the existing API service, the `agentauth-io` GitHub organization and the earlier application, which keep that name until they are moved.

**As of 21 October 2026 (updated after the Northwind run).** Tracked on the shared board
(github.com/orgs/agentauth-io/projects/1, private), which holds work from both repositories:
the engine (`Project-Fabric/fabric`, private) and the site and legacy app
(`agentauth-io/agentauth`). Update this page when a row changes.

## Where we stand

| What YC looks for | Status | Evidence |
|---|---|---|
| **A product that works** | Strong | Engine shipped: rule packs, approvals, change requests, signed sign-off, tool-call control for any agent, mandates, tamper-evident evidence with offline verification, undo. 133 automated tests; CI on Windows, macOS, Linux. |
| **Proof it does something real** | Good, internal only | Measured with Claude Code on a finance testbed (`EXPERIMENT_FINANCE.md`, `EXPERIMENT_FINANCE_OPS.md`): a 5,628 refund paid without approval 3/3 alone, held and rejected 3/3 with AgentAuth; four refunds needed one decision instead of four under a mandate; a feature needed 2 controller actions instead of 6 to 8. Our testbed, our scenarios, a scripted controller. |
| **Users** | **Missing** | No design partners, no installs outside this team. This is the gap that decides the application. (#36, #51, #58) |
| **A sharp customer and why now** | Drafted | Finance teams running agents for refunds, payables, close; positioned against card-network agent payments (`AGENTAUTH_PRODUCT.md`). Market facts unchecked (#52). |
| **A demo** | **Proof done; recording next** | Northwind (`EXPERIMENT_NORTHWIND.md`): real agents refunding real Stripe test-mode payments. Without AgentAuth 710.98 USD refunded (528.00 wrongly, 2 orders); with it exactly the correct 182.98, 0 wrong, same cost. One run per world. |
| **The story of what changed** | Not yet written | Since the last application: new customer, working product, measured results. (agentauth#7) |
| **Team and company** | Open | Roles, incorporation (#44). |

## The shortest path to a strong application

1. **M1 gateway hardening** (#62, in progress): agent identity and the credential broker, so
   the security story holds up to a technical partner's questions.
2. **M2 Stripe connector and demo** (#63): the product on a system every fintech uses.
3. **Design partners** (#36, #51): three teams using the gateway, even unpaid. One real
   controller reviewing real holds (#58) is worth more than any further feature.
4. **Pitch materials** (#38-#42, agentauth#6, agentauth#7): written from the results above, after
   the market facts are checked (#52).

Everything else on the board (control plane, receipts, AI assist, more connectors) makes the
product better but will not change a YC decision before users exist.
