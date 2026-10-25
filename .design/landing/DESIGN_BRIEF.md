# Design Brief: Cosigna landing page

## Problem

"Our AI agents already refund customers and pay people. Each one holds a live payment key, and the
only thing between that key and our money is a prompt. A ticket that says 'the CFO suspended the
limits' can talk an agent into a $640 refund at 2 a.m. Nobody approved it, and afterwards I can't
prove what happened." The operations or finance lead is accountable for money that software moves,
with no way to set limits, sign the big ones, or show an auditor the record.

## Solution

A quiet page that lets the reader watch Cosigna decide a real agent request in the first screen:
the injected $640 refund is denied, the honest $450 refund waits until they sign it. Everything
after that is evidence: real agents on real tickets, live attacks, a chain that breaks the moment
a record is edited. The page ends with one action: book a demo.

## Experience Principles

1. **Proof before promise** -- show a decision happening before describing the product; every
   claim on the page is a number from the Northwind test, the attack lab or the verifiers.
2. **Calm over spectacle** -- light page, one accent per state, motion only to show cause and
   effect (a request arriving, a decision landing). Nothing zooms, spins or performs.
3. **One action, many doubts removed** -- Book a demo is the only goal; each section answers one
   doubt (keys, prompt injection, overspending, audit, downtime).

## Aesthetic Direction

- **Philosophy**: Swiss/editorial minimalism with product-as-illustration. The product's dark
  screens are the only pictures.
- **Tone**: calm, accountable, precise. Finance tool, not developer toy.
- **Reference points**: Augment (augment.framer.media) for layout, navigation and warmth;
  Circles (circlestock.framer.ai) for the hero request box; DigitalArc for mono labels with big
  section heads; Stripe and Linear for restraint.
- **Anti-references**: neon/cyberpunk dashboards, crypto landing pages, stock photos of people at
  laptops, dark "hacker" aesthetics, fake logos and testimonials, anything that zooms.

## Existing Patterns

- Typography: Geist and Geist Mono (SIL OFL, self-hosted in `design/mocks/refs/fonts`); the
  current site uses Inter and Clash Grotesk.
- Colors: mock 6 palette: paper #f1f1ef, ink #151515, muted #77776f, dark screen #18191a;
  decision colours green #2f8f5b, amber #b7790f, red #c4422f.
- Spacing: no shared scale yet; tokens define a 4px scale.
- Components: the gateway model (`engine.js`, `blake3.js`) and the console widget (`demo.js`,
  `demo.css`) from the mocks; site server routes with a strict CSP (self-hosted assets, no inline
  scripts or styles).

## Component Inventory

| Component | Status | Notes |
| --------- | ------ | ----- |
| Top navigation bar (dark, centred) | Modify | From mock 6; CSP-safe, tokens only |
| Floating "Book a demo" dock | Modify | From mock 6 |
| Hero request box + verdict card | Modify | From mock 6, on a framed dark screen instead of footage |
| Proof chip | Modify | From mock 6 |
| Gateway model (engine + BLAKE3) | Exists | Move into `site/next/` as modules |
| Sticky numbered feature list + live product screens | Modify | From mock 6 |
| Metric pair | Modify | Real numbers only |
| Evidence band (4 numbers) | Modify | From mock 6 |
| Use-case tabs | Modify | From mock 6 |
| Live console (try it) | Exists | `demo.js` themed light |
| FAQ accordion | Modify | Native `details` |
| Closing card, footer | Modify | From mock 6 |
| Book-a-demo dialog | Exists | From `site.js`; mailto until there is a backend |

## Key Interactions

- **Example chip → decision.** Click a chip: the box fills (agent, payment, amount, reason) and the
  verdict card slides in above it: amount, state tag, one-sentence reason, the payment's real
  figures, budget left. Within 300 ms, no layout shift.
- **Held → signed.** On a held request the card offers "Approve · <digest>" and "Reject". Approving
  turns the tag green ("Paid · signed by you") and updates the figures.
- **Write your own.** Agent, payment and amount controls plus the reason; Enter sends.
- **Feature list follows the reader.** The numbered list marks the product screen in view; each
  screen runs its scenario only while visible.
- **Tabs.** Arrow keys move between use cases; the panel replays that scenario.
- **Dock.** Appears after the hero, steps aside for the closing card.

## Responsive Behavior

- ≥ 1100px: as designed; feature list sticky beside the screens.
- 720–1100px: feature list hidden, screens full width with their own headings; console two columns.
- < 720px: nav collapses to logo + Book a demo + menu; hero example chips scroll sideways; verdict
  card sits above the box; console one column; numbers two per row. No sideways page scroll.

## Accessibility Requirements

- Text contrast ≥ 4.5:1 (muted grey on paper checked); decision colours never the only signal
  (each tag also says paid / waits / denied).
- Every control reachable by keyboard; visible focus ring; tabs follow the ARIA tabs pattern;
  menu closes on Escape; dialog traps focus (native `dialog`).
- Verdict card and console results announced via `aria-live="polite"`.
- `prefers-reduced-motion`: no entrances, no loops; screens show a settled state.
- Works without JavaScript as a readable page (the demos need it; the copy does not).

## Out of Scope

- Pricing, customer logos, testimonials, team, blog: we have none, and none will be invented.
- A real booking backend (mailto until one exists).
- Dark mode for the page itself (product screens are dark by design).
- Replacing the home page at `/` (a separate decision after review).
- Mobile app, console redesign.
