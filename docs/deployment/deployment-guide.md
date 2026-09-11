# Deployment Guide

Issue: [#105](https://github.com/TucanoTechnology/TucanoTestAPI/issues/105), split from
[#15](https://github.com/TucanoTechnology/TucanoTestAPI/issues/15). This is the operator-facing
companion to the release engineering rules in [`AGENTS.md`](../../AGENTS.md) and to the
step-by-step promotion and rollback procedure in
[`docs/deployment/canary-validation-and-rollback.md`](canary-validation-and-rollback.md).

It is an operations document: it adds no route, no field and no stored-document change.

## What is deployed

Two containers, each built from its own Dockerfile — the API's from this repository, the GUI's from
the TucanoTestGUI repository:

| Service | Image | Dockerfile | Host port |
| --- | --- | --- | --- |
| API | `ghcr.io/tucanotechnology/tucanotestapi` (released); `tucano-test-api:local` (Compose build) | [`Dockerfile`](../../Dockerfile) | `3100` → container `3000` |
| GUI | `tucano-test-gui:local` | `../Tucano-Test-GUI/Dockerfile` | `8080` → container `8080` |

The API is the system of record and the only process that touches storage. The GUI is a client of
the same HTTP contract and never accesses the data directory directly
([`docs/architecture/gui-client-boundary.md`](../architecture/gui-client-boundary.md)).

Released images come from the release workflow
([`.github/workflows/release.yml`](../../.github/workflows/release.yml)), which pushes to
`ghcr.io/${{ github.repository }}` — that is, `ghcr.io/tucanotechnology/tucanotestapi` — on every
push to `main` and every `v*.*.*` tag, with two immutable tags:

- `vMAJOR.MINOR.PATCH` for a SemVer release (`type=ref,event=tag`);
- `build-<run number>` for every push (`type=raw,value=build-${{ github.run_number }}`).

The build number also fills the `BUILD_NUMBER` build argument, which the Dockerfile records as the
`org.opencontainers.image.version` label. Release tags and build numbers are never reused or
overwritten (see *Release numbering policy* in [`AGENTS.md`](../../AGENTS.md)); the commit SHA is the
audit identity. Pull requests build and test but publish no release artifact.

## The JSON volume mount is the only state

`TUCANO_DATA_DIR` is the entire persistent state of the service. The image sets it to `/data`
(`ENV TUCANO_DATA_DIR=/data`) and declares that path a volume (`VOLUME ["/data"]`); a deployment
mounts durable storage there. Everything the API persists lives beneath it as plain JSON documents
and folders — projects, suites, cases, runs, milestones, configurations, and their attachments —
and the container holds nothing that must survive a restart:

- **Containers are stateless and disposable.** The process keeps no sessions and no in-memory
  records, so `docker rm` and `docker run` against the same data directory returns the same service.
  Deleting a container never deletes data; deleting the data volume does.
- **The volume is inspectable.** Because the files are ordinary JSON, an operator can read, snapshot
  and archive them without the API. The layout is documented in the *Storage concept* section of
  [`README.md`](../../README.md) and in
  [`docs/architecture/rust-service-core.md`](../architecture/rust-service-core.md).
- **The port is not the state.** Host port `3100` (Compose) or `3000` (plain `docker run`) is where
  the API answers; which port it uses has no bearing on what is stored.

This is the property that makes both scaling and rollback possible: two containers pointed at the
same `TUCANO_DATA_DIR` see the same data.

## Compose configuration

`docker compose up -d --build` builds and starts both services from
[`docker-compose.yml`](../../docker-compose.yml):

```sh
docker compose up -d --build
```

The `api` service as checked in:

```yaml
services:
  api:
    build:
      context: .
      dockerfile: Dockerfile
    image: tucano-test-api:local
    environment:
      TUCANO_DATA_DIR: /data
      PORT: 3000
    ports:
      - "3100:3000"
    volumes:
      - ./data:/data
    read_only: true
    tmpfs:
      - /tmp
    security_opt:
      - no-new-privileges:true
    restart: unless-stopped
    deploy:
      replicas: 1
      resources:
        limits:
          cpus: "1.0"
          memory: 512M
```

`TUCANO_DATA_DIR=/data` and the `./data:/data` mount together are the whole persistence story: the
host directory `./data` is the state (Docker creates it on first run), and `.gitignore` excludes
`/data/*` — with an explicit carve-out for a `/data/.gitkeep` marker — so test data is never
committed.

> **Note on the ticket wording.** Issue #105 describes the Compose volume as
> `tucano-test-data` → `/data`. The checked-in file above actually bind-mounts the host directory
> `./data`, which is convenient for a single developer machine (the JSON is directly inspectable in
> the repository working copy). Both forms mount durable storage at `/data`; a deployment that
> prefers a managed named volume substitutes:

```yaml
    volumes:
      - tucano-test-data:/data
```

which requires the volume to exist (`docker volume create tucano-test-data`, or simply let Compose
create it) and moves where the data physically lives. Nothing else changes: the container contract
is the path `/data`, never the volume's name.

### Enabling authentication

Authentication is **off by default** (`TUCANO_AUTH_REQUIRED` defaults to `false`), so an existing
deployment behaves exactly as before until an operator opts in. Turning it on requires a signing
secret and, on a fresh volume, a bootstrap account:

| Variable | Purpose |
| --- | --- |
| `TUCANO_AUTH_REQUIRED` | `true` makes every guarded route require a bearer token; leave it unset or `false` to keep the historic behaviour. |
| `TUCANO_JWT_SECRET` | The HS256 signing secret, at least 32 bytes. Mutually exclusive with `TUCANO_JWT_SECRET_FILE`. |
| `TUCANO_JWT_SECRET_FILE` | Path to a file holding the secret (surrounding whitespace trimmed) — the preferred form here, so the secret is not visible in `docker inspect`. |
| `TUCANO_ACCESS_TOKEN_TTL` / `TUCANO_REFRESH_TOKEN_TTL` | Access-token and refresh-token lifetimes (defaults `15m` and `14d`). |
| `TUCANO_BOOTSTRAP_USERNAME` / `TUCANO_BOOTSTRAP_PASSWORD` | Set **together** to create the first `systemAdmin` account when the volume holds none; skip once the account exists. |

With `read_only: true` the secret file must be mounted read-only, for example
`--mount type=bind,src=/etc/tucano/jwt-secret,dst=/run/secrets/jwt-secret,readonly` plus
`--env TUCANO_JWT_SECRET_FILE=/run/secrets/jwt-secret`. Accounts and per-project grants are read from
`$TUCANO_DATA_DIR/auth/`; there is no grant-administration API yet, so provision that tree out of
band. See [docs/security/authentication-decision.md](../security/authentication-decision.md) and the
threat model's "Known limitations" before exposing the service beyond a trusted network.

## Container hardening

The Compose service and the standalone `docker run` examples in the promotion runbook both use the
same hardened shape, matching what the Dockerfile already does:

| Setting | Effect |
| --- | --- |
| `read_only: true` / `--read-only` | Root filesystem is immutable; only the data volume and `/tmp` are writable. |
| `tmpfs: /tmp` / `--tmpfs /tmp` | A writable scratch area for the few temporary files the process needs. |
| `security_opt: no-new-privileges:true` | A process can never gain more privilege than the container started with. |
| `USER tucano` (uid `10001`) | The runtime image creates a system user `tucano` with no login shell and runs the service as it. |
| `ca-certificates` only | The only package the runtime image installs is `ca-certificates`; the Rust toolchain stays in the build stage. |
| `deploy.resources.limits` | Per-replica CPU (`1.0`) and memory (`512M`) ceilings. |
| `restart: unless-stopped` | The container is restarted after a host reboot or crash unless an operator stopped it. |

A consequence worth internalising: because the root filesystem is read-only, the service must not
be given anything to write outside `TUCANO_DATA_DIR` and `/tmp` — and it does not need to. If a
change ever requires another writable path, that is a deployment-model change, not a container
tweak.

## Scaling out

A second replica is the same image, the same environment, and the same `TUCANO_DATA_DIR`. Because
replicas share no in-memory state, adding one needs no coordination — but it does need shared
storage:

- **Single node.** A local Docker volume or a host bind mount (`./data`) is fine, and `deploy.replicas`
  may exceed one only because the replicas share that one host directory.
- **Multiple nodes.** The volume must be shared POSIX storage reachable from every node at the same
  mount path, with **working advisory locks**. Repository mutations take an advisory lock file and
  write by atomic same-directory rename; if the filesystem does not honour those locks, concurrent
  writers on different nodes can lose data.
- **Never give each replica its own volume.** Separate per-replica local volumes diverge: each
  replica would serve a different partial view of the data, and a create would be invisible to the
  others. Use platform-provided shared storage (or a single node with one shared volume) instead.

Tune `deploy.replicas` and the resource limits per deployment; the checked-in values (`replicas: 1`,
`1.0` CPU, `512M`) are a local default, not a sizing recommendation.

## Rollback

Rollback means re-deploying the previously recorded **immutable** image tag — never moving a tag,
never rebuilding from a branch. The validated promotion-and-rollback procedure, including the canary
replica and the scratch-CRUD smoke check that gates promotion, is
[`docs/deployment/canary-validation-and-rollback.md`](canary-validation-and-rollback.md); this
section records the tag and compatibility rules that procedure depends on.

Record the tag that is serving before any change:

```sh
IMAGE="ghcr.io/tucanotechnology/tucanotestapi"
PREVIOUS="$IMAGE:build-4700"    # the tag currently deployed, or a vMAJOR.MINOR.PATCH release
CANDIDATE="$IMAGE:build-4711"   # the tag under test
```

To roll back:

1. Stop and remove the failing container (or point the Compose service back at the previously pinned
   tag) and start `$PREVIOUS` against the **same** `TUCANO_DATA_DIR`.
2. Confirm `GET /health` answers and run [`scripts/smoke.sh`](../../scripts/smoke.sh) against the
   restored port.
3. If the release changed a stored document's shape, strictness or validation, follow the versioning
   plan recorded in [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md), and
   restore the pre-change snapshot if that plan calls for it.

### What the compatibility guarantees cover

The authority is [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md). Its
rule that governs rollback: the legacy Draft 2020-12 schemas set `additionalProperties: false`, so
**adding a stored field is a breaking change** and requires a versioning plan recorded there before
implementation. Not every additive change is equally risky, though, because the service does not
read every document with the same strictness:

- **Lenient paths.** A single-document `GET` reads the stored JSON as a raw value and returns it
  verbatim (assembling child collections from the folders), and `PUT`, the duplicate routes and all
  deletes read the stored document the same raw way. A document written by a newer build is still
  served, updated, duplicated and deleted by an older one even when it carries an unknown field.
- **Strict paths.** Operations that load a document as its typed model — every run and milestone
  operation, including the suites, cases and configurations embedded into a run — carry
  `deny_unknown_fields`, so an unknown field makes the load fail with `500 storage_error`
  ("Stored JSON is invalid").

So a rollback is automatic only when the release changed code and the HTTP contract without touching
a stored document's field set, strictness or validation. Any release that did touch them needs the
recorded versioning/migration plan — and a pre-change snapshot, which is the cheap insurance that
makes the failure recoverable. The decision table and the snapshot command live in
*What rollback guarantees about the shared volume* in the promotion runbook.

## Deployment checklist

- [ ] Image tag identified: a `vMAJOR.MINOR.PATCH` release tag or an immutable `build-<run number>` tag.
- [ ] `TUCANO_DATA_DIR` (default `/data`) mounted on the intended durable storage for every replica.
- [ ] Container started with the hardened shape: read-only root filesystem, `/tmp` tmpfs,
      `no-new-privileges`, unprivileged user, resource limits.
- [ ] `GET /health` answers; the Swagger UI at `/api-docs` and `openapi.json` respond if the
      deployment exposes them.
- [ ] Multi-node deployments: shared storage with working advisory locks; no per-replica volumes.
- [ ] Authentication decided: either the historic `TUCANO_AUTH_REQUIRED`-unset shape, or a signing
      secret plus a provisioned `$TUCANO_DATA_DIR/auth/` tree.
- [ ] Rollback target (`PREVIOUS` tag) recorded, and a volume snapshot taken if the release changes a
      stored document's shape, strictness or validation.

| File | Role |
| --- | --- |
| [`docker-compose.yml`](../../docker-compose.yml) | The `api` and `gui` service definitions, volume mount, hardening and resource limits. |
| [`Dockerfile`](../../Dockerfile) | Image build, `TUCANO_DATA_DIR=/data`, `PORT=3000`, unprivileged uid 10001, `VOLUME ["/data"]`. |
| [`.github/workflows/release.yml`](../../.github/workflows/release.yml) | Publishes the immutable SemVer and `build-<run number>` tags to GHCR. |
| [`docs/deployment/canary-validation-and-rollback.md`](canary-validation-and-rollback.md) | Canary promotion, the scratch-CRUD smoke check, and the rollback procedure. |
| [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md) | The file-format and compatibility rules a rollback depends on. |
| [`scripts/smoke.sh`](../../scripts/smoke.sh) | Scratch-CRUD smoke check used to validate a canary or a restored build. |
