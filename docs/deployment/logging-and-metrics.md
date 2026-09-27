# Logging and metrics

How the service makes itself observable: one span per request, one audit line per mutation, a
self-reporting boot, and Prometheus counters — and the two environment settings that shape it.

Every request opens one `http.request` span carrying the method, the path, the query string (every
secret query parameter redacted), the status and the latency. Every mutating operation writes one
`tucano.audit` event naming the action, the resource, the identifier, the acting subject (`user=`
— the authenticated account, or the explicit `-` where no authentication is enforced) and the
outcome, alongside the error code when it was refused. Sign-ins, token rotations (a spent or
unknown refresh token is recorded as `replay`) and logout write the same event kind. No request
body, credential, or attachment's contents are ever logged.

The service opens by naming its own build under the `tucano.boot` target — version, CI build
number, toolchain, data directory, auth posture — so a container can confirm the artifact a tag
claims (#418).

`TUCANO_LOG` is the `tracing-subscriber` directive set, defaulting to `info` — the request spans, the
audit lines and the failures, without the per-connection noise `debug` adds — including an `error`
line for every 500 that names the server-side cause (the client keeps receiving the fixed,
redacted envelope, #417). `TUCANO_LOG_FORMAT` is
`compact` (the default, one human-readable line per event, coloured only when stdout is a terminal)
or `json` (one object per event, uncoloured, for a collector to parse). Both are read once at
startup, so an unparseable directive set or an unknown format stops the server rather than a
request.

`GET /metrics` serves Prometheus counters in the text exposition format the API renders itself — no
new endpoint dependency and no token, since the route sits with the other unguarded ones. Each
series counts requests by method, by the first segment of the matched route template, and by status
class.

## Routing and parsing

The audit trail is a single tracing target, `tucano.audit`, so a deployment keeps it beside or
apart from the request log with one `TUCANO_LOG` directive (see the audit lines above). Because
both levels above `info` are always emitted, the 500-cause `error` lines ride the default with no
configuration at all. `TUCANO_LOG_FORMAT=json` makes every line — request spans, audit events and
errors alike — one parseable JSON object for a collector.
