# Build tasks: Cosigna landing (`/next`)

Foundation first, then features, then polish. Each task ends with a check in a real browser.

- [x] **Tokens**: `site/next/tokens.css` and Geist self-hosted. _Done in step 4._
- [x] **Shell**: server routes for `/next` and its files under the strict CSP; page skeleton with the
  dark centred nav (active section, phone menu), floating Book-a-demo dock, footer, Book-a-demo
  dialog. Done when the page loads at `/next` with no CSP errors. _Reuses: mock 6 nav, `site.js`._
- [x] **Gateway model as modules**: copy `engine.js`, `blake3.js` into `site/next/` with their tests
  running in `npm test`. _Reuses: mocks/shared._
- [x] **Hero**: proof chip, headline, sentence, Book a demo; dark framed screen holding the request
  box, example chips and verdict card (approve / reject on held). Done when the CFO note is denied
  and the $450 refund can be signed.
- [x] **Product**: sticky numbered list + three live screens (inbox, grant, evidence) + metric pairs
  + Northwind quote. Loops run only while visible.
- [x] **Use cases**: four ARIA tabs replaying their scenario. No inline styles.
- [x] **Try it**: the console themed light. **Evidence** band, **FAQ**, **closing card**.
- [x] **Polish**: entrances (subtle), reduced motion settled states, phone layout, focus states,
  no sideways scroll.
- [x] **Tests**: page structure, CSP compliance (no inline style or script), engine reproduces
  Northwind, hero decisions.
- [x] **Design review** against the brief; fix by severity.
