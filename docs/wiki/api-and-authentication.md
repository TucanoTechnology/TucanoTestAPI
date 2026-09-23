# API and authentication quickstart

This page is for the person writing a client. It takes you from nothing to an authenticated request
in a few commands, then explains the two things every client has to get right whatever it does next:
the error envelope, and the request limits. It closes on where the real contract lives.

The [installation page](getting-started.md) covers running the service. This page assumes it is
running and that you can reach it. Every example uses `http://localhost:3100`, which is the port
Compose publishes; a plain `docker run` of the image listens on `3000` instead.

## The shortest path to an authenticated call

**1. Check the service is up.** `/health` is public, so it is the one call that works before you
have any credentials:

```sh
curl -s http://localhost:3100/health
```

```json
{"status":"ok","storage":"filesystem"}
```

**2. Sign in.** `POST /auth/login` (`login`) is public too — it is how you get credentials in the
first place:

```sh
curl -s -X POST http://localhost:3100/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"change-me-please"}'
```

```json
{
  "accessToken": "eyJhbGciOiJIUzI1NiIs…",
  "refreshToken": "u7Qk…",
  "tokenType": "Bearer",
  "expiresIn": 900
}
```

`LoginRequest` is exactly two fields, `username` and `password`, and both are required. In the
shipped Compose stack the account is the `TUCANO_BOOTSTRAP_USERNAME`/`TUCANO_BOOTSTRAP_PASSWORD`
pair from `.env`; the placeholders below stand in for your own credentials.

**3. Keep the access token in a variable.** `expiresIn` is seconds until the access token dies:

```sh
TOKEN=$(curl -s -X POST http://localhost:3100/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"change-me-please"}' \
  | python3 -c 'import sys, json; print(json.load(sys.stdin)["accessToken"])')
```

**4. Send it.** Every guarded operation takes the token in an `Authorization` header:

```sh
curl -s http://localhost:3100/auth/me -H "Authorization: Bearer $TOKEN"
```

```json
{"id":"…","username":"admin","systemAdmin":true,"roles":{}}
```

`GET /auth/me` (`getCurrentUser`) is the cheapest way to confirm a token works and to see who the
service thinks you are. That is the whole loop: **sign in, carry `Authorization: Bearer <token>`,
refresh before it expires.** Everything below is detail on top of it.

## What authentication is, and when it applies

**The shipped Compose stack authenticates by default** — `docker-compose.yml` sets
`TUCANO_AUTH_REQUIRED=true` and takes the signing secret and bootstrap account from `.env`. The
*service's* own default is `false`: with `TUCANO_AUTH_REQUIRED` unset or `false` every route is
anonymous and no token is needed, which is only safe on a machine nothing else can reach. Turning it
on in a hand-rolled deployment requires a signing secret of at least 32 bytes:

```yaml
    environment:
      TUCANO_AUTH_REQUIRED: "true"
      TUCANO_JWT_SECRET: "a-development-secret-of-at-least-32-bytes"
      TUCANO_BOOTSTRAP_USERNAME: admin
      TUCANO_BOOTSTRAP_PASSWORD: change-me-please
```

The bootstrap pair creates the first `systemAdmin` account, but **only when the auth store holds no
accounts at all** — it is a one-time step, and leaving the variables in place afterwards is
harmless. Change the password, or remove them.

Eight operations need no caller even when authentication is on. In `openapi.json` these are the
operations marked `security: []` while the document's global requirement is `bearerAuth`:

| Public operation | Why it is public |
| --- | --- |
| `GET /health` | A load balancer has no credentials |
| `GET /ready` (`getReady`) | The orchestrator that gates traffic on readiness is the same caller that has no credentials, and the answer names no path |
| `GET /diagnostics` (`getDiagnostics`) | Its caller is an operator with a shell on the host, not a client with a token; it reports booleans and a timestamp, never a path |
| `GET /metrics` (`getMetrics`) | A Prometheus scraper has no credentials either, and the counters name no stored content |
| `POST /auth/login` (`login`) | It is how you obtain credentials |
| `POST /auth/refresh` (`refreshSession`) | Its whole job is to renew an expired access token |
| `GET /openapi.json`, `GET /api-docs` | The contract, so clients can read it unauthenticated |

Everything else requires a valid access token. What you are then *allowed* to do is decided per
project by role — `viewer` < `editor` < `owner`, with a `systemAdmin` account reaching everything.
Listings are **filtered** to the projects you can reach rather than refused, so an empty list is
normal and is not evidence that something is broken; a direct read or write of a project you cannot
reach answers `403 forbidden`. Creating or duplicating a project requires `systemAdmin`.

> **There is no grant-administration route yet.** Accounts and per-project roles live as JSON under
> `$TUCANO_DATA_DIR/auth/` and are provisioned out of band. A client cannot grant itself access, and
> an integration test that needs a non-administrator account has to provision it directly.

## Tokens: the access token, the refresh token, and the window

Two tokens come back from a sign-in, and they behave differently:

| Token | Lifetime (default) | Sent where | Behaviour |
| --- | --- | --- | --- |
| `accessToken` | `TUCANO_ACCESS_TOKEN_TTL`, default `15m` | `Authorization: Bearer …` on every guarded call | Stateless: its signature and expiry are the whole check |
| `refreshToken` | `TUCANO_REFRESH_TOKEN_TTL`, default `14d` | Body of `POST /auth/refresh` | Single-use — **rotated on every exchange** |

**The access token is stateless, and that is a property clients must design around.** Nothing is
re-read when it is presented: a role revoked, a password changed, or an account disabled after the
token was minted has no effect until the token expires. `GET /auth/me` reports the authority *the
token* carries, not the account's current authority. Keep the access-token lifetime short if that
window matters to you; the default of 15 minutes is chosen to bound it.

**The refresh token can only be used once.** `POST /auth/refresh` (`refreshSession`) takes
`{"refreshToken": "…"}` and answers a **new** `SessionResponse` — a new access token *and* a new
refresh token. The token you presented is revoked in the same call:

```sh
curl -s -X POST http://localhost:3100/auth/refresh \
  -H 'Content-Type: application/json' \
  -d '{"refreshToken":"'"$REFRESH"'"}'
```

```json
{"accessToken":"eyJhbGciOiJIUzI1NiIs…","refreshToken":"vB3n…","tokenType":"Bearer","expiresIn":900}
```

> **Store the new refresh token every time, before you use the old one.** Replaying a refresh token
> — because a response was lost, a retry fired, or two requests raced — answers
> `401 invalid_refresh_token`, and a client that did not persist the replacement has to sign in
> again. Treat the pair as a single rotating value, not as two independent strings.

`POST /auth/logout` (`logout`) requires a valid access token **and** a body naming the refresh token
to revoke alongside it — `{"refreshToken": "…"}` is required, and the call is refused with
`invalid_request` if it is missing an `Authorization` header, a body, or the `refreshToken` field:

```sh
curl -s -X POST http://localhost:3100/auth/logout \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"refreshToken":"'"$REFRESH"'"}'
```

```json
{"message":"Signed out"}
```

Signing out therefore ends both halves of the session rather than only the short-lived one. Revoking
a refresh token that is unknown or was already revoked is **not** an error — sign-out is idempotent
from the caller's point of view.

## The error envelope

Every rejection the application raises has the **same shape**, so a client writes one parser and one
`switch` on `code`:

```json
{"error":{"code":"not_found","message":"…","requestId":"…"}}
```

`Error` requires `code` and `message`; `requestId` is always present in practice and is also
returned in the `X-Request-Id` response header. **Quote `requestId` when you report a failure** —
it is how a specific answer is found in the server's logs. If you send your own `X-Request-Id`, the
API honours it and echoes the same value back.

| Code | HTTP | Meaning |
| --- | --- | --- |
| `invalid_id` | 400 | A caller-supplied identifier was rejected (a separator, `..`, or an absolute path) |
| `invalid_request` | 400 | The body was rejected — unknown or missing field, or a payload that cannot be interpreted |
| `invalid_status` | 400 | A status value outside the accepted set |
| `invalid_multipart` | 400 | The multipart body parsed but its parts were not usable |
| `missing_file` | 400 | An upload carried no filename |
| `invalid_credentials` | 401 | The username is unknown **or** the password does not match — the two are deliberately not distinguished |
| `invalid_refresh_token` | 401 | The refresh token is unknown, expired, revoked, or already exchanged |
| `missing_token` | 401 | No `Authorization` header was sent |
| `invalid_token` | 401 | The header was sent but the token was not accepted |
| `token_expired` | 401 | The token was valid and is now past its expiry — refresh, or sign in again |
| `forbidden` | 403 | Authenticated, but the caller lacks the required role on the project |
| `not_found` | 404 | The identifier is valid but nothing is stored under it |
| `conflict` | 409 | The identifier already exists, or a document-level route is ambiguous |
| `storage_error` | 500 | Stored JSON could not be read or written |

**Every `401` carries a challenge header**, which is how a client can tell "no token" from "bad
token" without parsing the body:

```
www-authenticate: Bearer realm="Tucano Test API"
```

The `realm` is quoted. When the token itself was rejected rather than absent, the challenge adds
`error="invalid_token"`.

`openapi.json` names, per operation, which of these codes that operation can return — so a client
can be precise about what it handles without guessing. The envelope's field-by-field reconciliation
with the implementation is in
[docs/contracts/api-compatibility.md](../contracts/api-compatibility.md).

## Two answers that are not the envelope

Both of these come from the router or an extractor rather than from a handler, so neither uses the
envelope. A client that assumes every answer is JSON will misparse exactly these two:

| Answer | Body | Cause |
| --- | --- | --- |
| `413` | plain text `length limit exceeded` | The request body exceeded **50 MiB** |
| `400` | `text/plain`, mentioning `boundary` | The multipart extractor could not frame the body |

**The 50 MiB cap applies to every route and is checked before any handler runs**, so it is answered
plain text regardless of what the route normally returns — including on `POST /auth/login`. Send
large media through the attachment routes rather than inline in a document, and treat a `413` as
"retry smaller", not as "retry".

A body the multipart extractor cannot frame — an upload sent as `application/json`, or missing its
boundary — is answered `400 text/plain`. **Once the framing parses, upload rejections use the
envelope**, so `missing_file` and `invalid_multipart` arrive as JSON. The distinction is framing
versus content, and it is worth handling both.

## Swagger is the contract

| Endpoint | What it is |
| --- | --- |
| `http://localhost:3100/api-docs` | The interactive Swagger UI — every route, schema and status code, with *Try it out* |
| `http://localhost:3100/openapi.json` | The raw OpenAPI document |

Swagger UI is not a convenience copy of the contract: **`openapi.json` is the contract.** If a route
is not in it, it does not exist. When this page, the README, or any other prose disagrees with
`openapi.json`, the document wins and the prose is a bug.

Practically, that gives a client three things this page cannot:

- **Authoritative parameter and schema names**, down to which fields are required and which schemas
  refuse unknown properties.
- **The per-operation list of error codes**, so error handling is derived rather than assumed.
- **A runnable request.** Paste a token into the *Authorize* button once and Swagger sends it on
  every operation, which makes it the fastest way to check a payload before writing code.

The bearer scheme is declared once and applied globally, with the public operations marked
`security: []` — so if Swagger asks you for a token, the route needs one.

## A complete session

Copy-paste, with authentication on. It signs in, reads the caller, lists projects, and exercises one
deliberate failure to show the envelope:

```sh
BASE=http://localhost:3100

TOKEN=$(curl -s -X POST $BASE/auth/login -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"change-me-please"}' \
  | python3 -c 'import sys, json; print(json.load(sys.stdin)["accessToken"])')

curl -s $BASE/auth/me             -H "Authorization: Bearer $TOKEN"
curl -s $BASE/projects            -H "Authorization: Bearer $TOKEN"
curl -s $BASE/projects/nope.json  -H "Authorization: Bearer $TOKEN"
```

The third call answers `404` with the envelope, and the shape is the same one every other rejection
will use:

```json
{"error":{"code":"not_found","message":"…","requestId":"…"}}
```

Now drop the header and watch the difference between "absent" and "rejected":

```sh
curl -si $BASE/projects                        # 401 missing_token
curl -si $BASE/projects -H "Authorization: Bearer nonsense"   # 401 invalid_token
```

Both carry the challenge header, and both name their specific code in the envelope.

## Things that catch people out

| Symptom | Cause |
| --- | --- |
| `401 token_expired` on a client that worked minutes ago | The access token's lifetime elapsed. Refresh with `POST /auth/refresh`; do not retry the old access token |
| `401 invalid_refresh_token` right after a successful refresh | The refresh token was replayed — rotation revokes the one presented. Persist the replacement before making the next call |
| Every public route works but the first data call answers `401` | The token was not sent. The public routes need no header and succeed without one, which can mask a missing `Authorization` |
| A `403 forbidden` where you expected `404` | The route is project-scoped and your token does not reach the project. It is authorization, not a typo in the identifier |
| An empty listing where you expected data | Listings are **filtered** to the projects the token reaches. An empty list is a statement about permissions |
| A role you changed has no effect | The access token is stateless. The change lands when the token expires — shorten `TUCANO_ACCESS_TOKEN_TTL` if that window is too wide |
| `413` with a plain-text body | A request body over 50 MiB. It is answered before the handler runs, so it does not use the envelope |
| `{"error":{"code":"invalid_request"}}` on a payload copied from a page | The schema refuses unknown properties. Check the field against `openapi.json` rather than against prose |
| The password and the first call use different `curl` invocations and the token is empty | The login failed. Read the envelope — it is almost always `invalid_credentials` |
| `invalid_credentials` where the username is definitely right | The two causes are deliberately indistinguishable. Confirm the account exists in `$TUCANO_DATA_DIR/auth/` |

## Next

| I want to… | Read |
| --- | --- |
| Install the service and create a first project | [Installation and first project](getting-started.md) |
| Understand projects, suites, and cases | [Projects, suites, and cases](projects-suites-and-cases.md) |
| See how a client records and reports results | [Test runs and results](test-runs-and-results.md) |
| Check the contract itself | Swagger UI at `/api-docs`, or [`openapi.json`](../../openapi.json) |

---

*Sources of truth: [`openapi.json`](../../openapi.json) for every route, operation id, schema and
status code named here — including the global `bearerAuth` requirement, the eight `security: []`
operations, the `Error` envelope and its code enum, and the plain-text `413`; the
[README: Authentication](../../README.md#authentication) and
[README: Errors and request limits](../../README.md#errors-and-request-limits) for the variables and
the limits; [docs/security/authentication-decision.md](../security/authentication-decision.md) for
why the mechanism is a stateless JWT with project-scoped RBAC. Where this page and one of those
disagree, the source wins and this page is a bug.*
