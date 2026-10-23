# Cosigna design: "Graphite & Signature"

Static HTML mockups of the Cosigna look. Open any file in a browser; no build step.

| File | What it shows |
|---|---|
| `brand.html` | Mark, colours, type, status chips, voice |
| `landing.html` | Website hero, proof numbers, how it works |
| `approvals.html` | Console: approval inbox with detail panel and confirm step |
| `activity.html` | Console: today's summary and the decision timeline |
| `phone.html` | Approving one payment on a phone |
| `cosigna.css` | Design tokens and shared components (buttons, chips, meter, card) |
| `console.css` | Console layout (sidebar, list + detail, timeline) |

## Rules

- Dark graphite ground, one accent (Signature violet `#8E7DFF`). Alternatives tried:
  `#C6F432`, `#5B9DFF`, `#F2F3F5`.
- Status always has a text label and an icon, never colour alone: Allowed (green),
  Waiting (amber), Blocked (red), Evidence (accent).
- Money and identifiers are in JetBrains Mono; headlines in Inter Tight; everything else Inter.
- Every money action takes two steps (button, then confirm) and names the exact amount.

## Data

The $528.00 → $0.00 and 236-record numbers are from the Northwind run
(`docs/EXPERIMENT_NORTHWIND.md`), and the inbox requests are from `console/fixtures/`.
The timeline rows are illustrative. The phone screen's "Approve with Face ID" (passkey
approvals) is a planned feature, not built yet; today approvals are signed with an
Ed25519 approver key (`fabric approver`).

For the console build (`console/BRIEF.md`), map the tokens in `cosigna.css` one to one
into the Tailwind theme.
