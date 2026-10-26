# Replay example: 90 days of refunds

A synthetic company ("Northwind Supplies", invented data) with 122 support refunds over 90 days,
six of them labelled as mistakes afterwards. Run the proposed policy over them:

```sh
cargo run -p cosigna-agent -- replay --history examples/replay/refunds-90-days.csv \
  --policy examples/replay/policy.json --cases examples/replay/cases.csv --text
```

The policy (`policy.json`): a $15,000 budget renewed every 30 days, at most $500 per refund, two
people above $300, and an agent that may pay up to $100 alone when the refund names its case.

Result: 91 refunds paid automatically, 28 held for a person (about 2 a week), 3 stopped. Five
of the six mistakes would not have been paid without a person: the wrong amount and the wrong
order are held, and the duplicate, the over-refund and the injected $640 refund are stopped or
held. The sixth is still paid automatically, because the company's own case record agrees with
it. Replay says so rather than hiding it: no rule can catch a mistake the records also make.

To try it on real data, export refunds as a CSV with these columns (header names matter, order
does not): `id`, `at` (Unix seconds or `2026-09-01T10:00:00Z`), `kind` (`refund`), `source`
(the payment), `amount_minor`, `currency`, and optionally `customer`, `case_id`,
`payment_amount_minor` and `mistake` (`yes`). Cases: `id`, `customer`, `source`, `currency`,
`owed_minor`. Everything runs locally; nothing is sent anywhere. Rebuild this example with
`python -I examples/replay/make_example.py`.
