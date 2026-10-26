# Case records: choosing where they come from

Case binding checks each refund against the business's own record of what the customer is
owed. Which system those records come from is a switch, set when the gateway starts:

```sh
cosigna-agent serve --cases examples/cases/csv.json       # a CSV export
cosigna-agent serve --cases examples/cases/zendesk.json   # Zendesk tickets, read live
cosigna-agent serve                                       # the synthetic case book (default)
```

Only one source is connected at a time. With `--cases`, the synthetic case book is not used.

## A CSV export (any system)

`csv.json` points at `cases.csv` (path relative to the JSON file). Columns: `id`, `customer`
(the payment system's customer id), `source` (the payment to refund), `currency`,
`owed_minor` (cents). The file is read once at startup. A repeated id is refused. This works
with any help desk, store or spreadsheet that can export a table, and nothing leaves the
machine. The example rows refer to the gateway's synthetic payments.

## A help desk over HTTPS (Zendesk example)

`zendesk.json` reads one ticket per refund: `GET /api/v2/tickets/{id}.json` with an API token.
An agent names the ticket as `zd_<ticket number>`. Replace the subdomain, the username and the
three custom field ids with your own. The ticket fields must hold the payment system's
customer id, the payment id and the amount owed (`major`: `12.50`; `minor`: `1250`).

- **The secret** comes only from the environment variable the file names
  (`COSIGNA_CASES_SECRET`). It is never written to the file, logs or evidence.
- **Addresses** must be `https://`. Plain HTTP is accepted only on `127.0.0.1`, for tests.
  Redirects are not followed.
- **Field paths** are JSON pointers. A segment `[id=…]` picks the array element with that id,
  which is how Zendesk lists custom fields. If two elements match, the value is not read.
- **Failing closed:** if the help desk does not answer, or a ticket lacks a mapped field or
  has an invalid value, the request fails with `case_records_unavailable` (HTTP 503). Nothing
  is decided or recorded. Only a definite "not found" (404) counts as a missing case, which is
  denied.
- **Exposure:** a help desk cannot be listed, so the exposure report counts agents that must
  name a case up to their whole budget, and says so in its notes.

**Important:** map only fields the paying agent cannot write. If the AI agent can edit the
ticket field that says what is owed, the check only repeats the agent's own claim. Use a field
set by a person or by your store integration, and restrict it in the help desk's permissions.

**What is tested:** the mapping, authentication, failure handling and decisions are tested
against a local stand-in built from Zendesk's published ticket API. They have not been run
against a real Zendesk account yet. Other systems (Gorgias, Intercom, Shopify) use the same
`http` source with their own address, authentication (`basic`, `bearer`, or a named `header`)
and field paths. Each needs checking against a real account before use.
