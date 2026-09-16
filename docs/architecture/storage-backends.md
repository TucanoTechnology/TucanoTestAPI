# Storage Backends

Issue: [#186](https://github.com/TucanoTechnology/TucanoTestAPI/issues/186), child of epic
[#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167). The decision this document
records and explains is
[`docs/architecture/adr-object-storage.md`](adr-object-storage.md)
(issue [#181](https://github.com/TucanoTechnology/TucanoTestAPI/issues/181), **Accepted**).

This is a reference page: it states which storage backend the service has and what that choice
implies for backups, scaling, and rollback. It adds no route, no field, and no stored-document
change. Where this page and the ADR disagree, the ADR is the decision of record and this page is a
bug.

## There is exactly one backend, and the ADR declined to add a second

**The service has one persistence backend: the filesystem.** Folders and JSON documents under
`TUCANO_DATA_DIR`, atomic same-directory rename, and a process-exclusive advisory lock file. Nothing
else is implemented, and nothing else is planned.

That is a decision, not an accident of scheduling. Epic [#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167)
proposed an S3-compatible object store as an alternative backend and was **declined** by the ADR
above. The question the epic asked — "may a deployment keep its data in object storage?" — has been
answered **no**, on the grounds that the properties object storage cannot provide (atomic
multi-document publish, advisory locking, cheap directories) are precisely the properties this
service's documented behaviour rests on. It was declined rather than deferred: the epic and its
coding children are cancelled, not rescheduled.

Two consequences follow, and they are the reason this page exists:

- **Backend selection is not a thing you configure.** There is no backend key in the configuration
  file and no environment variable that switches the store. `TUCANO_DATA_DIR` names *where* the
  filesystem backend is rooted; it does not choose a backend.
- **A deployment that offers only object storage and no shared POSIX volume cannot run this
  service.** That is a genuine, accepted limitation rather than a gap to be worked around. The
  supported answer is a single node with one shared volume.

The backend identifies itself over HTTP. `GET /health`, `GET /ready` and `GET /diagnostics` all
report `"storage": "filesystem"`, and `openapi.json` publishes that field as a single-value enum:

```sh
curl -s http://localhost:3100/ready
{"status":"ready","storage":"filesystem"}
```

The value is a literal in the handler, not a reflection of configuration, so it is a statement about
the build rather than about a setting an operator chose. `GET /ready` is the useful one: it answers
`200` only when the data directory behind it takes writes, and `503` when it does not — see
[Operations and troubleshooting](../wiki/operations-and-troubleshooting.md) for the probe wiring.

## What the filesystem backend actually guarantees

These guarantees are what the ADR defends, and each one is load-bearing for a documented behaviour
rather than an implementation detail. They are stated here so an operator can judge what a future
change would cost, and so the mirror discussion below has something concrete to measure against.

| Property | Mechanism | What depends on it |
| --- | --- | --- |
| **Atomic publish** | A same-directory temporary file (`.tucano-<suffix>.tmp`), flushed and synced, then renamed over the destination. A reader sees the old document or the new one, never a partial write. | Every write. Also why a backup can be taken from a running service without quiescing it (see below). |
| **Cross-process mutual exclusion** | One exclusive advisory lock on `<data>/.tucano.lock`, taken by every mutating path — write, delete, place, attachment save/delete, revision snapshot. Reads do not take it. | Multi-replica correctness, and the attachment-plus-metadata update that must never diverge. |
| **Membership is the folder tree** | A project's suites are the folders inside it; a suite's cases likewise. Listing is a directory read. | The parent-scoped routes and the "one real home" rule in the storage concept. |
| **Cascade delete** | Deleting a project or suite removes its whole subtree, including attachments and revision snapshots. | The delete semantics the compatibility contract describes. |
| **Path confinement** | Every user-supplied identifier and filename is validated, and the constructed path is checked so a symlink or `..` cannot escape the data root. | The storage security invariants. |
| **The volume is portable as-is** | The files are ordinary JSON and folders. | Rollback ("point the old image at the same volume") and hand inspection with no running service. |

Two limits of these guarantees are worth naming plainly, because they are what the declined
alternatives would have traded away:

- **The advisory lock is a POSIX lock.** It is only a correctness guarantee on a filesystem that
  honours it. Shared storage that silently ignores advisory locks is not a supported multi-replica
  configuration, and the failure it produces is silent data loss rather than an error.
- **Atomicity is per document, and the lock is what makes the multi-document sequences atomic.**
  A composition that touches a source folder, a target folder and their markers is atomic because
  the whole sequence runs under the one lock — not because any single operation is.

## Operational implications

### Backup and restore

**A copy of the data directory is a complete, restorable backup**, and this needs no API
involvement, no export route, and no quiescing: because writes publish by atomic rename, a reader —
including a copying process — sees either the old document or the new one. The procedure, with
copy-pasteable commands and the restore-side cautions, is
[Backup and restore](../wiki/operations-and-troubleshooting.md) in the operations guide. Two points
from it belong in this page's scope:

- **Restore means mounting a copy of the directory at `TUCANO_DATA_DIR`.** There is no import step
  and no format to convert.
- **A restored archive supersedes; it does not merge.** Any run or result recorded after the backup
  is gone, not reconciled, because a run is a point-in-time record the API will not reconstruct
  from a later source.

### Scaling and multi-replica correctness

The process is stateless, so a second replica is the same image and the same environment. What it
also needs is **shared** storage:

- **Single node.** A local volume or a host bind mount is fine.
- **Multiple nodes.** The volume must be POSIX storage reachable from every node at the same mount
  path, with working advisory locks. This is the requirement the backend's correctness rests on.
- **Never per-replica volumes.** They diverge silently, and a create on one replica becomes
  invisible to the others.

The full scaling section is in
[docs/deployment/deployment-guide.md](../deployment/deployment-guide.md).

### Rollback

Reverting the image tag against the **same** data directory is the rollback, and it is automatic
only when the release changed code and the HTTP contract without touching a stored document's shape,
strictness or validation. A release that changed *where* documents live is the other case and needs
the pre-change snapshot — see *Rolling back across storage layout v3* in the deployment guide, and
[`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md) for the field-change
rules.

### Cost and consistency caveats

The honest summary of this backend's trade-offs, stated as the ADR requires:

- **Costs nothing beyond the volume you already mount.** No network service, no credentials, no
  per-request or per-byte charges, and no dependency whose availability becomes the service's
  availability.
- **Consistency is strong and local** — one writer at a time, atomic publishes — at the price of
  requiring a POSIX filesystem with real locking.
- **Durability is the operator's.** The service makes no claim about the storage underneath it. A
  volume that is not replicated, snapshotted or backed up is a volume whose loss is the loss of the
  data; see the mirror permission below for the sanctioned way to get a copy off the host.

## Object storage: declined as a backend, permitted as an external mirror

Both halves of this are decided in the ADR and are reproduced here so the operational position is
findable without reading a decision record. **They are two different things and the difference
matters.**

**Declined — object storage as a persistence backend.** The service will not read or write an
object store. No S3 client exists in the codebase, the `Repository` trait keeps a single
implementation and stays an internal seam rather than a pluggable plugin surface, and the ticket
that would have extracted a backend-agnostic boundary was cancelled along with the rest of the epic.
A consequence an operator will meet directly: a platform that offers object storage but no shared
POSIX volume cannot host a multi-replica Tucano Test.

**Permitted — an S3-compatible bucket as an out-of-process mirror or backup of the volume.** A copy
of the tree may be produced to a bucket by tooling *outside* the API. Because the service never
talks to it, no application guarantee changes: atomicity, locking and multi-replica correctness are
all still provided by the filesystem, and the bucket is a copy rather than a peer of the volume.

Three rules bind that permission, and none of them are optional:

1. **The mirror is out of process.** It is not a replica. It does not serve traffic, and the API
   never reads from it or writes to it. Restoring from a mirror means copying the bucket back to a
   volume and mounting that — the service has no knowledge of the bucket either way.
2. **Its consistency caveats must be stated wherever it is documented.** A copy tool run against a
   live volume can capture a torn view if it copies while a write is in flight; the correctness of
   the mirror depends on the copy tool's own guarantees (snapshot first, or copy
   temporary-then-rename, or take the advisory lock briefly), not on anything this service provides.
   The ADR records this as a requirement in the same breath as the permission, which is why a mirror
   that is merely mentioned — without its caveats — is not a complete statement of the position.
3. **It is not this repository's code.** Anything that implements the mirror lives outside this
   repository, and the service makes no claim about the copy it produces.

**No mirror runbook is published here, because no mirror tooling is implemented in this repository.**
Documenting a procedure nothing implements would be a guarantee the project cannot back. The
concrete recipe — which copy mechanism to use, how to keep it consistent, and the restore drill that
proves an archive is usable — is tracked as its own task, issue
[#246](https://github.com/TucanoTechnology/TucanoTestAPI/issues/246), a child of epic
[#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167), so it can be written and
verified against a real implementation rather than asserted in advance. Until that lands, the
supported and verified way to move a copy off the host is the directory-copy procedure in
[Backup and restore](../wiki/operations-and-troubleshooting.md), which needs no extra tooling and no
network service.

## What a future backend would have to answer for

Recorded for the same reason the ADR records it: so revisiting the decision is a bounded,
well-understood change rather than an exploratory rewrite. Admitting object storage as a real
backend would require an explicit answer to each of these, and the two weaker shapes it could take
(a pluggable `Repository` with an S3 implementation, or a distinct single-replica object-store
deployment mode) are worked through in the ADR's Options C and D.

- **Atomicity**, if same-directory atomic publish is unavailable — per-object only, or re-implemented
  with versioning and an index at the cost of the philosophy it is meant to serve.
- **Mutual exclusion**, if there is no advisory lock — a replacement mechanism, and a named,
  weaker guarantee that supersedes "working advisory locks".
- **Multi-replica agreement**, if correctness no longer rests on POSIX — up to and including
  enforcing a single replica in the weaker mode.
- **Backup and restore**, which becomes a provider-specific versioning and object-lock policy rather
  than a directory copy.
- **The document and code relaxations**, exactly as enumerated in the ADR's *What changes in
  `AGENTS.md` and `README.md`* section — that list is the change surface, and it is deliberately
  written down there rather than here so there is one copy of it.

Any such change must **supersede the ADR first**. It is not an implementation detail to be decided
while coding, which is precisely why the epic was blocked on the decision rather than on the work.

## References

- [ADR: Object Storage versus the File-Based Invariant](adr-object-storage.md) — the decision of
  record, its Options A–D, and the exact relaxations a reversal would require
- [Epic #167 — Add an object-storage (S3) backend for the data volume](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167)
- [Issue #186 — this document](https://github.com/TucanoTechnology/TucanoTestAPI/issues/186)
- [Issue #246 — the off-box mirror/backup runbook](https://github.com/TucanoTechnology/TucanoTestAPI/issues/246)
  — the spin-out task for the recipe this page deliberately does not publish
- [`AGENTS.md`](../../AGENTS.md) — Core Project Philosophy, Docker & Deployment, Storage Security
- [`README.md`](../../README.md) — Storage concept, Scaling
- [Deployment guide](../deployment/deployment-guide.md) — the volume as the only state, scaling,
  rollback, and the legacy-layout refusal
- [Operations and troubleshooting](../wiki/operations-and-troubleshooting.md) — backup and restore,
  health and readiness, the troubleshooting FAQ
- [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md) — the wire and
  file-format contract, unaffected by the backend decision
- [ADR: storage layout v3](adr-storage-layout-v3.md) — where runs, milestones and configurations
  live, and the rollback consequence of that layout change
- `src/storage/fs.rs` — `acquire_lock`, `write_json`, `place_locked`: the mechanisms the guarantee
  table above describes
- `src/api/mod.rs` — the `"storage": "filesystem"` literal the health, readiness and diagnostics
  handlers report
