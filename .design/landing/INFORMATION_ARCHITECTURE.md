# Information Architecture: Cosigna landing page (`/next`)

## Site Map

- Landing `/next` (one page, anchored sections)
  - Hero `#top`
  - Product `#product`
  - Use cases `#cases`
  - Try it `#try`
  - Evidence `#evidence`
  - FAQ `#faq`
- Existing pages, unchanged: home `/`, ledger `/ledger`, concept `/concept`.

## Navigation Model

- **Primary navigation** (5 max): Product · Use cases · Evidence · Try it · FAQ. Anchors with smooth
  scroll; the section in view is marked.
- **Secondary navigation**: the sticky numbered feature list inside Product; tabs inside Use cases.
- **Utility navigation**: "Book a demo" button (nav and floating dock); no login yet.
- **Mobile navigation**: logo + Book a demo + menu button; the menu drops down inside the bar,
  closes on link, Escape or second tap.

## Content Hierarchy

### Landing
1. Proof chip + headline + one sentence + Book a demo -- what this is and the action, in 5 seconds.
2. Live request box on a dark product screen -- proof that it is real, in 10 seconds.
3. "Works with" line -- removes "will this fit our stack?".
4. Product: three numbered capabilities, each a live screen + two real numbers -- approve the exact
   payment; keep keys out of agents; prove every decision. Ends with the Northwind result as a quote.
5. Use cases: four tabs -- support refunds, prompt injection, double refunds, agents without
   authority.
6. Try it: the full console.
7. Evidence: four numbers from the attack lab.
8. FAQ: money custody, agent changes, rails today, downtime, what evidence proves.
9. Closing card + footer -- Book a demo again.

## User Flows

### Operations lead, first visit
1. Lands on `/next`, reads headline.
2. Clicks "The fake CFO note · $640" → sees "Denied: over the per-refund limit".
3. Clicks "Cancelled order · $450" → sees "Waits for you" → clicks Approve → "Paid · signed by you".
   - If curious → scrolls Product / Try it.
   - If convinced → Book a demo (nav, dock or hero).
4. Dialog → name, work email, company, what agents pay → email app opens.

### Investor, skim
1. Reads proof chip → Evidence numbers → Northwind quote.
2. Tries one chip in the hero → Book a demo or leaves with the URL.

## Naming Conventions

| Concept | Label in UI | Notes |
|---------|-------------|-------|
| Operation awaiting owner | Waits for you | Never "pending" or "held" in prose |
| Auto-approved + paid | Paid | "Paid · signed by you" after an owner approval |
| Policy denial | Denied: <rule in words> | Rule names only in mono detail |
| Owner approval | Approve / sign | "Sign" in prose, "Approve" on buttons |
| Agent permissions | Grant | "What it may do, through which rail, how much alone" |
| Evidence chain | Evidence | "Chain" only next to verification |
| Gateway model on the page | Live demo | Always with "no money, no network, no keys" |

## Component Reuse Map

| Component | Used on | Behavior differences |
|-----------|---------|---------------------|
| Nav bar + dock | Landing | Dock hidden in hero and closing |
| Dark product screen frame | Hero, Product, Use cases, Closing | Hero holds the request box |
| Section head (mono label + big caps) | Use cases, Try it, Evidence | Evidence on dark band |
| Book-a-demo dialog | All CTAs | — |

## Content Growth Plan

Fixed single page. New proof (pilot results, a real customer) replaces numbers or the quote; it does
not add sections. Customer stories, docs and pricing become their own pages later.

## URL Strategy

- Pattern: `/next` now; becomes `/` if chosen.
- Anchors: `#product`, `#cases`, `#try`, `#evidence`, `#faq`.
- No query parameters.
