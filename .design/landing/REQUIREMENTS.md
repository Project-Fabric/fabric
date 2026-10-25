# Cosigna landing page: requirements

Decisions from the grill-me session (Julian Oczkowski's design process, step 1). Each entry is a
question, the answer, and why.

## 1. Who is the page for, and what is the one action?

- **Product:** Cosigna, the control and audit layer between a company's AI agents and its money:
  per-agent limits, a person signs the exact payment above them, credential isolation,
  tamper-evident evidence.
- **Primary reader:** the operations or finance lead at a company whose AI agents already issue
  refunds or payouts.
- **Secondary reader:** YC partners and investors checking that the product is real.
- **One action:** Book a demo. "Try the live demo" is supporting proof, not a second goal.
- **Consequence:** every section either earns trust (Northwind result, attack lab, live demo) or
  removes a doubt (agents never hold keys, fails closed, runs on your machine).

## 2. How should it feel?

- **Answer:** warm, light and calm, with dark product screens inside it (after Augment; mock 6).
- **Why:** the reader is handing over control of money. "Careful, accountable, in control" reads as
  a bright, quiet page; very dark cinematic pages read as dev-tool or studio.
- **Rules:** off-white background, near-black ink, one muted grey. Colour only for decisions:
  green paid, amber waits for you, red denied. Geist with a mono label face. Motion is subtle;
  nothing zooms or performs. Dark product screens and night footage are accents, not the base.

## 3. What does the first screen show?

- **Answer:** B. Statement and a live decision: headline, one sentence, Book a demo, and under it
  a working agent request box. Example chips ("The fake CFO note · $640", "Cancelled order · $450")
  let the reader watch Cosigna deny one and sign the other in seconds. The evidence number sits as
  a small chip above the headline ("Northwind test: $0 wrongly refunded →").
- **Backdrop:** no footage. The request box sits on the product's own dark screen, framed on the
  light page. Calmer, lighter, and no 15 MB video or brand signs in shot. Footage can be revisited.
- **Built at:** `/next` on the site, so the restored home page stays as it is until we swap.
