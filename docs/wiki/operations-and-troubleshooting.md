# Operations and troubleshooting

This page is for whoever runs Tucano Test: what has to be persistent, how to scale it, how to take a
backup and put it back, what the health endpoints actually answer, how to roll back a release, and
what to do when something has already gone wrong.

It assumes you have read [Installation and first project](getting-started.md). Every command is
copy-pasteable and states its expected result. Where this page and the engineering records disagree,
the records win and this page is a bug — the sources are named at the bottom.

## The data directory is the only state

Everything the service persists lives under one directory, `TUCANO_DATA_DIR` (`/data` in the
container). There is no database, no cache and no session store: a container holds nothing that must
survive a restart, and deleting a container never deletes data — deleting the directory does.

| Path | What it is |
| --- | --- |
| `./data` on the host | The persistent state, created by Compose on first run |
| `/data` in the container | The same directory, named by `TUCANO_DATA_DIR` |

Because the contents are ordinary JSON and folders, you can read, `grep`, snapshot and archive them
with the tools you already have — the API never needs to be involved in a backup. What you must not
do is **edit** them: the API is the only writer, and a hand-edited document is a corrupted one. If
you need to change stored data, change it through the API.

The tree, in one view:

```text
TUCANO_DATA_DIR/
├── projects/<project>/project.json     suites, cases and their attachments
│   ├── test_runs/<id>.json             runs live inside the project that governs them
│   ├── milestones/<id>.json
│   ├── configurations/<id>.json
│   └── …/                              suite and case folders
└── auth/                               accounts, project grants, refresh tokens
```

Since storage layout v3 a run, a milestone and a configuration each live **inside their project**
rather than in a directory at the root of the data directory. A volume written by an older build does
not start at all: the service refuses rather than serve a data set it can only partly read, and the
error names the counts it found and points at the deployment guide. The full layout is in the
[repository README](../../README.md#storage-concept) and its decision of record is
[docs/architecture/adr-storage-layout-v3.md](../architecture/adr-storage-layout-v3.md).

## Health, readiness, and diagnostics

Four unauthenticated endpoints answer the operational questions. They need no token by design — an
orchestrator probes a container before any credential could be presented, and a scraper has none at
all — and none of them reveals a path, a filesystem error or any stored content.

| Endpoint | Answers | Status |
| --- | --- | --- |
| `GET /health` | *Is the process serving?* Liveness only. | Always `200` while the process is up |
| `GET /ready` | *Can the store take writes?* Also checks the data directory and the advisory lock. | `200` ready, `503` not ready |
| `GET /diagnostics` | The same checks, reported individually, for an operator. | Always `200` — the report is not an error |
| `GET /metrics` | *How much has this deployment served?* Prometheus counters by method, resource and status class. | Always `200` — only counters, no stored content |

```sh
curl -s http://localhost:3100/health
```

```json
{"status":"ok","storage":"filesystem"}
```

Use `/health` for a liveness probe: it answers as soon as the process serves, and a `200` from it says
nothing about the store. Use `/ready` for a readiness probe, so an orchestrator stops routing to a
replica that cannot write:

```sh
curl -si http://localhost:3100/ready
```

Ready:

```json
{"status":"ready","storage":"filesystem"}
```

Not ready — `503` with the error envelope, naming which of the three checks failed:

```json
{"error":{"code":"not_ready","message":"storage is not ready: the data directory is not writable","requestId":"…"}}
```

The message is one of three sentences: *the data directory is missing*, *the data directory is not
writable*, or *the storage lock cannot be taken*.

`/diagnostics` is the operator's half — the same probe with each check reported separately, so a
failing deployment says which one failed rather than only that it did:

```sh
curl -s http://localhost:3100/diagnostics
```

```json
{
  "storage": "filesystem",
  "ready": true,
  "exists": true,
  "writable": true,
  "lockable": true,
  "lockHeld": false,
  "lastWriteUnix": 1758000000
}
```

| Field | Meaning |
| --- | --- |
| `ready` | All three of `exists`, `writable` and `lockable` — the same verdict `/ready` gives |
| `exists` / `writable` | The data directory is present, and the process may write to it |
| `lockable` | The advisory lock can be taken, so two replicas are not fighting over the same volume |
| `lockHeld` | `true` while another writer currently holds the lock. `true` *and* `lockable: false` together means a stuck writer |
| `lastWriteUnix` | When the store last accepted a write, in Unix seconds — a cheap "is anything happening" signal |

Neither `/ready` nor `/diagnostics` takes the lock for good: both probe it and release it, so a
healthy replica does not block a second one from starting.

## Scaling out

A second replica is the same image, the same environment, and the same `TUCANO_DATA_DIR`. Replicas
share no in-memory state, so adding one needs no coordination — but it needs shared storage:

- **Single node.** A local Docker volume or a host bind mount (`./data`) is fine, and more than one
  replica may share it because they are on the same host.
- **Multiple nodes.** The volume must be shared POSIX storage reachable from every node at the same
  mount path, **with working advisory locks**. Mutations take an advisory lock file and write by
  atomic same-directory rename; a filesystem that does not honour those locks lets two nodes lose
  each other's writes.
- **Never give each replica its own volume.** Per-replica local volumes diverge silently: each
  replica serves a different partial view of the data, and a create on one is invisible to the others.

If `/ready` reports *the storage lock cannot be taken* on a replica that used to be healthy, check
for a stuck writer on the shared volume before restarting anything — the diagnosis section below
covers it.

## Backup and restore

A backup is a directory copy. Nothing has to be quiesced for a *consistent* copy, but the process
writes by atomic rename, so take the copy while the service is stopped, or accept that a document
written during the copy may be captured mid-rename:

```sh
DATA_DIR=/srv/tucano/data                                  # the directory the replica mounts at /data
tar -C "$DATA_DIR" -czf "tucano-data-$(date +%Y%m%d-%H%M%S).tgz" .
```

With Compose, the same archive from the host side:

```sh
tar -C ./data -czf "tucano-data-$(date +%Y%m%d-%H%M%S).tgz" .
```

Restore is the reverse, into an empty directory, with the service stopped:

```sh
docker compose down
rm -rf ./data && mkdir -p ./data
tar -C ./data -xzf tucano-data-20260916-020000.tgz
docker compose up -d
```

Then confirm the service is serving *and* can write before declaring the restore good — a `200` from
`/health` alone does not prove the volume is usable:

```sh
curl -s http://localhost:3100/ready
SMOKE_USERNAME=admin SMOKE_PASSWORD='<the TUCANO_BOOTSTRAP_PASSWORD from .env>' \
  scripts/smoke.sh http://localhost:3100
```

`scripts/smoke.sh` creates a scratch project and case, reads both back, deletes both and confirms
each deletion. It needs `curl` and `python3`, and it always tries to remove what it created, so a
failing run does not leave scratch data behind. Since the shipped stack authenticates, it signs in
with `SMOKE_USERNAME`/`SMOKE_PASSWORD` (or accepts a ready-made `SMOKE_TOKEN`) and presents the
token on every request.

Two cautions: `rm -rf ./data` in the restore step is deliberate and destructive — it is why the
service is stopped first, and why the archive name carries a timestamp. And after restoring an older
archive, remember that any run or result recorded since the backup is **gone**, not merged; a run is
a point-in-time record, and the API will not reconstruct one from a later source.

### Off-box copies and object storage

The archive above is a copy on the same host. To get a copy *off* the host, an S3-compatible bucket
may be used as a mirror or backup of the data directory — but this is produced by tooling **outside**
the API, and the service never reads from it or writes to it. It is a copy, not a second backend:
there is no backend setting to select, and the API makes no claim about what the copy tool produced.
A copy taken while writes are in flight can capture a torn view, so the correctness of the mirror
rests on the copy tool's own guarantees (snapshot first, or copy temporary-then-rename, or take the
advisory lock briefly), never on this service. Restoring from a bucket means copying it back to a
directory and mounting that, exactly as the steps above do.

**No mirror runbook is published yet, because no mirror tooling is implemented in this repository.**
The supported and verified way to move a copy off the host today is to produce the archive above and
move *the archive*. The concrete recipe — which copy mechanism, how to keep it consistent, and the
restore drill — is tracked as issue
[#246](https://github.com/TucanoTechnology/TucanoTestAPI/issues/246) under epic
[#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167). The decision, the declined
backend, and the terms of the mirror permission are in
[Storage backends](../architecture/storage-backends.md) and
[the ADR](../architecture/adr-object-storage.md).

## Rolling back a release

Rollback means redeploying a previously recorded **immutable** tag — never moving a tag, never
rebuilding from a branch. Record what is serving before you change anything:

```sh
IMAGE="ghcr.io/tucanotechnology/tucanotestapi"
PREVIOUS="$IMAGE:build-4700"    # the tag currently deployed, or a vMAJOR.MINOR.PATCH release
CANDIDATE="$IMAGE:build-4711"   # the tag under test
```

Then point the deployment back at `$PREVIOUS` against the **same** `TUCANO_DATA_DIR`, and confirm
`/health`, `/ready` and `scripts/smoke.sh` before declaring recovery. The validated promotion and
rollback procedure, including the canary replica that gates a promotion, is
[docs/deployment/canary-validation-and-rollback.md](../deployment/canary-validation-and-rollback.md).

The shipped `docker-compose.yml` is the exception to "an immutable tag": it composes
`image: tucano-test-api:local` with `build: .`, so that name is a build output and `--build` retags
it. There the rollback target is the running container's image **id**, recorded and pinned under a
rollback-only tag *before* the candidate is built, and the Compose rollback recreates the service
from it with `--no-build`. The executable sequence is in the
[runbook's Rollback section](../deployment/canary-validation-and-rollback.md#rollback).

One rule is worth internalising before you roll back: **security fixes ship as patch releases and are
never rolled back**, because the previous image carries the flaw the patch closed. If the release you
are escaping contains a security fix, forward-fix instead.

Two compatibility cases decide whether a rollback is automatic:

- **Code and HTTP contract changed only.** Reverting the tag is enough — the stored documents are
  read the same way by both builds.
- **A stored document's shape, strictness or validation changed.** The rollback needs the versioning
  plan recorded in [docs/contracts/api-compatibility.md](../contracts/api-compatibility.md) and,
  usually, the pre-change snapshot. This is why you snapshot before a release that touches stored
  shape — it is cheap insurance, and without it the failure is not recoverable in place.

## Troubleshooting FAQ

Every rejection the application raises has the same envelope, so the `code` is what you switch on:

```json
{"error":{"code":"…","message":"…","requestId":"…"}}
```

### Compose refuses to start: a required variable is missing a value

Symptom: `docker compose up -d` prints a `error while interpolating services.api.environment.…`
line naming `TUCANO_JWT_SECRET` or `TUCANO_BOOTSTRAP_PASSWORD`, and no container is created — this
happens before the service is reached.

Cause: the shipped `docker-compose.yml` turns authentication on and declares both values required,
and the `.env` file Compose loads does not set them. The fix is not to weaken the file: create `.env`
from the committed template and set them.

```sh
cp .env.example .env
# set TUCANO_JWT_SECRET (at least 32 bytes) and TUCANO_BOOTSTRAP_PASSWORD
docker compose up -d --build
```

`docker compose config --quiet` validates the result without printing the secrets. Deliberately
unauthenticated runs set `TUCANO_AUTH_REQUIRED=false` in `.env` — only on a machine nothing else can
reach, see the [installation page](getting-started.md#authentication-is-on-by-default).

### The container exits immediately and the log says the data directory is unusable

Symptom: `docker compose up -d` reports `Exited`, and `docker compose logs api` names the data
directory, its writability, or the lock.

Diagnosis — ask the running service what it thinks, or inspect the volume directly if it will not
stay up:

```sh
docker compose logs api | tail -20
ls -ld ./data
```

Causes and fixes:

| Cause | Fix |
| --- | --- |
| The directory does not exist and the container cannot create it | Create it on the host: `mkdir -p ./data`. A bind mount does not create its source for you in every Docker version. |
| The directory is not writable by the container's user (uid `10001`) | `sudo chown -R 10001:10001 ./data`. A root-owned `./data` created by a stray `sudo mkdir` is the usual culprit. Since #355 the startup log says exactly this — it names the path and the uid — so `docker logs` is self-diagnosing. |
| A previously crashed writer left the advisory lock held | See *A replica reports the storage lock cannot be taken* below. |
| A volume written by a pre-v3 build | Not a fault: the service is refusing a legacy layout on purpose. See [Storage layout v3 and legacy volumes](../deployment/deployment-guide.md#storage-layout-v3-and-legacy-volumes). |

### `403 forbidden` on a project the operator can obviously see

Authentication is on and the caller reaches some projects but not this one. Grants are
project-scoped (`viewer`, `editor`, `owner`, plus a `systemAdmin` account that reaches everything),
and a caller reaches only the projects it was granted — listings are filtered down rather than
refused.

There is **no grant-administration route yet**: grants live under `$TUCANO_DATA_DIR/auth/`, so check
there and provision out of band. See the
[Authentication section of the README](../../README.md#authentication) and
[docs/security/authentication-decision.md](../security/authentication-decision.md).

### `401 missing_token` or `token_expired` on a call that used to work

Access tokens are short-lived (`15m` by default). Exchange the refresh token for a new pair:

```sh
curl -s -X POST http://localhost:3100/auth/refresh \
  -H 'Content-Type: application/json' \
  -d "{\"refreshToken\":\"$REFRESH\"}"
```

Refresh tokens rotate on every use, so the token you just spent is dead — store the new one. A
`invalid_refresh_token` answer means the token is unknown, expired, revoked or already exchanged;
log in again with `POST /auth/login`.

### `413` with the plain-text body `length limit exceeded`

The request body exceeded 50 MiB. This limit is enforced by the router before any handler runs, so it
answers plain text rather than the error envelope — that asymmetry is expected, not a bug. Reduce the
upload, or upload the attachment to a case or step rather than embedding it in a document.

### `500` with `{"code":"storage_error"}`

Stored JSON could not be loaded as the document it claims to be. The most common cause is a document
edited by hand outside the API: the legacy schemas are strict and carry `additionalProperties:
false`, so an unknown field is a load failure rather than something ignored.

```sh
curl -s http://localhost:3100/diagnostics
docker compose logs api | tail -40
```

Fix the document through the API rather than by hand, or restore the offending file from a backup.
Do not "repair" it by deleting fields you do not recognise. The lenient and strict read paths are
enumerated in *What the compatibility guarantees cover* in
[docs/deployment/deployment-guide.md](../deployment/deployment-guide.md).

### A replica reports the storage lock cannot be taken

`/ready` answers `503` with *the storage lock cannot be taken*, or `/diagnostics` reports
`lockable: false` with `lockHeld: true`.

A writer is stuck, or a previous process died holding the lock. First find it: `lockHeld: true` with
`lockable: false` on a replica you did not restart is the other replica doing real work — do not kill
it. If no process is writing and the lock is still held, the writer crashed; stop the replicas that
mount this volume, remove the stale lock file, and start one replica again.

Never delete a lock file while a replica that mounts the volume is running: you are removing the only
thing keeping two writers apart.

### The published container tag I expected is not in the registry

Every push to `main` publishes `build-<GitHub run number>`; a SemVer release additionally publishes
`vMAJOR.MINOR.PATCH`. Pull requests build and test but publish nothing, so a tag from a PR does not
exist by design. Ask the registry rather than the UI:

```sh
docker manifest inspect ghcr.io/tucanotechnology/tucanotestapi:build-4711 > /dev/null && echo present
```

If the workflow ran but the image is missing, read its log: the publish step logs in to GHCR with
`secrets.GITHUB_TOKEN` and requires `packages: write`
([`.github/workflows/release.yml`](../../.github/workflows/release.yml)), so a missing tag means that
step failed rather than that the tag was withheld. Tags are immutable and are never reused or
overwritten — if you need a different build, you need a different run, not a re-push.

### CI fails with a DNS or crates.io error

A registry or `static.crates.io` lookup was refused. This has bitten this project's CI before and is
almost always transient. Re-run the job; if it recurs, the workflow's cargo registry and git caches
are the mitigation already in place (the cargo registry volumes in `.github/workflows/build-test.yml` and `.github/workflows/lint.yml` mount them so a cold
container does not re-download every crate).

### A page in this wiki contradicts the running service

That is a documentation defect, not a configuration problem — please
[open an issue](https://github.com/TucanoTechnology/TucanoTestAPI/issues). Every page names the
engineering records it derives from, and `openapi.json` is normative for the HTTP contract:

```sh
curl -s http://localhost:3100/openapi.json | head -40
```

---

*Sources of truth: [docs/deployment/deployment-guide.md](../deployment/deployment-guide.md) for the
volume mount, scaling rules, container hardening and rollback;*
*[docs/deployment/canary-validation-and-rollback.md](../deployment/canary-validation-and-rollback.md)
for the promotion and rollback runbook;*
*[docs/security/threat-model.md](../security/threat-model.md),
[docs/security/authentication-decision.md](../security/authentication-decision.md) and
[docs/security/scanning-policy.md](../security/scanning-policy.md) for the security model;*
*[docs/contracts/api-compatibility.md](../contracts/api-compatibility.md) for the compatibility rules;*
*the [README](../../README.md) for the storage concept and the error envelope; and*
*[`openapi.json`](../../openapi.json) for every route, parameter and status code. Where this page and
one of those disagree, the source wins and this page is a bug.*
