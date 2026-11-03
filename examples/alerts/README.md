# Alerts: tell people when money is waiting for them

The worker sends an alert when a payment is held for approval, when it has one approval and
needs a second person, and when a payment was sent but its result is still unknown after two
minutes. Alerts are skipped if the payment is no longer waiting by the time they are due.

```sh
cosigna-agent worker --alerts examples/alerts/alerts.json
```

- **Slack:** create an incoming webhook for the approvers' channel and put its URL in the
  variable named by `url_env` (here `COSIGNA_ALERT_SLACK_URL`), or in a file named by
  `COSIGNA_ALERT_SLACK_URL_FILE`. The URL is a secret: anyone with it can post to the channel.
- **Webhook:** any HTTPS endpoint (Teams, PagerDuty, an email relay, your own service). Set a
  signing secret of at least 16 characters in `secret_env`. Check `X-Cosigna-Signature`
  (`v1=` + hex HMAC-SHA256 of `<X-Cosigna-Timestamp>.<body>`) in constant time, and reject
  old timestamps.
- Remove a destination you do not use. Alerts carry the amount, the reason a payment is held,
  the agent's name, the case id and a link to `console_url`; never a credential, the customer
  or the agent's free-text reason.

The same destinations receive monitor findings: a failed evidence check, another destination
that has stopped answering, and, with Stripe connected, refunds that do not match the ledger.

A worker that has stopped cannot report that. To find out, sign up for any service that alarms
when pings stop, put its address in `COSIGNA_HEARTBEAT_URL`, and add
`"heartbeat": {"url_env": "COSIGNA_HEARTBEAT_URL"}` to the file. The worker pings it every 60
seconds. `GET /gateway/v1/monitor` shows the same state to the owner.

With Docker, add `--alerts /config/alerts.json` to the worker's command and mount the file
and secrets as in `deploy/docker/compose.stripe-test.yaml`. The full format, retries and
delivery guarantees are in `docs/STORAGE_FORMAT.md` (Alerts).
