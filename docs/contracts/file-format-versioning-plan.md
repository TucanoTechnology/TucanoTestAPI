# File-Format Versioning Plan (Issue #98)

Issue: [#98](https://github.com/TucanoTechnology/TucanoTestAPI/issues/98) — the durability step split from
[#16](https://github.com/TucanoTechnology/TucanoTestAPI/issues/16). It is the storage-format analogue of the
test-case versioning plan ([#89](https://github.com/TucanoTechnology/TucanoTestAPI/issues/89) /
[#90](https://github.com/TucanoTechnology/TucanoTestAPI/issues/90)): where that plan versions an individual
case's content, this one versions the **on-disk shape of a stored document**.

**The marker described here is not implemented.** No document on disk carries a `formatVersion` today, no
model declares the field, and no reader checks it. This issue is documentation only: no code, no schema, and
no stored document changes with it.

`docs/contracts/api-compatibility.md` remains the compatibility authority, and the *File-format versioning*
section the ticket asks for belongs inside it. That file is protected on the API (code) track, so that
section and its pointer back to this file are deferred to that track; the deferral is recorded on
[#98](https://github.com/TucanoTechnology/TucanoTestAPI/issues/98). Until that section lands, **this file is
the recorded plan**.

## Why a marker at all

A document's storage format is implicit today. Nothing written to `TUCANO_DATA_DIR` records which schema the
writer had in mind, so a reader can only tell two states apart by failing: it either deserialises a document
or it does not. That conflates two different situations:

- a document that is corrupt, hand-edited, or carries a key the model never declared; and
- a document that is perfectly valid for a **newer build** and simply describes a format this build does not
  know.

The models carry `deny_unknown_fields`, so both land on the same answer: `500 storage_error` "Stored JSON is
invalid". For the first case that is right. For the second it is an accident — the document is not invalid,
it is *from the future*, and the message tells the operator nothing about which build wrote it or what to do.

The marker's whole purpose is the second case, and it matters most in the one operation that puts two
different builds in front of the same volume: **rollback**. A release raises the on-disk format, writes
documents in it, and is then pulled back to the previous image — by the canary procedure in
[`docs/deployment/canary-validation-and-rollback.md`](../deployment/canary-validation-and-rollback.md), or by
the tag rules in [`docs/deployment/deployment-guide.md`](../deployment/deployment-guide.md). With no marker,
the older build's failure to read those documents is indistinguishable from corruption. With one, the reader
can say a specific, actionable thing: this document declares a format I do not support.

`formatVersion` is deliberately **not** the test-case `version`/`lastModified` pair. Those are per-case
revision numbers the API manages on the case document — plus the immutable `revisions/v{n}.json` snapshots a
qualifying update writes — and the plan that defines them is
[`docs/contracts/test-case-versioning-plan.md`](./test-case-versioning-plan.md); the routes that read them
belong to a later ticket. A caller reads those values. This field is a schema marker for one stored document:
not part of the logical resource, not surfaced by any route, and not something a client sets.

## The field (recorded decision)

Each persisted top-level document gains one optional field, in the model's camelCase wire shape:

| Field | Type | Default when absent | Written by |
| --- | --- | --- | --- |
| `formatVersion` | `Option<u32>` | `1` | the API only — never a client |

The Rust field is `format_version: Option<u32>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`,
and the reader's supported value is a named constant, `SUPPORTED_FORMAT_VERSION: u32 = 1`.

**Where it goes.** The six types that are persisted as a document in their own right:

| Model | Stored as |
| --- | --- |
| `Project` | `projects/<project>/project.json` |
| `TestSuite` | `projects/<project>/<suite>/suite.json` |
| `TestCase` | `<project or suite>/<case>/test-case.json` |
| `TestRun` | `test_runs/<id>.json` |
| `Milestone` | `milestones/<id>.json` |
| `TestConfiguration` | `configurations/<id>.json` |

A run is not only a document of its own: it embeds full `Project`, `TestSuite`, `TestCase` and
`TestConfiguration` snapshots, and a project and suite embed their children. So the same field also appears
inside a run document, on every embedded snapshot, because those are the same models. That is intentional —
one field on one model, wherever the model is written — and it is why a run is the document most likely to
carry a format a given build does not know.

**Where it does not go.** `Attachment`, `StepAttachment`, `TestStep`, `TestCaseResult`, `DefectLink`,
`ImportSummary`, `ImportCounts` and `MilestoneProgress` are auxiliary values, not documents with a storage
format of their own; they inherit the format of whatever contains them. A case's `revisions/v{n}.json`
snapshots are full case documents, so they carry whatever the writing build puts on a case.

**Absent means 1.** Reading a document without the key is exactly equivalent to reading one carrying
`"formatVersion": 1`, and the service must not rewrite a document merely because the key is missing. This is
the property that keeps a volume written by today's build readable by every build that predates the field:
no document changes shape just because a build that knows the field ran over it.

## The writer (recorded decision)

Two rules, both of which exist to keep rollback possible:

1. **The field is omitted while the document is at the current format.** The writer emits `formatVersion`
   only when a build has raised the format above `SUPPORTED_FORMAT_VERSION`; at version 1 the key is absent.
   This is what `skip_serializing_if` is for, and the consequence is the point: a document written by this
   build is shape-identical to one written by a build that never heard of the field, so it stays readable by
   both. Stamping `"formatVersion": 1` onto every write would do the opposite — it would add a key that every
   older build's `deny_unknown_fields` rejects, turning a routine upgrade into a one-way door and breaking
   the very rollback the marker is meant to protect.
2. **The API owns the value.** A create or update body that supplies `formatVersion` has no effect: the field
   is dropped on write, the same way [`#90`](https://github.com/TucanoTechnology/TucanoTestAPI/issues/90)
   treats the `version` / `lastModified` pair the API manages. Storing a client-supplied value would let a
   caller hand the service a document the service cannot read back, which is a denial of service on its own
   data. Note that the payload validator cannot make this call on its own: `check_against_model` only proves
   a supplied value has the right JSON type, so it accepts `"formatVersion": 2` and the *writer* must be what
   discards it.

## The reader (recorded decision)

| Value read | Behaviour |
| --- | --- |
| absent, or `1` | read normally — the document is at the supported format |
| any value above `SUPPORTED_FORMAT_VERSION` | refuse with `500 storage_error` and a message naming `formatVersion` and the supported value |
| a value of the wrong JSON type (for example `"formatVersion": "2"`) | refuse with `500 storage_error`, like any other stored document that does not match its model |

The refusal is intentionally **not** `404` (the document exists) and **not** `400` (the client did not send
this value — it is on disk). It is a server-side condition, and the stable envelope already has the right
member for it: `DomainError::Internal(message)` renders as `500` with code `storage_error`
(`src/api/error.rs`), the same code and status the reader answers today for a document that does not match
its model. The message must name the field and the supported version and must not carry a filesystem path —
the rule that internal paths never reach a client applies here as everywhere.

### Where the check must be applied

The service does not read every document the same way, and the check has to sit on the readers that
interpret a document as a model. Source-verified as of this writing:

| Site | What it loads | Needs the check |
| --- | --- | --- |
| `TestService::load_entity<T>` (`src/domain/service.rs`) | assembled reads — the project, suite and case paths | yes |
| `TestService::load<T>` (`src/domain/service.rs`) | flat reads — runs, milestones, configurations | yes |
| `milestone_progress`, milestone load (`src/domain/service.rs`) | a milestone, for progress | yes |
| `milestone_progress`, run load (`src/domain/service.rs`) | each referenced run, for progress | yes — and see below |
| `read_json` (`src/storage/fs.rs`), `merged_document` (`src/domain/service.rs`) | raw `Value` for single-document `GET`, `PUT` merge, duplicate, delete | no — raw paths by design |

`load_entity` and `load` are the natural home: both are already generic over `T: DeserializeOwned` and both
already turn a deserialise failure into `DomainError::Internal("Stored JSON is invalid")`, so a version check
can live beside that mapping once. The clean shape is a small trait implemented by the six models —
`fn format_version(&self) -> Option<u32>` — with the comparison performed in one shared helper both sites
call, so a model added later cannot silently skip it.

`milestone_progress` needs its own attention for a reason worth recording: it already loads a milestone and
its runs directly, and it currently **skips** a run it cannot deserialise
(`let Ok(run) = … else { continue }`). Left alone, an unsupported `formatVersion` on a run would make the run
quietly vanish from a milestone's progress numbers instead of failing — a wrong answer rather than an error,
which is exactly the failure mode a version marker is meant to prevent. The drill work is to route both
loads through the shared check so an unsupported format is refused, not silently dropped.

The raw paths are deliberately not given the check. A single-document `GET`, the `PUT` merge, duplicate and
delete work on the stored `Value` and copy it through without interpreting it, so they already serve,
update, duplicate and delete a document that carries keys they do not know. That leniency is a property of
the current implementation, not an oversight (it is recorded in the
[*What rollback guarantees about the shared volume*](../deployment/canary-validation-and-rollback.md#what-rollback-guarantees-about-the-shared-volume)
section of the promotion runbook), and this plan does not change it.

## Rollback drill matrix

This is the matrix [#101](https://github.com/TucanoTechnology/TucanoTestAPI/issues/101) asserts; it is
recorded here so the tests cite one specification rather than re-deriving it. It states what a build does
when it opens a document whose `formatVersion` it does not support.

| Document | Lenient paths (raw `Value`) | Strict paths (typed load) |
| --- | --- | --- |
| Project | `GET`, `PUT`, duplicate, delete — served and kept verbatim | run and milestone composition that assembles it — refused, `500 storage_error` |
| TestSuite | as above | as above |
| TestCase | `GET`, `PUT`, duplicate, delete, attachments, `revisions/` — verbatim | run and milestone composition that embeds it — refused |
| TestRun | `GET`, `PUT`, duplicate, delete — verbatim | results, import, suite/case composition, milestone progress — refused |
| Milestone | `GET`, `PUT`, duplicate, delete — verbatim | progress — refused |
| TestConfiguration | `GET`, `PUT`, delete — verbatim | use by a run — refused |

Two rollback outcomes follow, and only the first is safe without a migration step:

- **The release did not raise the format** (no `formatVersion` above 1 was written). Every document is at the
  supported format — most of them simply omit the key — so the older build reads all of them, on the lenient
  paths and the strict ones alike. Rolling back is complete.
- **The release raised the format.** The documents the newer build wrote carry a value the older build does
  not support: it still serves and deletes them verbatim on the lenient paths, but every run, milestone and
  composition operation that has to interpret them answers `500 storage_error`. That is the intended
  behaviour — an explicit refusal beats a misread — and it is why a format bump is not rollback-safe on its
  own: going back fully means running the migration the bump documented, rewriting the affected documents
  into the supported format, from the pre-change snapshot the promotion runbook takes.

## Migration rules (recorded decisions)

- **Additive only.** A new *optional* field on a model — the shape #22 (`results`), #90
  (`version`/`lastModified`), #49 (`tags`) and #93 (step attachments) all used — needs no `formatVersion`
  bump. It is still a compatibility event, because the legacy Draft 2020-12 schemas set
  `additionalProperties: false` and a legacy consumer that rejects unknown fields will refuse a document that
  carries the key; it must therefore be recorded under *Breaking change accounting* in
  `docs/contracts/api-compatibility.md`, but it leaves the format version alone: a build that predates the
  field reads the document fine because an optional field is absent, not unknown.
- **A bump is required for a change that makes a stored document mean something different**, or unreadable,
  to a reader that understood the previous format: removing or renaming a field, changing a field's type or
  its meaning, a change to what omission means, or a change to how strictly a stored document is validated.
  Those are the changes a reader cannot detect by failing loudly, which is what the marker is for.
- **A bump is a code decision, recorded before implementation.** The number is chosen in the plan entry in
  `docs/contracts/api-compatibility.md`, with the migration step that accompanies it, before any build writes
  it. A build only ever writes the version it implements, and only ever refuses versions above its own — it
  never coerces a document down, and there is no automatic downgrade.
- **Other documents are untouched by a bump.** Because the field is omitted at the current version, raising
  it changes only what the *new* build writes from then on; every document already on disk keeps its shape
  and stays readable by the build that wrote it, which is what makes the migration step bounded rather than a
  rewrite of the whole volume.

## Interaction with the compatibility contract

- The plan is the mechanism compatibility rule 3 asks for: "preserve file names and JSON formats during the
  evaluation; incompatible changes require explicit versioning". Today that rule is met by not making
  incompatible changes; with the marker it is met by making them *detectable*.
- `formatVersion` is itself an additive wire field whenever it is present, so it joins the *Breaking change
  accounting* list: a legacy consumer that rejects unknown fields will refuse a document a bumped build
  wrote. That entry belongs to the code track with the rest of the plan, in
  `docs/contracts/api-compatibility.md`.
- Nothing in this plan changes an error code, a route, a status, or a stored field's name. The only new
  observable behaviour a build implementing it would have is the refusal of a document it cannot know, and
  that refusal uses the existing `storage_error` envelope.

## What this issue delivers, and what is deferred

| Item | Where | Status |
| --- | --- | --- |
| Versioning plan (field, default, reader, writer, migration rules, drill matrix) | `docs/contracts/file-format-versioning-plan.md` | this file (new) |
| One row pointing here | `README.md` | this issue |
| *File-format versioning* section and the pointer back to this file | `docs/contracts/api-compatibility.md` | **deferred** to the API (code) track |
| `format_version` on the six models, with round-trip and absent-field unit tests | `src/models.rs` | **deferred** to the API (code) track |
| The reader refusal, through one shared check | `src/domain/service.rs` (or `src/storage/`) | **deferred** to the API (code) track |
| A test that a future `formatVersion` is refused cleanly | `tests/` | **deferred** to the API (code) track |

The four deferred items are all in files this backlog's ownership boundary protects, so they are recorded on
[#98](https://github.com/TucanoTechnology/TucanoTestAPI/issues/98) rather than opened as a branch that cannot
satisfy the Definition of Done.

## Files

| File | Change |
| --- | --- |
| `docs/contracts/file-format-versioning-plan.md` | This plan (new). |
| `README.md` | One documentation-table row pointing here. |
| `docs/contracts/api-compatibility.md` | **Deferred to the API track** — a *File-format versioning* section, the breaking-change entry, and a pointer to this file. Recorded on #98. |
| `src/models.rs`, `src/domain/service.rs` (or `src/storage/`), `tests/` | **Deferred to the API track** — the field, the reader check, and their tests. Recorded on #98. |
