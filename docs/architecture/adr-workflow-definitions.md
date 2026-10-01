# ADR: Workflow definitions — a named, ordered plan that points at live cases

- **Status:** Accepted
- **Date:** 2026-10-01
- **Issue:** [#463](https://github.com/TucanoTechnology/TucanoTestAPI/issues/463) (gating TucanoTestGUI[#101](https://github.com/TucanoTechnology/TucanoTestGUI/issues/101))
- **Deciders:** repository owner (ECiurleo)
- **Supersedes:** nothing

## Context

The GUI's Phase 4 epic item is a canvas for building test flows (TucanoTestGUI#101), and the data model
has no object for it. Today:

- a **run** freezes a selection of suites and cases at creation — embedded document snapshots and
  `caseVersions` pins, which is exactly what an execution record needs and what a reusable plan must
  not be;
- a **suite** is membership, not order: the folder holds case files and a listing that sorts by
  identifier;
- a **milestone** aggregates runs but sequences nothing.

There is nowhere to persist "run these eleven checks, in this order, reusing suite S's current
contents" as a template. The canvas cannot render what the store cannot hold, so this decision comes
first. This ADR decides the resource, its storage, its surface and its relationship to runs; it
deliberately contains no implementation, and it gates one build ticket here and the GUI canvas after
it.

### The invariants this ADR must not disturb

- **Files are the truth** (`AGENTS.md`, Core Project Philosophy). Every rule below is enforceable
  against a hand-edited tree, because that is the only kind of tree there is.
- **Storage layout v3** ([`adr-storage-layout-v3.md`](adr-storage-layout-v3.md)): project-scoped
  resources live under `projects/<p>/<collection>/<id>.json` — the placement milestones,
  configurations and (project-held) cases already use.
- **Reads are global, writes may be parent-scoped** — the addressing convention that
  [#462](https://github.com/TucanoTechnology/TucanoTestAPI/issues/462) just completed for cases.
- **Runs are immutable snapshots** — a run must never reach back into a workflow after materialising.

## Decision

### 1. The resource

A **workflow** is a project-scoped document naming an ordered list of references to live cases and
suites. Sketch (a workflow stored at `projects/checkout.json/workflows/release-smoke.json`):

```json
{
  "format": 3,
  "workflowId": "release-smoke",
  "name": "Release smoke path",
  "description": "The checks release day runs, in order.",
  "steps": [
    { "testCaseId": "TC-LOGIN-1" },
    { "suiteId": "smoke.checkout.json" },
    { "testCaseId": "TC-CHECKOUT-1" }
  ],
  "tags": ["release"]
}
```

The rules that give the sketch its meaning:

- **Order is array position.** No per-step `order` integer: a `PUT` replaces the whole `steps` array
  the way a run update replaces `projects`, and a canvas drag-and-drop is an array reorder — one
  representation, no reconciliation between an array and its own index fields.
- **A step references, never copies.** Exactly one of `testCaseId` or `suiteId` per step. The target
  is read *live* at materialisation time; the definition carries no case content, so editing a case
  needs no workflow fan-out. Runs stay the only freeze.
- **Identity follows the milestone rule:** `name` required, identifier derived as `<name>.json`,
  `workflowId` accepted-but-derived, the whole document `additionalProperties: false` like every
  strict schema in the contract.
- **References must be real at save time.** Every `testCaseId` must be a case the home project
  reaches (directly or through one of its suites, the same reach a coverage report uses); every
  `suiteId` must be a suite of the home project. A dangling reference is `400 invalid_request`
  naming the offending step index. Files stay the single truth: the API refuses to *store* a plan it
  cannot execute, but never re-validates old ones — a case deleted under a saved workflow makes that
  workflow fail *at run time* with the same `404`, which is the honest order of dependency.
- **Cycles are impossible** in this shape (a flat list of pointers), which is one reason branching is
  out of scope below.
- **`steps` must hold at least one entry and at most 512** — the emptiness rule of milestones
  ("a milestone that references none is `invalid_request`") and the shared reference cap, both
  reused rather than reinvented.

### 2. Runs

`POST /projects/{id}/workflows/{workflow_id}/run` (optionally with a body `{ "name": "...", "timestamp": "..." }`)
materialises the workflow **as it and its targets currently read**:

1. expand suite steps into the cases their folders hold right now;
2. keep definition order; a case reached twice keeps its first position (the flat-list rule again);
3. create the run through the *existing* parent-scoped run creation — the same embedded snapshots,
   the same `caseVersions` pins captured per case's live `version`, the same projects-reached guard;
4. stamp the new run with `sourceWorkflowId` — an optional field on `TestRunCreateRequest` and
   `TestRunUpdateRequest`, additive, following the `defectLinks` (#460) precedent for schema growth.

Provenance is a string, not a link to follow: the run answers everything about itself, and
`sourceWorkflowId` exists so the GUI can say *where this came from* — and so a later
`GET /projects/{id}/workflows/{wid}/runs` (listing, not contract-required, add it when a consumer
exists) has something to match. Nothing about materialisation mutates the workflow; running a plan
is reading it.

Execution *control* — result-recording gates, stop-on-failure, pause/resume — is the runner's job and
there is no runner: results are recorded through the run routes exactly as today. A workflow with a
`gate` field today would be a field nothing reads. When a real need appears it is its own ADR,
because it is a semantic change to what "steps" means.

### 3. The surface

Mirrors the milestones/configurations shape so the GUI's generic patterns (parent-scoped create and
list, bare read/update/delete, duplicate) apply without a new vocabulary:

| Route | Role | Notes |
| --- | --- | --- |
| `GET /projects/{id}/workflows` | `viewer` | listing, like `GET /projects/{id}/milestones` |
| `POST /projects/{id}/workflows` | `editor` | the only create; the flat one answers `400` naming this route, the retired-door convention |
| `GET /workflows/{id}` | `viewer` in the home | bare global read |
| `PUT /workflows/{id}` | `editor` | partial merge; an identity field naming another document is refused as everywhere |
| `DELETE /workflows/{id}` | `editor` | bare delete, resolved through the single home; workflows are never duplicated into second homes, so `409` is unreachable here in practice and still documented, as for milestones |
| `DELETE /projects/{id}/workflows/{workflow_id}` | `editor` | parent-scoped delete door, like milestones' |
| `POST /workflows/{id}/duplicate` | `editor` | composition copy of the *document*: steps are references, so the copy may name targets its own project does not reach — see below |
| `POST /projects/{id}/workflows/{workflow_id}/run` | `editor` | every project the materialised run reaches, the existing run-creation rule verbatim |

- **Write role is `editor`, not `owner`**: a workflow is project *content* like cases and suites.
  Milestones crown projects (`owner`) because they aggregate across them; a workflow cannot reach
  outside its project by reference (save-time validation, §1) so `editor` is the honest bar.
- **Duplication is the one place a reference can strand**: copying `release-smoke.json` into another
  project produces a workflow whose steps dangle. The duplicate route answers `409`-free but
  `invalid_request`-loud: duplicate succeeds and the copy is saved **only if** every target the
  source's steps name is reachable in the destination too — otherwise the duplicate is refused with
  the offending step indexes, telling the editor to re-point the copy rather than silently storing a
  broken plan. A future "re-map targets" option on the duplicate body is additive.
- Identifiers collide across projects the same way milestones' collide, and the contract's
  verbatim-address + single-home rules (§row 22 of the
  [seed spec](../testing/seed-dataset-spec.md)) apply unchanged: derived ids are unique
  (`<name>.json` under one project) and the bare routes resolve globally.

### 4. Storage

`projects/<p>/workflows/<id>.json` — a fourth project-scoped collection folder beside `milestones/`
and `configurations/`, layout v3's established grammar (`src/storage/layout.rs` gains the child
segment; `Resource::Workflows` joins the enum). Workflow folders do not exist: a workflow owns no
children, no attachments and **no revisions** — it is a pointer list, cheap to edit, and history that
nobody has asked to read is storage debt. The seed dataset grows one row (a workflow per demo
project, steps naming seeded suites and cases) and the validator one assertion pair, in the build
ticket.

### 5. Deliberately out of scope

- **Branching, gates, parallelism** — the canvas starts linear (ordered steps). A graph is a new
  resource shape with cycles, reachability and per-node state, and it changes materialisation
  semantics; it needs its own ADR when a real need appears, as §2 says of gates.
- **Cross-project workflows** — the home project bounds the reach. Multi-project execution is what a
  *run* is for, and a run can already span projects; a workflow whose steps could name foreign
  targets would break the save-time-reachable rule with a live dependency `editor` cannot audit.
- **Workflow-run binding beyond provenance** — no re-runs, no schedules, no version pinning of the
  workflow itself (§1: the definition reads live, by design).
- **`GET /projects/{id}/workflows/{wid}/runs`** — held back until a consumer exists, per the rule the
  milestone history of this section keeps restating: publish an operation because something reads it.

## Consequences

**Positive**

- GUI #101 gets a renderable model: drag cases from the tree = append step references; reorder = PUT
  the array; run = one POST. Every pattern is one the GUI already speaks (`/projects/{id}/…` CRUD +
  duplicate).
- Runs stay the only frozen state and workflows the only live plan — the two roles cannot blur,
  because the schema has nowhere to put a snapshot on a workflow.
- The surface reuses four settled precedents (milestone placement + identity, strict schemas,
  composition reach rules, #460's additive-field policy), so its review surface is small.

**Costs / risks**

- A dangling target only fails at materialisation — a plan can rot silently if a case is deleted
  (accepted: the same exposure exists for runs' `caseVersions` and milestones' references today).
- Duplicate-destination validation (§3) needs reach checks the duplicate route does not do for other
  resources — it is the one genuinely new guard, and the reason the build ticket belongs to the API
  before any canvas work starts.
- No revisions means a destroyed plan is gone; if that ever hurts, revision stamping is the
  `revise_case` pattern already in the codebase, not a redesign.

## References

- [`adr-storage-layout-v3.md`](adr-storage-layout-v3.md) — the placement grammar this ADR extends
- [`../contracts/api-compatibility.md`](../contracts/api-compatibility.md) — additive-change policy
  the `sourceWorkflowId` field follows
- [`../testing/seed-dataset-spec.md`](../testing/seed-dataset-spec.md) — single-home identifiers and
  the row the build ticket adds
- [#462](https://github.com/TucanoTechnology/TucanoTestAPI/issues/462) — the parent-scoped doors
  convention this surface continues
