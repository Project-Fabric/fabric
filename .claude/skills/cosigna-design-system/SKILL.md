---
name: cosigna-design-system
description: Cosigna's visual rules for marketing pages (landing, /next): tokens, type, colour-for-decisions, components and interaction. Use when building or changing any Cosigna web page, section or component, or reviewing one.
---

# Cosigna design system

Source of truth: `site/next/tokens.css`. Brief: `.design/landing/DESIGN_BRIEF.md`.
Use tokens only. No raw hex, px font sizes or durations in components.

## Direction

Calm, accountable, precise. A light page; the product's dark screens are the only pictures.
Proof before promise: every claim is a real number from the Northwind test, the attack lab or the
verifiers. No invented logos, testimonials, customers or pricing.

## Colour

- Page `--color-bg`, raised `--color-surface`, quiet card `--color-surface-2`, text `--color-text`,
  secondary `--color-text-muted`. `--color-text-faint` is decoration and big numerals only.
- Colour means a decision and nothing else: `--color-pay` (paid), `--color-wait` (waits for you),
  `--color-deny` (denied), each with `-bg` (on paper) and `-screen` (on dark) variants. A tag always
  says the word too; colour is never the only signal.
- Dark product screens: `--color-screen*` tokens.

## Type

Geist + Geist Mono. Labels: `--text-label`, uppercase, `--tracking-label`. Headings h1/h2/h3,
section heads `--text-display` uppercase, numbers `--text-numeral`. Two-tone headings: second phrase
in `--color-text-muted`.

## Spacing and shape

4px scale (`--space-*`), page gutter `--gutter`, section rhythm `--section`. Radius 4 (`--radius`),
8 for cards and screens. No other values.

## Components

- Nav: dark bar centred at the top (max 712px), logo badge, ≤5 anchor links, white "Book a demo ↗".
  Floating dock after the hero.
- Buttons: dark (primary), light (on dark), ghost (outline). 42px tall, press scale .97.
- Product screen: dark frame, sidebar + main, rows with state tags; powered by the gateway model.
- Section head: mono dotted label left, big caps title right.
- Numbers: light numerals with a mono caption; real figures only.

## Interaction

- Motion shows cause and effect only (a request arriving, a decision landing). 150 ms press,
  300 ms reveal, one ease-out. Nothing zooms, spins or loops on the page itself; product screens
  may loop only while visible.
- `prefers-reduced-motion`: no entrances, no loops, settled states.
- Every control keyboard-reachable with a visible focus ring; results announced with `aria-live`.

## Hard constraints (site CSP)

Self-hosted assets only; no inline `<script>` or `style` attributes (set styles from JS via the
CSSOM or with classes); every new file needs a route in `site/server.mjs`.
