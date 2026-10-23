# Cosigna landing page

Local, buildless developer-preview site for Cosigna: a light layout, blue dimensional
gateway illustration, responsive navigation, restrained motion, and an interactive
transaction-boundary example. The existing `design/` prototypes are retained unchanged.
No dependency installation is required to serve the site.

```powershell
cd C:\Users\Harsha\Pictures\project\site
npm run dev
```

Open **http://127.0.0.1:3091**. The site includes `/about`, `/architecture`, and `/benchmarks`.
Try the within-budget, over-limit, and repeated-request scenarios, workflow tabs, FAQs,
motion control, and console launch instructions. The purchase illustration never calls a
backend or executes a payment. Inter is bundled locally with licensing retained in
`assets/FONT-LICENSE.txt`. No analytics, external fonts or outbound calls run on page load.

The site serves only an explicit file allowlist and binds to loopback. It has no account,
lead submission or provider authorization endpoint. It does not mint or share Console
sessions. The dashboard keeps its one-time sign-in link, session and CSRF boundary.
No site is published or domain ownership assumed. The preview is marked `noindex`.

Canvas motion is capped at 30 frames/second and pauses offscreen or in background tabs.
The explicit motion control also pauses the CSS illustration and route animation.
Reduced-motion preferences start the site still. Content stays visible without JavaScript;
navigation, workflow tabs, dialogs and controls support keyboard use. Native page transitions
are an enhancement where supported.

Stripe is labelled as test-mode validated. Other provider entries are planned. Agent
workflows are illustrative and agent templates are drafts. The $528 result is the single
Northwind comparison documented in [EXPERIMENT_NORTHWIND.md](../docs/EXPERIMENT_NORTHWIND.md).
No testimonials, customer logos, partnership claims or performance guarantees are invented.

Mobbin MCP section searches returned a paid-plan requirement. The approved fallback uses
public structural references from [Ramp](https://ramp.com/), [Linear](https://linear.app/),
and [Stripe](https://stripe.com/), with original copy, components, and SVG artwork. No Mobbin
screens or third-party artwork were used. See the [staged product consolidation plan](../docs/COSIGNA_UNIFICATION.md)
for the one-product identity and remaining technical merge work.

Run `npm run check` and `npm test` here. Tests use the installed Console jsdom dependency.
The 12 tests cover landing interactions, stale simulation callbacks, reduced motion,
keyboard navigation, subpage content, route/asset availability, the preview server's
read-only/Host boundary, and the retained pixel concept. JavaScript syntax and CSS parsing
were checked; the gateway SVG was rendered and visually inspected. Full browser layout,
mobile composition, and scroll-feel review remain pending because no browser was connected.

## Pixel-art concept preview

Open **http://127.0.0.1:3091/concept** after starting (or restarting) the preview server.
This separate motion study is retained for reviewing the earlier pixel-art direction.
It has not been substituted for the refreshed landing page at `/`.

The original code-rendered scene includes a neighbourhood, café interior, commuter,
barista, phone, coffee and payment path. Scrolling advances and reverses the same timeline:
intent, one approval, provider confirmation, pickup. Camera travel, character animation,
phone tap and payment effects depend only on timeline progress. There is no video download,
AI runtime, payment request or autoplay. The concept does not claim café integration exists.

`pixel-renderer.js` draws a low-resolution canvas with a bitmap font, limited colors and
crisp scaling. Desktop and portrait use different framing. A static poster provides a
no-JavaScript preview. Chapter buttons, the keyboard-accessible range input, manual mode,
reduced-motion support and short-screen fallback keep the story explorable without scrolling.
Rendering stops when settled, offscreen or in a background tab.

Deterministic desktop/portrait frame rendering, reverse scrolling, manual mode, reduced
motion and short screens remain checked. Representative concept frames were previously
rendered with a temporary native canvas QA tool and visually inspected.
