# Tucano Test API

[![Workflows](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/lint.yml/badge.svg?branch=main&event=push&job=workflows)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/lint.yml)


[![Docs](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/docs.yml/badge.svg?branch=main&event=push&job=validate)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/docs.yml)

[![Rustdoc](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/build-test.yml/badge.svg?branch=main&event=push&job=rustdoc)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/build-test.yml)

[![Audit](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/security.yml/badge.svg?branch=main&event=push&job=audit)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/security.yml)

[![Release](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/release.yml/badge.svg?branch=main)](https://github.com/TucanoTechnology/TucanoTestAPI/actions/workflows/release.yml)

The Tucano Test API is the file-based test case management service for TucanoTCM: a Rust service
built on Axum that stores projects, suites, cases, runs, milestones, and configurations as JSON
documents on disk — no database — and exposes every operation through the documented HTTP contract
in [openapi.json](openapi.json).

> **Not a developer?** User documentation lives in the [wiki](docs/wiki/README.md) and is published
> to the repository's GitHub Wiki on every merge to `main`. AI agents should read
> [AGENTS.md](AGENTS.md) before making changes.

This README is the quick-start: clone, build, run, authenticate, call. Everything else
lives in the documentation tree linked at the bottom.

## Prerequisites

| Requirement | Version | Purpose |
| --- | --- | --- |
| Rust toolchain | `1.98.0` (pinned by `rust-toolchain.toml`) | Build and test the API |
| `rustfmt` and `clippy` components | Bundled with the pinned toolchain | Formatting and lint gates |
| Docker Engine | 24 or newer | Build and run the production container |
| Docker Compose plugin | v2 | Local deployment via `docker compose` |
| Node.js | 18 or newer (native `fetch` and ES modules) | Run the seed, teardown and validation scripts |
| `actionlint` (optional) | `1.7.12` | Validate workflow files locally |

Install the toolchain with [rustup](https://rustup.rs); `rust-toolchain.toml` pins the exact version
automatically. If you would rather not install Rust at all, every cargo command also runs inside
the pinned image:

```sh
docker run --rm -v "$PWD":/workspace -w /workspace rust:1.98.0-bookworm cargo test --all-targets --all-features
```

## Build and run

**With Docker (the shipped posture — authentication enforced):**

```sh
cp .env.example .env
# set TUCANO_JWT_SECRET (>= 32 bytes) and TUCANO_BOOTSTRAP_PASSWORD, then:
docker compose up -d --build api
```

`node -e 'process.stdout.write(require("node:crypto").randomBytes(32).toString("base64url"))'`
makes a good signing secret. Compose refuses to start while either required value is unset — it
never silently falls back to an anonymous stack. The compose stack's `gui` service builds from a
sibling `../TucanoTestGUI` checkout; `up ... api` alone needs neither it nor a browser.

**From source (no Docker):**

```sh
cargo build --release
TUCANO_AUTH_REQUIRED=false ./target/release/tucano-test   # anonymous, single-user machine only
```

`TUCANO_AUTH_REQUIRED=false` is the service default: every guard returns and no token is asked.
The API binds every interface, so only do this on a machine nothing else can reach — otherwise keep
authentication on (or restrict Compose to `127.0.0.1:3100:3000`). The container writes only under
its data volume (`/data`, from `./data` under Compose; create it owned by uid `10001`, since
startup otherwise refuses with a message naming both — issue #355 made that refusal the guide):

```sh
mkdir -p data && sudo chown 10001 data
```

What runs where:

| Service | Host port | Container port | Notes |
| --- | --- | --- | --- |
| `api` | `3100` | `3000` | this repository's `Dockerfile` |
| `gui` | `8080` | `8080` | sibling `../TucanoTestGUI` checkout |

The four request-ceiling knobs (`TUCANO_MAX_BODY_BYTES` 50 MiB, `TUCANO_REQUEST_TIMEOUT_MS` 300000,
`TUCANO_MAX_CONCURRENCY` 128, `TUCANO_LOCK_TIMEOUT_MS` 5000) validate at startup and refuse a
nonsense boot loudly rather than fail requests late; see the
[configuration reference](docs/deployment/configuration-reference.md). Before `docker compose up
--build` a second time, pin the running image id — local tags are overwritten by every rebuild; the
sequence is in the [rollback runbook](docs/deployment/canary-validation-and-rollback.md#rollback).

## Get a token and call the API

With authentication on (the compose default), guarded routes need a bearer token for a
project-granted role (`viewer` / `editor` / `owner`) or a system administrator:

```sh
TOKEN=$(curl -s http://localhost:3100/auth/login -H 'content-type: application/json' \
  -d '{"username":"admin","password":"<TUCANO_BOOTSTRAP_PASSWORD from .env>"}' | jq -r .accessToken)
curl -s http://localhost:3100/projects -H "authorization: Bearer $TOKEN"
```

Accounts and grants are provisioned out of band below `TUCANO_DATA_DIR/auth/` (the
binary's `seed-auth` subcommand writes a demo account); sign-ins, rotation and logout are
audited, and a refresh token is exchanged once. The role model, the session contract and every error code are in the
[API and authentication quickstart](docs/wiki/api-and-authentication.md) and the
[storage and API reference](docs/reference/storage-and-api.md); the decision record is
[authentication-decision](docs/security/authentication-decision.md).

## Endpoints that always help

| URL | What it is |
| --- | --- |
| `http://localhost:3100/api-docs` | interactive Swagger UI over the contract |
| `http://localhost:3100/openapi.json` | the authoritative contract itself |
| `http://localhost:3100/health` · `/ready` | liveness; readiness (writes actually work) |
| `http://localhost:3100/diagnostics` | a safe-to-share store probe — no paths, no secrets |
| `http://localhost:3100/metrics` | Prometheus counters, text format, unguarded by design |

The full route table is the [generated operations reference](docs/generated/operations-reference.md).

## Check locally before a PR

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
scripts/coverage-check.sh              # the whole suite + the per-operation contract
docker run --rm -v "$PWD:/repo:ro" --workdir /repo rhysd/actionlint:1.7.12 -color
```

```sh
cargo test --all-targets --all-features   # unit + integration
cargo test --lib                          # unit tests only
cargo test --test attachments             # one suite
```

Every CI job must pass locally before raising a PR; the suite-by-suite map, the coverage-table
mechanics and the CI caching design are in the
[testing guide](docs/testing/testing-guide.md). [CONTRIBUTING.md](CONTRIBUTING.md) and
[AGENTS.md](AGENTS.md) carry the full workflow rules.

## Documentation

- [docs/SUMMARY.md](docs/SUMMARY.md) — the complete, ordered index of this repository's docs tree
- [docs/wiki/README.md](docs/wiki/README.md) — the user wiki (also published to the GitHub Wiki)
- [docs/reference/storage-and-api.md](docs/reference/storage-and-api.md) — storage model, HTTP contract, authentication, errors, scaling
- [docs/deployment/deployment-guide.md](docs/deployment/deployment-guide.md) — containers, hardening, volumes, rollback
- [docs/deployment/logging-and-metrics.md](docs/deployment/logging-and-metrics.md) — spans, audit trail, `/metrics`
- [docs/testing/testing-guide.md](docs/testing/testing-guide.md) — suites and the operation-coverage contract
- [docs/security/audit-summary-s1-s4.md](docs/security/audit-summary-s1-s4.md) — the security audits, one page in
- [CONTRIBUTING.md](CONTRIBUTING.md) · [AGENTS.md](AGENTS.md) — human and agent contribution rules

The README deliberately stops here; the tree above takes it from here.
