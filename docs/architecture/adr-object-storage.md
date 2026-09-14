# ADR: Object Storage versus the File-Based Invariant

- **Status:** Accepted
- **Date:** 2026-09-14
- **Issue:** [#181](https://github.com/TucanoTechnology/TucanoTestAPI/issues/181) (child of epic [#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167))
- **Deciders:** repository owner (ECiurleo)
- **Supersedes:** nothing

## Context

Epic [#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167) proposes that a deployment be
able to keep its data in an S3-compatible object store instead of a shared POSIX volume. That is a direct
challenge to an invariant this repository states in several places, so the epic is blocked on a decision that
must be recorded before any code is written. This document is that decision.

### The invariant as it stands today

Three documents carry the same rule, and they agree:

- **`AGENTS.md` — Core Project Philosophy.** "Do not introduce databases, ORMs, or external persistence
  services." "Keep the API stateless so replicas can share the configured persistent storage." "Require
  shared storage with working advisory locks for multi-replica deployments; never use separate per-replica
  data volumes."
- **`AGENTS.md` — Docker & Deployment.** "Scale only with a shared persistent POSIX volume and
  advisory-lock support." "Storage mirrors the conceptual organisation."
- **`README.md` — Storage concept / Scaling.** "Tucano Test is a **file-based test case management system**:
  there is no database. All state is kept as folders and JSON files on the filesystem". "Repository mutations
  use an advisory lock file and atomic same-directory renames."
- **`docs/deployment/deployment-guide.md` — Scaling.** Multi-node deployments "must provide shared storage
  with working advisory locks"; per-replica volumes are forbidden because replicas would diverge.

### What the implementation actually relies on

The invariant is not aspirational; the current filesystem backend depends on POSIX semantics that an object
store does not offer. `src/storage/fs.rs` implements exactly two durability mechanisms:

1. **Atomic publish by same-directory rename.** `write_json` creates a temporary file beside the
   destination, writes the document, calls `sync_all`, then `fs::rename`s it over the target. A reader
   therefore observes either the old document or the new one, never a partial write. Rename-over-existing
   is atomic only on a filesystem that guarantees it.
2. **A single process-exclusive advisory lock.** `acquire_lock` opens `<data>/.tucano.lock` and takes
   `lock_exclusive()` (via `fs2`, i.e. `flock`). Every mutating path — write, delete, place, attachment
   save/delete, revision snapshot — is wrapped in it, so the read-modify-write sequences (for example,
   appending to a case's `attachments` array under the same lock as the file write) are serialised across
   processes, not just across threads.

On top of those two primitives the storage layer assumes, without ever stating it in one place:

- **Cheap directories.** Membership *is* the folder tree (`src/storage/layout.rs`): a project's suites are
  the folders inside it, a suite's cases likewise. Listing is `read_dir`. `place` with `copy` is a recursive
  directory copy; `move` is a rename, falling back to copy-then-delete.
- **Path confinement via the real filesystem.** `ensure_within` resolves and compares canonical paths so a
  symlink or `..` cannot escape `TUCANO_DATA_DIR`.
- **Rename and unlink semantics for identity.** Creating a second child with an existing name is a `409`
  because the folder already exists; deleting a project removes its whole subtree with `remove_dir_all`.
- **POSIX permission control.** `set_private_permissions` sets `0o666` on written documents; a test asserts
  it.
- **The volume is the state and is portable as-is.** The deployment guide's rollback story is "swap the
  image back and point it at the same volume"; a human can also inspect and hand-edit the JSON with no
  running service.

### Why S3 is a different thing, not another disk

An S3-compatible store is an **external persistence service** — it is a network service with its own
availability, credentials, consistency model, and failure modes, reached over HTTP. It is not a filesystem,
and the differences are not cosmetic:

- **Writes are not atomic publishes.** `PUT` of an object is atomic per object, but there is no
  rename-over-destination. A read-modify-write of a document is a read, a modification, and a put: between
  the read and the put another writer can interleave, and the loser's change is silently lost
  (last-writer-wins). Nothing in S3 makes the pair atomic.
- **There are no advisory locks to take.** `flock` has no S3 equivalent. The one mechanism that serialises
  cross-process mutations today simply does not exist. S3 has since gained conditional writes
  (`If-None-Match`, and in some implementations `If-Match`), but those are per-object compare-and-swap, not
  the multi-document, multi-step critical sections this code performs (a `place` touches a source folder,
  a target folder, and their markers; a delete touches a whole subtree).
- **There are no directories.** The conceptual tree is a key-prefix convention, not a structure. Listing is
  a paginated, eventually-consistent-by-default operation with its own cost and rate limits, not a `read_dir`.
- **`remove_dir_all` is a loop.** Deleting a subtree becomes an enumerate-then-delete-many-objects
  operation that can partially fail, leaving orphans — for which the current cascade semantics have no
  answer.

So the conflict is real and it is about **guarantees, not preference**: adopting S3 as the store means
giving up same-directory atomic publish, advisory locking, and cheap directory semantics, each of which is
load-bearing for a documented behaviour (`409` on collision, atomic attachment-plus-metadata update, cascade
delete, multi-replica correctness).

## Options considered

Each option is assessed against the properties this repository currently promises. "Filesystem" below means
a POSIX filesystem with working advisory locks and atomic rename.

| Property | What it means here | Today (filesystem) |
| --- | --- | --- |
| **Atomic writes** | A reader sees the old or the new document, never a mix | Same-directory temp + `sync_all` + rename |
| **Advisory locking** | Cross-process mutual exclusion for the whole mutation | `.tucano.lock` via `flock`, one lock for every write |
| **Multi-replica correctness** | N replicas on shared storage serve one consistent view | Guaranteed by the two mechanisms above |
| **Backup / restore** | A copy of the volume is a complete, restorable backup | `cp`/`tar`/snapshot of a directory; the volume *is* the state |
| **GUI contract** | GUI uses only the HTTP API and never touches storage | Unaffected by where storage lives |

### Option A — Decline and close

Keep the invariant exactly as written; do not admit object storage. Record that the file-based deployment on
a shared POSIX volume is the single supported persistence model, and close epic
[#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167) and its children
([#181](https://github.com/TucanoTechnology/TucanoTestAPI/issues/181)…[#186](https://github.com/TucanoTechnology/TucanoTestAPI/issues/186)).

- **Atomic writes:** unchanged — full guarantee.
- **Advisory locking:** unchanged — full guarantee.
- **Multi-replica:** unchanged — correctness rests on the documented shared-volume requirement.
- **Backup / restore:** unchanged — the volume is the backup; restore is a mount.
- **GUI contract:** unchanged — no API surface changes.
- **Cost:** a deployment that for organisational reasons cannot mount a shared POSIX volume (for example, a
  platform that only offers object storage) cannot run a multi-replica Tucano Test. That is a real
  limitation, but it is a *deployment constraint*, and the guide already tells such operators to use a
  single node with one shared volume.
- **Integrity:** keeps every promised guarantee with no new code, no new dependency, and no new failure
  mode.

### Option B — Object store as a replication or mirror target behind the filesystem

The shared volume stays authoritative and the only thing the service reads or writes. A separate process
(not the API) periodically or continuously copies the tree to an S3 bucket for backup, disaster recovery,
or read-only distribution. The API keeps talking to the filesystem and nothing in `src/` changes except, at
most, documentation and a sidecar.

- **Atomic writes:** unchanged in the service. The *mirror* may capture a torn view if it copies while a
  write is in flight; correctness of the mirror then depends on the copy tool (snapshot first, or copy
  temp-then-rename itself) and on its consistency guarantees, which must be stated per tool.
- **Advisory locking:** the service keeps `flock`; the replica tool must either take the same lock briefly
  or operate on a filesystem snapshot, otherwise it can copy a half-applied state.
- **Multi-replica:** unchanged — replicas still share the POSIX volume. The mirror is not a replica; it does
  not serve traffic.
- **Backup / restore:** *improved* — this is the option that actually answers the common motivation ("we
  want off-box backups"). The mirror is a second copy; restore means "copy the bucket back to a volume and
  mount it". This can be delivered without touching the invariant.
- **GUI contract:** unchanged.
- **Cost:** one operational component outside the API. It must be documented (what it copies, how often,
  how it stays consistent) or it becomes an untested claim.

### Option C — `Repository` backend abstraction with S3 as a documented non-default backend

Extract the `Repository` trait as the *only* persistence contract and implement an S3-backed
`Repository` alongside `FileRepository`. The filesystem remains the default and the reference
implementation; S3 is opt-in, documented, and explicitly weaker.

The trait already exists (`src/storage/mod.rs`), but today it encodes filesystem-shaped operations:
`list_children`, `place(mode)`, `save_attachment` (which must update the case marker *and* write a file
under one lock), `read_revision`, and so on. Making it backend-agnostic is not a rename:

- **Atomic writes:** S3 cannot offer same-directory atomic publish. A write is a `PUT`; the multi-document
  update the trait currently guarantees is no longer atomic. Either the contract is weakened to
  "per-object atomic only", or the S3 backend re-implements atomicity itself with versioning plus an index
  — which is a database in all but name and contradicts the philosophy more deeply than it satisfies it.
- **Advisory locking:** S3 has no `flock`. Cross-process mutual exclusion must be replaced by something
  else: conditional writes where the provider supports them, or an external lock service. Either way the
  "working advisory locks" guarantee is gone and must be replaced by a *different, weaker* documented
  guarantee.
- **Multi-replica:** correctness now depends on the replacement mechanism, not on POSIX. Without a real lock
  the safe configuration collapses to **single replica**, which must be enforced or loudly documented —
  otherwise two replicas lose writes silently, exactly the divergence the guide forbids today.
- **Backup / restore:** different model. "Restore" is bucket-versioning/object-lock policy, not "copy the
  volume". The runbook must be rewritten for this backend.
- **GUI contract:** unchanged — clients still only use the HTTP API. This is the one property that survives
  intact, because storage is already behind the trait.
- **Cost:** a second backend to implement, test, and keep at parity; a conformance suite that must pin
  *both* backends to one contract, whichever way the contract is weakened; and new failure modes (network
  partition, credential expiry, throttling, partial multi-object failure) that the current error envelope
  does not describe.
- **Integrity risk:** the highest. It is the option most likely to produce a system that *looks* like
  Tucano Test (files, JSON, no database) while having quietly lost the atomicity, locking, and multi-replica
  properties the repository says it has. If it is ever taken, those losses must be stated in the same
  breath as the feature.

### Option D — A separate deployment mode with its own stated guarantees

Ship object storage as a **distinct deployment mode** with a different, explicitly weaker contract, rather
than as a peer of the default backend. The build may share the trait, but the deployment, its limits, and
its guarantees are documented as their own thing — for example: "object-store mode: single replica, no
advisory locking, per-object atomicity only, no cascade-delete atomicity; use filesystem mode when replicas
or concurrent writers are required."

- **Atomic writes:** per-object only, stated up front.
- **Advisory locking:** not available; the mode's guarantee replaces it and is named.
- **Multi-replica:** fixed at one replica, enforced (or documented as unsupported) rather than left to
  chance.
- **Backup / restore:** provider-specific (versioning, object lock); documented per provider.
- **GUI contract:** unchanged.
- **Cost:** the same implementation cost as Option C, plus the discipline of two documented contracts and
  the risk that operators pick the weaker mode without reading the limits. Genuinely honest, but it is
  still new code and a new support surface for a capability no user has yet asked for.

## Decision

**Option A — decline object storage as a persistence backend — with Option B permitted strictly as an
external, out-of-process backup/mirror that the API never reads from or writes to.**

Concretely:

1. **The file-based invariant stands.** The server keeps exactly one persistence model: folders and JSON
   under `TUCANO_DATA_DIR`, atomic same-directory rename, and a process-exclusive advisory lock file. This
   ADR does **not** relax `AGENTS.md` or `README.md`; no wording in either is changed by this decision.

2. **No `Repository` backend abstraction is added for the sake of a second backend.** `Repository` remains
   an internal seam (it already exists and keeps the domain testable without a filesystem), not a pluggable
   storage-plugin surface with a published, weakened contract.

3. **An S3-compatible bucket MAY be used as a mirror or backup of the volume**, produced by tooling outside
   the API. The service never talks to it, so no application guarantee changes. Such a mirror is out of
   scope for this repository's code and, when documented, must state its consistency caveats (see Option B).

4. **Epic [#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167) and its coding children are
   cancelled** to match this decision, and the epic is closed as *declined by decision, not delivered*. The
   child tasks [#182](https://github.com/TucanoTechnology/TucanoTestAPI/issues/182),
   [#183](https://github.com/TucanoTechnology/TucanoTestAPI/issues/183),
   [#184](https://github.com/TucanoTechnology/TucanoTestAPI/issues/184),
   [#185](https://github.com/TucanoTechnology/TucanoTestAPI/issues/185) and
   [#186](https://github.com/TucanoTechnology/TucanoTestAPI/issues/186) are re-scoped as described in
   *Downstream tasks*.

### Rationale

- **The properties S3 cannot provide are the ones users depend on.** Atomic writes, advisory locks, and
  cascade-delete consistency are not implementation trivia; they are why a project with replicas behaves
  predictably. Trading them away to change *where bytes live* is a bad exchange.
- **The stated motivating problem is already solvable without relaxing anything.** "We want the data off
  a single host" is a backup question, and Option B answers it via an out-of-process mirror with zero loss
  of guarantees. The epic asked for a *store*, but the underlying need is usually a *copy*.
- **Admitting a weaker mode invites silent data loss.** Options C and D both end in a system that is
  file-shaped but not file-guaranteed. That is the worst outcome for a tool whose value proposition is
  "inspectable, portable, no database" — it would keep the honest appearance and drop the substance.
- **Reversibility.** Declining now costs nothing and can be revisited. An ADR is a record, not a
  prohibition with no way back; if a concrete deployment requirement appears with a use case that a mirror
  cannot meet, this decision can be superseded by a new ADR that takes Option D explicitly.
- **Constraint honesty.** The repository cannot serve a deployment that offers *only* object storage and
  *no* shared POSIX volume. That limitation is already implied by the deployment guide; this ADR makes it
  explicit rather than papering over it.

## Consequences

### What changes in `AGENTS.md` and `README.md`

**Nothing.** This is the deliberate outcome of Option A: the invariant is upheld, so neither document
requires an edit, and any edit that would relax it would require superseding this ADR first. For
completeness, if a future decision *were* to admit object storage, these are the exact relaxations it would
have to make — recorded here so the cost is visible now:

- **`AGENTS.md`**
  - *Core Project Philosophy:* replace "Do not introduce databases, ORMs, or external persistence
    services" with a rule that names an object store as an explicitly permitted, non-default persistence
    service and states its required guarantees.
  - *Core Project Philosophy:* replace "Require shared storage with working advisory locks for
    multi-replica deployments; never use separate per-replica data volumes" with a rule that allows a
    single-replica object-store deployment and states that multi-replica correctness is unavailable there.
  - *Docker & Deployment:* replace "Scale only with a shared persistent POSIX volume and advisory-lock
    support" with the per-backend scaling rule (filesystem: shared volume, N replicas; object store: one
    replica).
  - *Storage Security:* "Use atomic writes (same-directory temporary file, flush, rename) for all
    persistence" would have to become per-backend, since an object store cannot satisfy it.
- **`README.md`**
  - *Storage concept:* "there is no database. All state is kept as folders and JSON files on the
    filesystem" would have to name the backend and stop asserting the filesystem as the only store.
  - *Scaling:* the sentence "Repository mutations use an advisory lock file and atomic same-directory
    renames" would have to be qualified per backend, and the single-replica limit of the object-store mode
    stated.
  - *Documentation table:* link the new ADR describing each backend's guarantees.
- **`docs/deployment/deployment-guide.md`** would gain a per-backend scaling and backup/restore section,
  and **`docs/contracts/api-compatibility.md`** would need an entry only if the wire contract changed —
  which it would not, because the GUI contract is unaffected (it uses the HTTP API, never storage).

None of the above is done. It is recorded so that revisiting the decision is a bounded, well-understood
change rather than an exploratory rewrite.

### Downstream tasks (re-scoped)

| Task | Original intent | Disposition under this decision |
| --- | --- | --- |
| [#181](https://github.com/TucanoTechnology/TucanoTestAPI/issues/181) | This ADR | **Delivered** by this document |
| [#182](https://github.com/TucanoTechnology/TucanoTestAPI/issues/182) | Extract a backend-agnostic `Repository` boundary | **Cancelled** — not needed; `Repository` is already an internal seam and stays internal |
| [#183](https://github.com/TucanoTechnology/TucanoTestAPI/issues/183) | Implement the S3-backed `Repository` | **Cancelled** — declined by this decision |
| [#184](https://github.com/TucanoTechnology/TucanoTestAPI/issues/184) | S3 configuration and credential wiring | **Cancelled** — no S3 backend exists to configure |
| [#185](https://github.com/TucanoTechnology/TucanoTestAPI/issues/185) | Conformance tests for the storage backends | **Cancelled** — no second backend |
| [#186](https://github.com/TucanoTechnology/TucanoTestAPI/issues/186) | Document the storage backends | **Re-scoped** — becomes, if pursued, documentation of the *mirror* option (Option B): what a mirror may be, how to produce one consistently, and how to restore from it. Optional and out of scope here. |
| Epic [#167](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167) | Add an object-storage backend | **Closed as declined by decision** |

### Positive

- The invariant is upheld with no new code, dependency, failure mode, or support surface.
- The backup/mirror need is acknowledged and given a safe home (Option B) that does not touch the service.
- The decision, its alternatives, and the exact cost of reversing it are written down once, so the question
  is not reopened from scratch.

### Negative / accepted risks

- A deployment that cannot provide a shared POSIX volume cannot run a multi-replica Tucano Test. Accepted:
  such a deployment must run a single replica with one shared volume, as the deployment guide already
  requires.
- There is no built-in object-store backup. Accepted: off-box copies are produced by external tooling, and
  the service makes no claim about them.

### Follow-up (only if a concrete need appears)

If a deployment requirement arises that a mirror cannot satisfy, the path is a **new ADR** taking Option D
(a distinct, single-replica deployment mode with its own stated guarantees), not an in-place relaxation of
this one. Such an ADR must name the lost guarantees, the single-replica enforcement, and the per-provider
backup/restore runbook before any code lands.

## References

- [Epic #167 — Add an object-storage (S3) backend for the data volume](https://github.com/TucanoTechnology/TucanoTestAPI/issues/167)
- [Issue #181 — this ADR](https://github.com/TucanoTechnology/TucanoTestAPI/issues/181)
- [`AGENTS.md`](../../AGENTS.md) — Core Project Philosophy, Docker & Deployment, Storage Security
- [`README.md`](../../README.md) — Storage concept, Scaling
- [`docs/deployment/deployment-guide.md`](../deployment/deployment-guide.md) — scaling and rollback
- [`docs/architecture/rust-service-core.md`](./rust-service-core.md) — the repository trait and
  atomic-persistence design this ADR defends
- [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md) — the wire contract, unaffected
  by this decision
- `src/storage/fs.rs` — `acquire_lock`, `write_json`, `place_locked` (the mechanisms the invariant stands on)
