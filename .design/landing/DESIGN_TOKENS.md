# Design tokens: Cosigna landing

Source of truth: `site/next/tokens.css`. Philosophy from the brief: Swiss/editorial minimalism with
product-as-illustration; colour only for decisions.

## Contrast (WCAG 2.1, measured)

| Pair | Ratio |
|------|-------|
| text #151515 on bg #f1f1ef | 16.15 |
| text-muted #62625b on bg / surface-2 / surface / white | 5.43 / 5.01 / 5.88 / 6.14 |
| pay #23714a on pay-bg / bg | 5.10 / 5.26 |
| wait #865a00 on wait-bg / bg | 5.16 / 5.34 |
| deny #a8382a on deny-bg / bg | 5.12 / 5.69 |
| screen-text #e9e9e7 on screen #18191a | 14.48 |
| screen-muted #9a9a96 on screen | 6.23 |
| pay / wait / deny screen variants on screen | 9.83 / 10.75 / 7.85 |

`text-faint` (2.95:1) is for decoration and large numerals only. Mock 6's grey (#77776f, 3.99:1)
and its tag colours failed and were darkened.

## Scales

- Spacing: 4px base: 4, 8, 12, 16, 20, 24, 32, 40, 48, 64, 96, 128.
- Type: label 11 mono caps · caption 12.5 · small 13.5 · body 15 · lead 17 · h3 26 · h2 32–46 ·
  h1 42–66 · display 40–78 caps · numeral 40–64 light.
- Radius: 3 / 4 / 8 / pill. Shadows: float, card.
- Motion: 150 ms press (scale .97), 300 ms reveal, 450 ms dock; one ease-out curve; all zero under
  reduced motion.

## Fonts

Geist and Geist Mono, © Vercel with basement.studio, SIL Open Font License 1.1, self-hosted in
`site/assets/fonts/geist/` with the licence.
