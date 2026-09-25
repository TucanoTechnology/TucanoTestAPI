# Off-Box Mirror and Backup Runbook

The data volume is the deployment's only state (see
[deployment-guide.md](deployment-guide.md)): everything the service knows — projects, suites, cases,
runs, milestones, configurations, attachments, and the auth store — lives under `TUCANO_DATA_DIR`
as plain JSON and files. This runbook is the procedure to keep a copy of it **off the machine that
serves it**, and to prove that copy restores. Epic #166's mirror boundary (#186's *external mirror*
row) is what this documents; the in-cluster options it does not cover are in
[storage-backends.md](../architecture/storage-backends.md).

## What the volume guarantees you

- Every published document is the product of an atomic write (same-directory temp, flush, rename),
  so no single file in a snapshot is torn, whatever moment the snapshot catches.
- Nothing below the data root is modified outside the API; an out-of-band tool never sees a half
  step the API takes, only its completed renames.
- There is, however, **no cross-file transaction**: a multi-document change (a copy, a
  duplication) is several renames, and a mirror that runs mid-change can catch the pair on
  opposite sides of it. A mirror is therefore *convergent* — one more pass always reaches the
  current state — and only a **quiesced snapshot** is a point-in-time one. This runbook uses both
  kinds deliberately.

## Roles

- **Gold copy (backup)**: one point-in-time snapshot per cycle, taken in a write-quiet window.
  This is the restore target for disaster recovery.
- **Live mirror**: continuous `rsync` of the volume to off-box storage, for near-zero data-loss
  objectives between gold copies. It inherits the convergence caveat above and is never the
  authoritative DR artifact.

## Prerequisites

- Off-box destination reachable over SSH, with roughly 2× the volume's size free per retained
  generation.
- `TUCANO_JWT_SECRET` (and any other secret values) stored **separately from both copies** — the
  volume holds only Argon2id password hashes and refresh-token digests (invariant 7 of the
  [threat model](../security/threat-model.md)), and a restore signed with a different secret
  invalidates live tokens; account passwords still authenticate because the hashes are in the
  copy.
- The image you will restore *with*: any `tucano-test-api` release tag or pinned image id, per
  [canary-validation-and-rollback.md](canary-validation-and-rollback.md). The data format is what
  makes old-new combinations safe (`formatVersion`,
  [file-format-versioning-plan.md](../contracts/file-format-versioning-plan.md)).

## 1. Gold copy — quiesced snapshot

The window is seconds; do it on a schedule the business will honour (a stopped replica, a
maintenance slot, or the last pass of a blue/green swap):

```bash
# with the API's write traffic stopped (compose down, or every replica stopped —
# one service still holding the advisory lock still means writes can land)
tar -C /srv/tucano/data -cf - . | ssh backup@offbox 'cat > /backups/tucano/gold-$(date -u +%Y%m%dT%H%M%SZ).tar'
ssh backup@offbox 'gzip /backups/tucano/gold-<stamp>.tar && zcat /backups/tucano/gold-<stamp>.tar.gz | tar -t >/dev/null && echo archive-ok'
```

`tar` streams the tree as it exists at read time — hence the quiet-window requirement for the
point-in-time property. The `.tucano.lock` file may or may not be present; it is advisory, empty,
and inert in a restore.

## 2. Live mirror — convergent copy

```bash
rsync -a --delete --omit-dir-times --noatime /srv/tucano/data/ backup@offbox:/backups/tucano/mirror/
```

Run it from cron/systemd timer as often as the data-loss objective demands. `--delete` keeps the
mirror honest about removals; the first pass after any interrupted run converges. Nothing on the
API side needs to pause for this path — a mid-write catch self-heals on the next pass, and
per-file atomicity means the interrupted view is whole documents, never corrupt ones.

## 3. Restore drill — verify the copy actually serves

A backup nobody has restored is a rumour. Quarterly, and after any change to the storage layout,
run the drill on the off-box host (or any machine with the image):

```bash
# 1) unpack the artifact into a fresh directory — never over a live volume
mkdir -p /drill/restored-data && zcat /backups/tucano/gold-<stamp>.tar.gz | tar -C /drill/restored-data -xf -

# 2) boot a second instance against it, on a spare port, with the same secrets
set -a; . /etc/tucano/env; set +a          # TUCANO_JWT_SECRET et al., from the secret store
TUCANO_DATA_DIR=/drill/restored-data PORT=3101 TUCANO_AUTH_REQUIRED=true \
  docker run --rm -p 3101:3000 -v /drill/restored-data:/data tucano-test-api:<tag> &

# 3) assert the restored copy answers like the live one did
curl -sf http://localhost:3101/health          # {"status":"ok","storage":"filesystem"}
curl -sf http://localhost:3101/openapi.json | sha256sum   # == the live /openapi.json digest
curl -sf -X POST http://localhost:3101/auth/login -H 'content-type: application/json' \
  -d '{"username":"<account>","password":"<password>"}'   # hashes travelled with the copy
# then the golden-path check: list projects (and any known attachment) and diff against the
# live deployment's listings taken the same day.
```

Pass criteria: health `ok`, the OpenAPI document byte-identical to the live one of the same
release, a successful login, and identical listings for the queries the drill cares about. Tear
the drill container down afterwards; its data directory is a scratch copy.

## 4. Retention and rotation

Keep at least one daily gold copy for 14 days and one monthly gold copy for 3 months, rotated by
stamp — the archive names above sort chronologically. The image registry already holds immutable
per-build and SemVer tags, so a *release* can always be restored with its era's data. Delete the
mirror nothing; it self-overwrites.

## What has been verified

The 2026-09-25 first drill executed exactly this sequence locally: quiesced `tar` snapshot of a
live volume (one project plus the auth store), restore into a fresh directory, boot a second
release-binary instance on a spare port with the same `.env` secrets, and assert `{"status":"ok"}`,
a byte-identical `/openapi.json`, a successful bootstrap login, and an identical `/projects`
listing against the source. Off-box transport (SSH) and the container form of step 2 are the
parts a local drill cannot exercise; treat the first off-box run as an extra drill.

## Related

- [deployment-guide.md](deployment-guide.md) — the volume model, scaling, and statelessness
- [canary-validation-and-rollback.md](canary-validation-and-rollback.md) — restoring the *image*
- [../architecture/storage-backends.md](../architecture/storage-backends.md) — the permitted
  external mirror vs the declined object-store backend (#186)
