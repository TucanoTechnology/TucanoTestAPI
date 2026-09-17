# Installation and first project

This page takes you from a clean checkout to a running instance with a project, a suite and a case
in it. Every command below is copy-pasteable, and the expected output is stated so you can tell
whether you are on track.

Tucano Test is a **file-based** service: everything it stores lives as folders and JSON documents
under one data directory. There is no database to install, and nothing below that directory is
edited by hand — the API is the only writer. See the [repository README](../../README.md) for the
full storage concept.

## What runs, and where

The Compose file defines two containers:

| Service | Host port | Container port | Built from |
| --- | --- | --- | --- |
| `api` | `3100` | `3000` | this repository's `Dockerfile` |
| `gui` | `8080` | `8080` | a sibling `../Tucano-Test-GUI` checkout |

> **The port and the container port are different numbers, and that is expected.** Inside the
> container the API listens on `3000` (the `PORT` environment variable); Compose publishes it on
> host port `3100`. So the address you use from your machine is
> `http://localhost:3100`, and a plain `docker run` of the image without a port mapping listens on
> `3000` instead. Every URL below uses `3100`.

## Prerequisites

- **Docker Engine 24 or newer** and the **Docker Compose v2** plugin (`docker compose`, not the
  legacy `docker-compose`).
- A checkout of this repository.
- For the `gui` service only: a sibling checkout of TucanoTestGUI at `../Tucano-Test-GUI` relative
  to this repository. Without it, start the API alone.

Building the binary yourself additionally needs the Rust toolchain pinned by `rust-toolchain.toml`
(`rustup show` will select it). That is only necessary for local development — the container build
takes care of it for a deployment. The prerequisites table in the
[README](../../README.md#prerequisites) is authoritative.

## Start it

The stack authenticates by default, so it needs a signing secret and a bootstrap account before it
will start. Copy the committed template and set both values:

```sh
cp .env.example .env
# edit .env: set TUCANO_JWT_SECRET (at least 32 bytes) and TUCANO_BOOTSTRAP_PASSWORD
```

`docker compose` loads `.env` automatically from this directory, and the file is gitignored, so the
secret never reaches a commit. Generate one with
`node -e 'process.stdout.write(require("node:crypto").randomBytes(32).toString("base64url"))'`. A
missing `TUCANO_JWT_SECRET` or `TUCANO_BOOTSTRAP_PASSWORD` stops Compose with an error that names
the variable, rather than starting an anonymous stack.

Then build and start:

```sh
docker compose up -d --build
```

Expected: Compose builds both images and prints `Started` for each container. The first build
compiles the Rust crate, so it takes a few minutes; subsequent starts are seconds.

If you do not have the GUI checkout, start the API alone:

```sh
docker compose up -d --build api
```

Either form creates a host directory `./data` on first run and bind-mounts it at `/data` inside the
container. That directory **is** the state.

## Check that it is up

```sh
curl -s http://localhost:3100/health
```

Expected — HTTP `200` with a JSON body:

```json
{"status":"ok","storage":"filesystem"}
```

Two more endpoints are worth knowing immediately:

| Endpoint | What it gives you |
| --- | --- |
| `http://localhost:3100/api-docs` | The interactive Swagger UI — every route, schema and status code, with a *Try it out* button |
| `http://localhost:3100/openapi.json` | The raw OpenAPI document |

Swagger is not a convenience copy of the contract; `openapi.json` **is** the contract. If a route is
not in it, it does not exist. When this page and Swagger disagree, Swagger wins.

## The data directory is the only state

| Path | What it is |
| --- | --- |
| `./data` on the host | The persistent state, created by Compose on first run |
| `/data` in the container | The same directory, named by `TUCANO_DATA_DIR` |

Because the contents are ordinary JSON, you can read, snapshot and archive them without the API.
Deleting the container never deletes data; deleting the directory does. The container's root
filesystem is read-only and only `/data` and `/tmp` are writable, so it has nowhere else to put
anything and nothing else to lose.

Test data is never committed: `.gitignore` excludes `/data/*`.

## Authentication is on by default

The shipped `docker-compose.yml` sets `TUCANO_AUTH_REQUIRED=true`, so the container you just started
already guards every route except the public ones. The `.env` values are what it uses:
`TUCANO_JWT_SECRET` signs the tokens and `TUCANO_BOOTSTRAP_USERNAME`/`TUCANO_BOOTSTRAP_PASSWORD`
create the first `systemAdmin` account — but only when the data directory holds no accounts at all,
so it is a one-time step and harmless to leave in place (change the password afterwards, or remove
the two variables).

The service's own default is `false`, so a deployment that supplies its own container definition and
does not set the variable runs anonymously. **That is only safe on a machine nothing else can
reach.** The API is published on every interface (`3100:3000`), so if you want an unauthenticated
stack that is still reachable from elsewhere, restrict the publish to loopback instead — change the
mapping to `127.0.0.1:3100:3000`. To turn authentication off in this stack, set
`TUCANO_AUTH_REQUIRED=false` in `.env`.

With authentication on, a guarded route without a token answers `401` with a challenge:

```sh
curl -si http://localhost:3100/projects
```

```
HTTP/1.1 401 Unauthorized
www-authenticate: Bearer realm="Tucano Test API"

{"error":{"code":"missing_token","message":"Authentication required","requestId":"…"}}
```

Log in and keep the access token:

```sh
TOKEN=$(curl -s -X POST http://localhost:3100/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"<the TUCANO_BOOTSTRAP_PASSWORD from .env>"}' \
  | python3 -c 'import sys, json; print(json.load(sys.stdin)["accessToken"])')
```

The login response carries `accessToken`, `refreshToken`, `tokenType` (`Bearer`) and `expiresIn` in
seconds. Send the token on every guarded call:

```sh
curl -s http://localhost:3100/auth/me -H "Authorization: Bearer $TOKEN"
```

```json
{"id":"…","username":"admin","systemAdmin":true,"roles":{}}
```

`roles` is the caller's project-scoped grants (`viewer`, `editor`, `owner`), and `systemAdmin`
reaches everything. There is no grant-administration route yet: grants live in
`$TUCANO_DATA_DIR/auth/` and are provisioned out of band. Access tokens are short-lived (`15m` by
default); `POST /auth/refresh` exchanges the refresh token for a new pair, and refresh tokens are
rotated on every use.

The variables, the RBAC rules and the public routes are described in full in the
[Authentication section of the README](../../README.md#authentication) and decided in
[docs/security/authentication-decision.md](../security/authentication-decision.md).

Every setting above can instead come from an optional JSON file named by `TUCANO_CONFIG_FILE`, read
once at startup. Precedence is resolved per key — environment, then file, then the built-in default
— and a file that is unreadable, unparseable or carries an unknown key is a startup failure rather
than a silent fallback. The template is
[docs/deployment/config.example.json](../deployment/config.example.json) and the model is decided in
[docs/security/configuration-decision.md](../security/configuration-decision.md). The environment
variables on this page keep working unchanged whether or not a file is used.

## Create your first project

The walkthrough below runs against the authenticated stack, so log in first (above) and append
`-H "Authorization: Bearer $TOKEN"` to each call.

### 1. Create a project

```sh
curl -s -X POST http://localhost:3100/projects \
  -H 'Content-Type: application/json' \
  -d '{"name":"My First Project"}'
```

Expected — `201 Created`:

```json
{"id":"My First Project.json","message":"Resource created"}
```

Note the shape: a create answers `{"message": …, "id": …}`, and the `id` is exactly the value the
member routes accept. The identifier was derived from the name because `projectId` was omitted; you
may supply your own, as long as it is a single path segment ending in `.json`.

### 2. List projects

```sh
curl -s http://localhost:3100/projects
```

```json
["My First Project.json"]
```

### 3. Create a suite inside the project

A suite always lives inside a project. The parent is named in the **route**, not in the body:

```sh
curl -s -X POST "http://localhost:3100/projects/My%20First%20Project.json/test_suites" \
  -H 'Content-Type: application/json' \
  -d '{"name":"Login Suite"}'
```

```json
{"id":"Login Suite.json","message":"Test suite created"}
```

> Identifiers contain spaces here, so URLs must percent-encode them (`%20`). The `"id"` value the
> API returned is the raw identifier; you encode it when you put it in a URL.

### 4. Create a case inside the suite

A case needs both an identifier and a title — `title` is the displayed name, `testCaseId` the
handle. Either a project or a suite can own a case; here it goes in the suite:

```sh
curl -s -X POST "http://localhost:3100/test_suites/Login%20Suite.json/test_cases" \
  -H 'Content-Type: application/json' \
  -d '{
        "testCaseId": "Login rejects a bad password.json",
        "title": "Login rejects a bad password",
        "expectedResult": "An error message is shown"
      }'
```

```json
{"id":"Login rejects a bad password.json","message":"Test case created"}
```

### 5. Read it back

Reading the project assembles the suites and the cases it directly owns:

```sh
curl -s "http://localhost:3100/projects/My%20First%20Project.json"
```

```json
{
  "name": "My First Project",
  "projectId": "My First Project.json",
  "testSuites": [
    {
      "name": "Login Suite",
      "suiteId": "Login Suite.json",
      "testCases": []
    }
  ]
}
```

Note that the suite marker's `testCases` is empty even though the suite has a case: **membership is
the folders on disk, not an array in the document.** Reading the suite itself assembles them:

```sh
curl -s "http://localhost:3100/test_suites/Login%20Suite.json"
```

```json
{
  "name": "Login Suite",
  "suiteId": "Login Suite.json",
  "testCases": [
    {
      "expectedResult": "An error message is shown",
      "lastModified": "…",
      "testCaseId": "Login rejects a bad password.json",
      "title": "Login rejects a bad password",
      "version": 1
    }
  ]
}
```

The `version` stamp and `lastModified` are written by the API; a client-supplied value is ignored.

### 6. Clean up

```sh
curl -s -X DELETE "http://localhost:3100/test_cases/Login%20rejects%20a%20bad%20password.json"
curl -s -X DELETE "http://localhost:3100/test_suites/Login%20Suite.json"
curl -s -X DELETE "http://localhost:3100/projects/My%20First%20Project.json"
```

Or let `scripts/smoke.sh` do a whole scratch round trip for you — create a project and a case, read
both back, delete both and confirm each deletion. It needs `curl` and `python3`, and defaults to a
different port than Compose publishes, so pass the base URL. The stack authenticates, so hand it the
bootstrap credentials from `.env`; it signs in through `POST /auth/login` and presents the token on
every request:

```sh
SMOKE_USERNAME=admin SMOKE_PASSWORD='<the TUCANO_BOOTSTRAP_PASSWORD from .env>' \
  scripts/smoke.sh http://localhost:3100
```

## Where to go next

| I want to… | Read |
| --- | --- |
| Understand the project / suite / case hierarchy properly | [Projects, suites, and cases](projects-suites-and-cases.md) |
| Make my first authenticated call, or read the error contract | [API and authentication quickstart](api-and-authentication.md) |
| Deploy, back up, scale, or roll back | [Operations and troubleshooting](operations-and-troubleshooting.md) |

## Troubleshooting the first run

Every rejection the application raises has the same envelope, so the `code` is what you switch on:

```json
{"error":{"code":"…","message":"…","requestId":"…"}}
```

| Symptom | Cause |
| --- | --- |
| `413` with the plain-text body `length limit exceeded` | The request body exceeded 50 MiB. This is the router's limit, so it is checked before any handler runs and it answers plain text rather than the envelope. |
| `400` with a plain-text body mentioning `boundary` | The multipart extractor could not frame the body — for example an upload sent as JSON. Once the framing parses, upload rejections use the envelope. |
| `{"code":"invalid_id"}` | The identifier was rejected, for example because it tried to escape the data directory with `..` or an absolute path. |
| `{"code":"not_found"}` | The identifier is valid but nothing is stored under it. |
| `{"code":"conflict"}` | Something already exists at that identifier. |
| `{"code":"missing_token"}` / `{"code":"token_expired"}` | Authentication is on and the token is absent or stale — log in again. |
| `{"code":"forbidden"}` | You are authenticated but do not hold the required role on the project. |
| `500` `{"code":"storage_error"}` | Stored JSON could not be loaded — see [Operations and troubleshooting](operations-and-troubleshooting.md). |

A `401` also carries a `WWW-Authenticate: Bearer realm="…"` challenge. The published codes,
per-operation, are listed in the Swagger UI and in
[docs/contracts/api-compatibility.md](../contracts/api-compatibility.md).

---

*Sources of truth: [docs/deployment/deployment-guide.md](../deployment/deployment-guide.md) for the
Compose configuration, the volume mount and container hardening; the
[README](../../README.md) for the storage concept, the prerequisites and the authentication
variables; [`openapi.json`](../../openapi.json) for every route and schema. Where this page and one
of those disagree, the source wins and this page is a bug.*
