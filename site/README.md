# Cosigna landing page

Local, buildless landing page inspired by the spacious composition, serif typography,
editorial progression and restrained motion of [Anthropic Glasswing](https://www.anthropic.com/glasswing).
Cosigna's overlapping-ring canvas, text and workflow examples are original. The existing
`design/` prototypes are retained unchanged. No dependency installation is required.

```powershell
cd C:\Users\Harsha\Pictures\project\site
npm run dev
```

Open **http://127.0.0.1:3091**. Use the workflow tabs, integration accordions, role links,
motion control and dashboard launch instructions. `npm run check` checks JavaScript syntax.
Inter is bundled locally; the serif display uses Georgia. All font licensing is retained
in `assets/FONT-LICENSE.txt`. No analytics, external fonts or outbound calls run on page load.

The site serves only an explicit file allowlist and binds to loopback. It has no account,
lead submission or provider authorization endpoint. It does not mint or share Console
sessions. The dashboard keeps its one-time sign-in link, session and CSRF boundary.
No site is published or domain ownership assumed. The preview is marked `noindex`.

Canvas motion is capped at 30 frames/second, pauses in background tabs, supports an explicit
pause button, and starts still for reduced-motion preferences. Content stays visible without
JavaScript; keyboard navigation and motion reduction are supported. Browser visual review
is limited by browser availability in the development environment.

Stripe is labelled as test-mode validated. Other provider entries are planned. Agent
workflows are illustrative and agent templates are drafts. The $528 result is the single
Northwind comparison documented in [EXPERIMENT_NORTHWIND.md](../docs/EXPERIMENT_NORTHWIND.md).
No testimonials, customer logos, partnership claims or performance guarantees are invented.

## Landing redesign checkpoint

The home page now loads `home.css`, `tokens.css` and `base.css`. It keeps the original
ink, paper and clay palette, Georgia display headings, self-hosted Inter, all prose and
figures, route destinations and control IDs. Captions use sentence or lower case; decorative
link arrows and status dots were removed. Workflow tabs declare their responsive orientation,
and page content remains visible with JavaScript disabled.

The seven landing and pixel-animation regressions, JavaScript syntax checks, Rust formatting,
Clippy and workspace tests passed on Windows on October 31, 2026. Hallmark's static home-page
check reports no failures or warnings. Browser launch timed out in the available environment;
visual layout, fold and rendered contrast still need browser verification. No deployment or
publication was performed.

## Pixel-art concept preview

Open **http://127.0.0.1:3091/concept** after starting (or restarting) the preview server.
The original landing page at `/` is unchanged. This separate motion study is for reviewing
the pixel-art direction before deciding on a landing-page replacement.

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

Run `npm run check` and `npm test` here. Tests reuse the installed Console jsdom dependency.
Deterministic desktop/portrait frame rendering, reverse scrolling, manual mode, reduced
motion, short screens and the existing landing interactions are checked. Representative
frames were rendered with a temporary native canvas QA tool and visually inspected.
Full browser layout and scroll-feel review remain pending because no browser is available.
