# Shadow & Canary Validation and Rollback Runbook

Issue: [#96](https://github.com/TucanoTechnology/TucanoTestAPI/issues/96), split from
[#16](https://github.com/TucanoTechnology/TucanoTestAPI/issues/16). Phase 3 of the Rust migration
([`docs/architecture/rust-service-core.md`](../architecture/rust-service-core.md)) calls for the
service to be "shadow or canary validated with rollback to the current implementation". This runbook
is that procedure, plus the scratch-CRUD smoke check that gates a promotion.

It is an operations document: it adds no route, no field and no stored-document change.

## The deployment model this runbook assumes

- **One image.** The release workflow (`.github/workflows/release.yml`) publishes
  `ghcr.io/tucanotechnology/tucanotestapi` for every push to `main` and every `v*.*.*` tag, tagged
  with the immutable build tag `build-<run number>` and, for SemVer releases, `vMAJOR.MINOR.PATCH`
  (see *Release numbering* in `AGENTS.md`). Validation uses an existing immutable tag — a tag is
  never moved or reused.
- **A shared, writable data volume.** Every replica mounts the same persistent POSIX volume at
  `TUCANO_DATA_DIR` (the container default is `/data`); `docker-compose.yml` maps `./data:/data` on
  port `3100:3000`. A local Docker volume serves a single node; multi-node deployments must supply
  shared storage with working advisory locks (`README.md`, *Application container*).
- **Stateless replicas.** The process keeps no sessions or in-memory records, so a second replica can
  start beside the first without coordination. Mutations take an advisory lock file and write by
  atomic same-directory rename.
- **Hardened container.** Read-only root filesystem, `--tmpfs /tmp`, `no-new-privileges`, unprivileged
  user (uid 10001), `TUCANO_DATA_DIR=/data`, `PORT=3000`.

## Why canary, not shadow

A *shadow* runs the candidate beside production and discards its writes. This service cannot do
that: reads and writes both act on `TUCANO_DATA_DIR`, and the writes are the real ones — there is no
write-side branch to redirect. Pointing a shadow at the production volume would make its writes
visible.

A *canary* is therefore the mechanism: a second replica of the candidate image, mounted on the same
`TUCANO_DATA_DIR`, published on a port the stable replica does not use, and **not yet in the load
balancer**. Because replicas are stateless and mutations are lock-guarded, the canary can safely
serve real reads and writes. Step 1 only ever gives it the scratch CRUD of the smoke check, never
live traffic, until Step 3.

## Step 0 — record what is currently serving, and snapshot if the release changes stored shape

```sh
IMAGE="ghcr.io/tucanotechnology/tucanotestapi"
PREVIOUS="$IMAGE:build-4700"        # or the current vMAJOR.MINOR.PATCH tag
CANDIDATE="$IMAGE:build-4711"       # the build under test
```

Record `PREVIOUS` before touching anything; it is the rollback target.

If the candidate changes the shape, strictness, or validation of any stored document, snapshot the
volume first. Documents are plain JSON and inspectable on the host, so a filesystem snapshot or an
archive of the data directory is enough:

```sh
DATA_DIR=/srv/tucano/data           # the directory the stable replica mounts at /data
tar -C "$DATA_DIR" -czf "tucano-data-$(date +%Y%m%d-%H%M%S).tgz" .
```

See *What rollback guarantees about the shared volume* below for when this is mandatory.

## Step 1 — start the canary replica

Run the candidate image against the **same** data directory the stable replica uses, on a port the
stable replica does not publish:

```sh
docker run --detach --name tucano-api-canary \
  --read-only --tmpfs /tmp \
  --security-opt no-new-privileges:true \
  --env TUCANO_DATA_DIR=/data --env PORT=3000 \
  --volume "$DATA_DIR":/data \
  --publish 3101:3000 \
  "$CANDIDATE"
```

If the data lives in a named Docker volume rather than a host directory, pass the same volume name
the stable service uses (`--volume <volume-name>:/data`). Wait for the container to become healthy:

```sh
docker logs --follow --tail 20 tucano-api-canary
```

## Step 2 — health check and the smoke sequence

```sh
curl --fail --silent --show-error http://localhost:3101/health
# {"status":"ok","storage":"filesystem"}

scripts/smoke.sh http://localhost:3101
```

[`scripts/smoke.sh`](../../scripts/smoke.sh) performs a scratch CRUD round trip against the canary and
exits non-zero on the first deviation:

1. `GET /health` returns `200` with `status: ok`.
2. `GET /projects` returns a JSON array.
3. `POST /projects` creates a uniquely named scratch project (`smoke-<timestamp>-<pid>`) and returns
   `201` with the derived id `<name>.json`.
4. `GET /projects/{id}` returns the stored document, and `GET /projects` lists the new project.
5. `POST /projects/{id}/test_cases` creates a scratch case (`testCaseId`, `title`, `expectedResult`)
   and returns `201` with the case id.
6. `GET /test_cases/{caseId}` returns the case and `GET /projects/{id}/test_cases` lists it exactly
   once — the child is stored in the folder its parent owns.
7. `DELETE /test_cases/{caseId}` returns `200`; the subsequent `GET` returns `404` and the project's
   case list no longer contains it — the delete is observable, not a partial write.
8. `DELETE /projects/{id}` returns `200`; the subsequent `GET` returns `404` and `GET /projects` no
   longer lists it.

A `trap` removes both scratch resources on exit, so a failure at any step does not leave the volume
holding test data.

**What the smoke proves:** the candidate starts, answers the health endpoint, can list, create, read
and delete the project and case resources against the shared volume, and its deletes are durable
rather than partially applied.

**What it does not prove:** it exercises one resource family, not the whole surface (`openapi.json`
is the contract). It does not cover bulk reads, attachments, imports, runs, milestones or
configurations, and it does not measure performance. Promote when you are satisfied with the candidate
on the paths you changed; the smoke is a floor, not a full acceptance suite.

## Step 3 — promote

Once the smoke passes and the canary log is clean, point traffic at the candidate and keep `PREVIOUS`
recorded:

- **Load balancer / orchestrator:** add the canary replica to the backend pool, watch it, then remove
  the old replica.
- **Standalone `docker run`:** stop the previous container and start the same way with `$CANDIDATE` on
  the production port (`--publish 3000:3000` or the host mapping in use).
- **Compose:** pin the image tag in the deployment manifest and recreate the `api` service, then
  remove the standalone canary container:

  ```sh
  docker stop tucano-api-canary && docker rm tucano-api-canary
  ```

## Rollback

Rollback re-deploys `PREVIOUS` and re-runs the checks; it never moves a tag.

If the candidate was **never promoted** (it is still the standalone canary), remove it:

```sh
docker stop tucano-api-canary && docker rm tucano-api-canary
```

If it **was promoted**, redeploy the recorded previous tag:

```sh
docker stop tucano-api && docker rm tucano-api
docker run --detach --name tucano-api \
  --read-only --tmpfs /tmp \
  --security-opt no-new-privileges:true \
  --env TUCANO_DATA_DIR=/data --env PORT=3000 \
  --volume "$DATA_DIR":/data \
  --publish 3000:3000 \
  "$PREVIOUS"

curl --fail --silent --show-error http://localhost:3000/health
scripts/smoke.sh http://localhost:3000
```

With Compose, restore the previously pinned image tag and recreate only the `api` service:

```sh
docker compose up --detach --no-deps --force-recreate api
```

After a rollback, confirm `/health` and re-run `scripts/smoke.sh` against the restored port before
declaring the rollback complete.

## What rollback guarantees about the shared volume

Both images read and write the same `TUCANO_DATA_DIR`, so a rollback is only safe if the previous
build can still read what the candidate wrote. The authority is
[`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md); the relevant behaviour,
verified against the current source, is:

- **Reads and most writes are lenient.** A single-document `GET` reads the stored JSON as a raw
  value and returns it verbatim (assembling child collections from the folders), and `PUT`, the
  duplicate routes and all deletes read the stored document the same raw way. A document written by a
  newer build is therefore still **served, updated, duplicated and deleted** by an older one even when
  it carries a field the older build does not define.
- **Some paths are strict.** Operations that load a document as its typed model — every run and
  milestone operation (`TestRun`, `Milestone`, and the `TestSuite`, `TestCase` and `TestConfiguration`
  embedded into a run) — reject a field the model does not define: the models carry
  `deny_unknown_fields`, so an unknown field makes the load fail with `500 storage_error`
  ("Stored JSON is invalid"). Milestone progress, run composition, run results and defect linking all
  read this way.
- **Adding a stored field is a breaking change.** Because of the strict paths, the compatibility doc
  treats a new stored-document field as breaking and requires a versioning plan recorded there before
  implementation. Each additive field it records (run `results`, `defectLinks`, `tags`, step
  `attachments`) is written only when present, so documents persisted before the change still read.

The practical decision table:

| The release changed… | Rollback across it |
| --- | --- |
| only code and the HTTP contract — no stored-document field, no model strictness | Safe. Redeploy `$PREVIOUS`, check `/health`, run the smoke. |
| a stored-document field, a model's strict field set, or validation | **Not automatic.** Follow the versioning/migration plan recorded in `docs/contracts/api-compatibility.md`, and restore the Step 0 snapshot if that plan calls for it. |
| a new field that is written only when a client uses it | Safe for documents that never used it. A document that used it still reads on the previous build through `GET` and `PUT`, but any run or milestone operation over it answers `500 storage_error` — restore the snapshot or migrate before rolling back. |

Taking a Step 0 snapshot before any release in the second or third row is the cheap insurance that
makes the third row recoverable.

## Evidence to record per validation

- `PREVIOUS` and `CANDIDATE` — the exact immutable tags.
- Whether a volume snapshot was taken, and where.
- The `/health` response from the canary.
- The full `scripts/smoke.sh` output against the canary.
- Either the promotion decision or the rollback and its post-rollback smoke output.

| File | Role |
| --- | --- |
| [`scripts/smoke.sh`](../../scripts/smoke.sh) | Scratch-CRUD smoke check; exits non-zero on the first deviation. |
| [`docker-compose.yml`](../../docker-compose.yml) | Reference for the hardened container settings and the volume mount. |
| [`Dockerfile`](../../Dockerfile) | Image build, `TUCANO_DATA_DIR=/data`, `PORT=3000`, unprivileged uid 10001. |
| [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md) | Compatibility rules and the versioning plans that govern rollback. |
| [`docs/architecture/rust-service-core.md`](../architecture/rust-service-core.md) | Phase 3 exit criteria this runbook satisfies. |
