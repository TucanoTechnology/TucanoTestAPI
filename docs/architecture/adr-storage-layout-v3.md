# ADR: Storage Layout v3 — Runs, Milestones and Configurations Inside Their Project

- **Status:** Accepted
- **Date:** 2026-09-15
- **Issue:** [#215](https://github.com/TucanoTechnology/TucanoTestAPI/issues/215)
- **Deciders:** repository owner (ECiurleo)
- **Supersedes:** the three flat root collections of the v2 layout recorded under *Hierarchy and
  Real-Home Storage Plan (Issue #65)* in
  [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md). Nothing else in that
  plan changes.

## Context

Issue [#215](https://github.com/TucanoTechnology/TucanoTestAPI/issues/215) is the repository's only
`priority:P0` ticket and its whole body is its title:

> `test_runs/<id>.json` / `milestones/<id>.json` / `configurations/<id>.json` — should be stored
> within a project

Three of the six resources are still stored in root-level collections. That is the shape Issue #65
deliberately left behind when it moved projects, suites and cases into the real-home tree: it
declared the earlier directories development artifacts, migrated nothing, and kept runs, milestones
and configurations flat. This ADR closes that gap. It is a decision document, not an implementation:
it fixes every choice the work needs so the execution tasks can be carried out without reopening
any of them.

### The rule the ticket is invoking

`AGENTS.md` states the invariant three times over, and `README.md` repeats it:

- "**Storage mirrors the conceptual organisation.** A project is a folder that contains its test
  suites and may also contain test cases directly."
- "**Every case and suite has one real home; a real parent is required at creation.** … Nothing is
  created in a standalone top-level pool, and the on-disk tree mirrors the homes."
- "**Placing an existing entity elsewhere is copy by default, move opt-in.**"
- "Reads stay global — listing and retrieval search the whole tree, so an entity is always findable
  regardless of home."

A run, a milestone and a configuration have no home today. They are the last three entities created
in a standalone top-level pool, which the philosophy forbids for everything else.

### What the implementation relies on today

Source-verified against `main` at the time of writing:

| Where | What it assumes |
| --- | --- |
| `src/storage/layout.rs` | `Resource::ROOT_DIRS = [Projects, Runs, Milestones, Configurations]`; `dir_name()` maps them to `projects`, `test_runs`, `milestones`, `configurations`; `marker_name()` is `None` for all three; `is_flat()` is true for all three; `document_path()` builds `<root_dir>/<id>.json`; `Parent` has exactly two variants, `Project` and `Suite` |
| `src/storage/mod.rs` | the `Repository` doc states that flat resources "pass `None` and rely on a globally unique identifier" |
| `src/storage/fs.rs` | `FileRepository::new` creates all four root directories; `list_flat()` reads one root directory; `document()` rejects a parent for the three (`reject_parent`); `locate()` and `list_children()` refuse them ("resource is not stored in the project tree"); `delete_at` uses `remove_file` for them |
| `src/domain/service.rs` | `create()` routes them through `create_at(resource, None, value)`; `assembled()` has a `_` arm reading with `None`; `owner_for_write()` returns `Ok(None)` for everything but suites and cases; `save()` always writes with `None`; `load()`, `first_document()`, `document()`, `milestone_progress()` and `summary_report()` all read them at the root |
| `src/api/access.rs` | ownership is **derived, never stored**: `projects_of()` reads a run's `projects` snapshot array, resolves a milestone's `testSuiteIds`/`testRunIds`, and returns `Ok(Vec::new())` for configurations; four sites special-case configurations as installation-wide (`guard_get`, `guard_update`, `guard_delete`, `filter_list`) |
| `src/api/crud.rs` | `crud_handlers!` hard-codes `Path(id): Path<String>` and never supplies a parent, so a project-scoped route needs `Path((project, id))` and its own handler, exactly as `src/api/suites.rs` already does |
| `src/api/mod.rs` | `ROUTES` holds the flat `/test_runs…`, `/milestones…`, `/configurations…` families; `UNDOCUMENTED_ROUTES = ["/api-docs/", "/test_suites", "/test_cases"]` |

The documents themselves are small and stable:

```rust
pub struct TestRun {           pub struct Milestone {          pub struct TestConfiguration {
    test_run_id: String,           milestone_id: String,           config_id: String,
    timestamp: String,             name: String,                   name: String,
    name: Option<String>,          description: Option<String>,    browser: Option<String>,
    projects: Option<Vec<Project>>,start_date: Option<String>,     os: Option<String>,
    test_suites: Option<…>,        target_date: Option<String>,    device: Option<String>,
    test_cases: Option<…>,         status: Option<String>,         resolution: Option<String>,
    results: Option<…>,            test_suite_ids: Option<…>,  }
    tags: Option<…>,               test_run_ids: Option<…>,    }
    configurations: Option<…>, }
    case_versions: Option<…>,  }
```

All three carry `deny_unknown_fields`, and the legacy Draft 2020-12 schemas set
`additionalProperties: false`, so **any new field is a breaking change**.

### The three semantics that make this more than a `mv`

1. **A run can span projects.** `TestRun.projects` is an array of *embedded project snapshots*.
   Authorisation requires the role in **every** project it names
   (`access::require_every`), a run that names none is readable and writable by any authenticated
   caller, and `filter_list` hides a project-less run from a restricted caller's listing while
   `guard_get` still serves it. Giving a run one home therefore has to say what happens to that
   array and to the rule built on it.
2. **A milestone's project is already derived, and required.** `access::milestone_projects()`
   resolves `testSuiteIds` and `testRunIds` to projects; `guard_create` refuses a milestone that
   reaches none with `400 invalid_request` ("A milestone must reference at least one project…") and
   `require_milestone` refuses to read one with `403`. `README.md` explains why: "a milestone is a
   project resource". The ticket agrees with the README's *intent* and contradicts its *encoding*.
3. **Configurations are documented as installation-wide.** `README.md`: "Configurations are
   installation-wide and readable and writable by any authenticated caller."
   `docs/wiki/tags-and-configurations.md` repeats it, `write_role()` gives them `Editor` that
   nothing ever consults, and the seed dataset creates them *before* any project exists
   (`docs/testing/seed-dataset-spec.md` §3 step 1). Project-scoping them is a semantic change, not a
   path change, and it has to be stated rather than implied.

### Why now, and why it is cheap

The crate is at `version = "0.1.0"`. There is no released on-disk format to protect, no captured
compatibility fixture (`docs/contracts/api-compatibility.md`: "Sanitised reference fixtures have
**not** yet been captured"), and an explicit precedent for treating an existing data directory as a
development artifact. The GUI is a sibling repository that consumes only the HTTP contract, so the
wire surface is the real cost — and Issue #66 already established the pattern for changing it.

## Options considered

| Property | What it means here |
| --- | --- |
| **One real home** | `AGENTS.md`: every entity has one physical home and a real parent at creation |
| **Document shape frozen** | no field added to `TestRun` / `Milestone` / `TestConfiguration`, so no legacy-schema break and no `formatVersion` bump |
| **Global reads** | `AGENTS.md`: listing and retrieval search the whole tree, so an entity is findable regardless of home |
| **No new disclosure** | authorisation never weaker than today's (`access.rs` resolves the projects behind every request) |
| **No silent wrong answer** | the failure mode this repository's contracts repeatedly refuse: a document that is present but invisible, or a report that quietly under-counts |
| **Trait shape stable** | `Repository`'s signatures unchanged, so [#182](https://github.com/TucanoTechnology/TucanoTestAPI/issues/182) and the accepted [object-storage ADR](./adr-object-storage.md) are not disturbed |

### Option A — Folder home only, project-scoped creation, global reads (chosen)

Store each of the three in a per-resource subfolder of its project folder, exactly as suites and
cases are stored. The folder is the home; nothing is added to any document. Creation becomes
parent-scoped; reads, updates, deletes and every run sub-route stay addressable by bare id and
resolve through `locate()`, answering `409 Conflict` when two projects hold the same id.

- **One real home:** satisfied, and by the same mechanism the tree already uses.
- **Document shape frozen:** satisfied — this is the option's main advantage. No schema break, no
  `formatVersion` bump, no legacy consumer refuses a document.
- **Global reads:** satisfied; identical to the suite/case rule a caller already meets.
- **No new disclosure:** satisfied by keeping the "every covered project" rule and adding the home
  to it (see *Decision 3*).
- **No silent wrong answer:** satisfied, provided the reports and the listing filter iterate
  per-project rather than over de-duplicated ids (see *Decision 6*).
- **Trait shape stable:** satisfied — `parent: Option<&Parent>` already exists on every method; only
  its meaning for three resources changes.
- **Cost:** six new route paths, three retired creation routes, a per-project id namespace, and the
  reports/filter rework.

### Option B — Store an owning-project field on the document

Add `projectId` (or `homeProjectId`) to the three models and keep the flat root collections.

- **One real home:** satisfied only in the abstract — the home becomes a *claim inside the document*
  that nothing on disk corroborates, and the tree still reads `test_runs/nightly.json` with no
  project in sight. That is precisely the "membership duplicated inside a document" the v2 layout
  removed for suites and cases ("Membership is never duplicated inside parent documents… stored
  parents can never become stale").
- **Document shape frozen:** **violated.** `additionalProperties: false` makes this a breaking
  schema change for all three documents, it needs a *Breaking change accounting* entry of its own,
  and a legacy consumer rejects every document the new build writes.
- **Global reads:** unchanged.
- **Cost:** lower code cost than A, but it buys the ticket's letter ("stored within a project")
  without its substance, and it makes the stored field and the physical tree two sources of truth
  that can disagree.

### Option C — Both: folder home and a stored field

- Every downside of B, plus a redundancy the repository has already decided against. The only
  argument for it is that a document carries its home when it is copied out of the tree — which is
  not a use case this API has, since nothing reads storage except the API. Rejected.

### Option D — Leave configurations installation-wide, scope only runs and milestones

Delivers two thirds of the ticket and keeps `README.md` true.

- **One real home:** violated for configurations, which would remain the only entity with no home.
- It is also the option that ages worst: the next ticket would be this one again, minus two lines.
- Rejected, but it is the reason *Decision 5* states the configuration semantics explicitly instead
  of leaving them to be inferred: the ticket asks for project-scoped configurations, so
  configurations become project-scoped, and the README sentence that says otherwise is replaced.

### Option E — Automatic startup migration of the flat documents

On boot, move each root document into a project folder: a run into the first project of its
`projects` array, a milestone into its first resolved reference, a configuration into… nothing,
because a configuration names no project.

- It cannot answer the configuration case at all, so it needs a synthetic "unassigned" project —
  which reintroduces the standalone pool the philosophy forbids, now with a fake project in it.
- It writes to the volume at startup on every replica, against a shared mount, which is exactly the
  multi-replica hazard the deployment guide's advisory-lock rule exists to prevent.
- It is irreversible on a volume whose operator did not take the pre-change snapshot the promotion
  runbook already requires.
- Rejected in favour of the #65 precedent plus a loud refusal (*Decision 8*).

## Decision

Layout **v3**. Runs, milestones and configurations are stored inside their project folder, in a
per-resource subfolder, and are created through a project-scoped route. Their identifiers become
unique per project. Their document shapes do not change at all.

```text
TUCANO_DATA_DIR/
├── .tucano.lock
├── auth/…                                 unchanged
└── projects/
    └── <project>/
        ├── project.json                   unchanged
        ├── test_runs/<id>.json            was <data>/test_runs/<id>.json
        ├── milestones/<id>.json           was <data>/milestones/<id>.json
        ├── configurations/<id>.json       was <data>/configurations/<id>.json
        ├── <test suite>/suite.json …      unchanged
        └── <test case>/test-case.json …   unchanged
```

The three subfolders hold single documents, not folders: a run stays "one flat `<id>.json` file, not
a folder", as `README.md` property 3 requires. Only the level they hang from changes.

### Decision 1 — the owning link is the folder, and nothing is stored

The home is the folder, full stop. **No field is added to `TestRun`, `Milestone` or
`TestConfiguration`.** Ownership continues to be derived (`src/api/access.rs`: "ownership is
DERIVED, never stored"), now by `locate()` instead of by reading a snapshot array.

Consequences that follow automatically and are required:

- No `additionalProperties: false` break, so **no `formatVersion` bump**. This is a *layout* change,
  not a *document format* change: no stored document's shape or meaning changes, so
  [`docs/contracts/file-format-versioning-plan.md`](../contracts/file-format-versioning-plan.md)
  keeps `SUPPORTED_FORMAT_VERSION = 1` and only its stored-path table changes.
- A body cannot set or change the home. `TestRun`, `Milestone` and `TestConfiguration` have no home
  field, so a body carrying `projectId` is already refused as an unknown field by
  `validation::validate_payload` (Issues #71 and #121). `PUT` therefore can never move a document;
  neither can a duplicate, which lands in the source's own home.
- The home is never taken from a run's `projects` array or a milestone's references. Those stay
  metadata (*Decisions 3 and 4*).

### Decision 2 — identifiers are unique per project

A run, milestone or configuration id is unique **within its project**, exactly as a suite id is
unique within its project and a case id within its parent. `nightly.json` may therefore exist under
two projects. Project ids stay globally unique.

Resolution rules, which reuse the machinery suites and cases already have:

| Situation | Answer |
| --- | --- |
| A bare id on a global document route, zero occurrences | `404 not_found`, message unchanged ("Resource not found" where that is today's text) |
| exactly one occurrence | operate on it |
| two or more | `409 conflict` from the existing `ambiguous()` helper, whose `_` arm gains a per-resource endpoint string: runs → `POST /projects/{id}/test_runs, or the matching /{run_id} delete`, milestones → `…/milestones…/{milestone_id}…`, configurations → `…/configurations…/{config_id}…` |
| A global list route | de-duplicated, sorted ids — unchanged behaviour, and the same information loss suites and cases already accept |
| A parent-scoped route | addresses the named project's occurrence directly; it never resolves globally, so it works even when the id is ambiguous elsewhere |

Because a global list de-duplicates, **the server must not compute anything from it**. `filter_list`
and both reports iterate projects and their `list_children` instead (*Decision 6*).

`test_runs`, `milestones` and `configurations` become **reserved child names inside a project
folder**: creating a suite or a case whose folder name equals one of them is refused with
`409 conflict` ("child name is already taken by another kind of resource", the message
`FileRepository::ensure_kind_available` already produces). Without this, a suite named
`test_runs.json` would be created *inside* the run collection and `list_children(project, Runs)`
would then report `suite.json` as a run id.

### Decision 3 — a run's home is its folder; `projects` stays as covered-project metadata

`TestRun.projects` keeps its exact shape and its meaning as *the projects this run covered at
execution time* — embedded snapshots, used by the reports and by history. It is **no longer the
ownership or authorisation source**, and it is **not** constrained to the home: a run stored in
project A may still cover A and B, because a cross-project nightly run is a real thing the reports
already serve.

Authorisation becomes the union, which is never weaker than today:

> Every operation on a run document requires the role in the run's **home project** and in **every
> project its `projects` array names** — `Viewer` for reads, `Editor` for writes.

- Today a run requires the role in every covered project; adding the home keeps that and anchors a
  run that covers none. The "a run naming no project is readable and writable by any authenticated
  caller" fallback is **withdrawn**: such a run is now governed by its home.
- The union is what prevents a new disclosure path. A run embeds whole `Project` snapshots, so
  scoping authorisation to the home alone would let a `Viewer` of A read B's structure through a run
  stored in A. Keeping the covered-project requirement preserves today's guarantee exactly.
- `require_run_source` is unchanged: including a suite or a case in a run still needs the role in
  the *source's* project, so a run in one project cannot pull content out of another.
- `filter_list` for runs: keep an id when its home resolves uniquely and is reachable **and** every
  covered project is reachable. The "must name at least one project" condition is dropped, because
  the home now anchors it — so a project-less run becomes visible in a reachable project's listing
  instead of hidden. An id two projects hold is dropped for a restricted caller, which is how an
  ambiguous suite or case id already behaves.

### Decision 4 — a milestone's home is its folder, and the "must reference a project" rule is withdrawn

A milestone is stored in its project and governed by it:

> Every operation on a milestone requires the role in the milestone's **home project** and in
> **every project its `testSuiteIds` / `testRunIds` references reach** — `Viewer` for reads,
> `Owner` for writes (unchanged from `write_role(Milestones)`).

Two rules exist only because a milestone had no home, and are **removed**:

- `guard_create`'s `400 invalid_request` "A milestone must reference at least one project: link it
  to a test suite or a test run" (`access::milestone_needs_project`).
- `require_milestone`'s `403 forbidden` "This milestone is not linked to any project".

A milestone with no references is now a legal planning milestone in a project; its progress reports
zeros, as `progress::compute` already does for an empty run set. This is a loosening of a validation
rule and is recorded as such in *Breaking change accounting*.

`milestone_progress` keeps its documented tolerance — a reference to a run that no longer exists is
skipped, because "milestone progress simply recomputes over the runs that still exist" (Issue #65).
It gains one refusal: a reference that resolves to **two or more** runs answers `409 conflict`
rather than picking one, because a silently arbitrary choice is a wrong progress number. The
separate, pre-existing hazard that an *undeserialisable* run is skipped
(`let Ok(run) = … else { continue }`) is **not** in scope here; it belongs to the format-version
reader check of [#98](https://github.com/TucanoTechnology/TucanoTestAPI/issues/98), and layout v3
does not make it worse — a run that moved is *absent*, not unreadable.

### Decision 5 — configurations become per-project resources

Configurations are stored in a project and governed by it. The installation-wide model is
withdrawn:

- `projects_of(Configurations)` returns the home instead of `Ok(Vec::new())`.
- The four installation-wide short-circuits in `src/api/access.rs` are removed (`guard_get`,
  `guard_update`, `guard_delete`, `filter_list`).
- Reads need `Viewer` in the home project; writes and creation need `Editor` there
  (`write_role(Configurations)` is already `Editor`; it simply starts being consulted).
- `GET /configurations` is filtered to the projects a restricted caller reaches, like every other
  listing. A system administrator and an auth-off deployment are unaffected (`scope` returns `None`).

A run may link a configuration from **any** project, resolved with the run's home preferred
(*Decision 7*); `POST /test_runs/{id}/configurations` requires `Editor` in the run's home, in every
project the run covers, and in the configuration's home project. This mirrors `require_run_source`
for suites and cases, so a run cannot be used to pull another project's content.

`?configuration=` on `GET /test_runs` and `?configuration=` on `GET /reports/summary` are unchanged
string comparisons against a run's embedded `configurations` snapshots. The summary report's
existence pre-check changes from `exists_at(Configurations, None, id)` to "at least one project
holds this id", still answering `404 not_found` when none does and **not** `409` when several do — a
filter value is not a dereference.

**The `README.md` sentence that must change** (Authentication section):

> `POST /milestones` and `PUT /milestones/{id}` must name a project-bearing reference, because a
> milestone is a project resource. Configurations are installation-wide and readable and writable by
> any authenticated caller.

becomes

> A milestone and a configuration are project resources like everything else: creating one needs
> `editor` in the project it is created in (`owner` for a milestone), reading one needs `viewer`
> there, and a run may reference a suite, case or configuration from any project the caller reaches.

`docs/wiki/tags-and-configurations.md` ("installations-wide", "any authenticated caller may read and
write them", "There is no ownership on a configuration") and `docs/wiki/milestones.md` ("A flat
document, `milestones/<milestoneId>.json`") change with it, as does
`docs/testing/seed-dataset-spec.md` §1 row 10 and §3 step 1, which today creates configurations
before any project exists.

### Decision 6 — the wire surface: project-scoped creation, global reads, retired flat creation

This follows the Issue #66 pattern for suites and cases exactly: creation is parent-scoped, reads
stay global, and a retired flat creation route stays registered to explain itself.

**New routes, all documented in `openapi.json`:**

| Route | Methods | Auth | Answers |
| --- | --- | --- | --- |
| `/projects/{id}/test_runs` | `GET`, `POST` | `Viewer` / see below | `200` bare sorted id array; `201 {"message","id"}` |
| `/projects/{id}/test_runs/{run_id}` | `DELETE` | `Editor` in `{id}` | `200 {"message":"Test run deleted"}` |
| `/projects/{id}/milestones` | `GET`, `POST` | `Viewer` / `Owner` | as above |
| `/projects/{id}/milestones/{milestone_id}` | `DELETE` | `Owner` in `{id}` | `200 {"message":"Milestone deleted"}` |
| `/projects/{id}/configurations` | `GET`, `POST` | `Viewer` / `Editor` | as above |
| `/projects/{id}/configurations/{config_id}` | `DELETE` | `Editor` in `{id}` | `200 {"message":"Test configuration deleted"}` |

- Creation additionally requires the role in every project the body names (a run's `projects`, a
  milestone's resolved references), preserving `guard_create`'s guarantee. One new helper carries
  this: `access::guard_project_create(state, principal, resource, project, body)`.
- Unknown project → `404 not_found` ("Project not found", via `require_parent`); duplicate id
  inside the project → `409 conflict`; missing required fields → `400 invalid_request` with the
  existing field matrix.
- Handler shape: `Path(id): Path<String>` for the collection routes and
  `Path((id, run_id)): Path<(String, String)>` for the delete routes, as `delete_project_suite`
  already does. `crud_handlers!` is **not** extended — it generates globally addressed handlers, and
  these six are hand-written in `src/api/{runs,milestones,configurations}.rs` beside the existing
  wrappers.
- The parent-scoped delete requires the role in the **named project only** and then deletes that
  occurrence (`delete_in`). It does not resolve globally, so it succeeds for an id two projects
  hold. (`delete_project_suite` used to resolve globally through `guard_delete`; it now authorises
  the project the route names, like `delete_project_case` and the run, milestone and configuration
  deletes, so the route its own conflict message recommends can actually delete one home at a time.)
- The parent-scoped lists publish **no query parameters** and answer a bare sorted id array, exactly
  like `GET /projects/{id}/test_suites`. `?filter=`, `?tags=` and `?configuration=` remain on the
  global scans only. *Amended by [Issue #293](https://github.com/TucanoTechnology/TucanoTestAPI/issues/293):*
  the project-scoped suite, case and run listings now serve the same `?filter=` and `?tags=` (and,
  for runs, `?configuration=`) the global scans do, so a listing is narrowed where it is read; the
  parent-scoped milestone and configuration listings and `GET /test_suites/{id}/test_cases` are
  unchanged.

**Kept routes, unchanged paths and shapes:** `GET /test_runs/{id}`, `PUT`, `DELETE`,
`POST /test_runs/{id}/duplicate`, every run sub-route (`/test_suites`, `/test_cases`, `/results`,
`/results/{case_id}/defects[…]`, `/import/junit`, `/import/json`, `/configurations[…]`),
`GET/PUT/DELETE /milestones/{id}`, `/duplicate`, `/progress`, `GET/PUT/DELETE /configurations/{id}`.
Each now resolves its id through `locate()`, so each can answer `409 conflict` when the id is
ambiguous — a new answer for these routes, and the same one `/test_cases/{id}` already gives.

**Retired routes:** `POST /test_runs`, `POST /milestones`, `POST /configurations`. Following #66,
each path stays registered and answers `400 invalid_request` naming its replacement, produced by the
existing retirement mechanism in `TestService::create()` — which gains three arms beside the suite
and case ones:

- `"Test runs are created inside a project: POST /projects/{id}/test_runs"`
- `"Milestones are created inside a project: POST /projects/{id}/milestones"`
- `"Configurations are created inside a project: POST /projects/{id}/configurations"`

`access::guard_create` loses its `Runs` and `Milestones` arms so the explanation is answered
deterministically rather than preceded by a `403` for a restricted caller; the project-scoped
handlers do that authorisation instead.

**Documentation granularity.** The route-parity test
(`tests/service.rs::openapi_document_matches_the_registered_routes`) compares **path keys**, so the
retirement follows #66: `/test_runs`, `/milestones` and `/configurations` join
`api::UNDOCUMENTED_ROUTES` as whole path keys. The global scans `GET /test_runs`, `GET /milestones`
and `GET /configurations` stay **served** (the philosophy requires global reads) and become
**undocumented**, exactly as `GET /test_suites` and `GET /test_cases` did. The published contract
advertises the parent-scoped collections instead.

**Operation count.** `openapi.json` holds 68 operations today (the pin at
`tests/service.rs:653`; the "64" in the Issue #145 accounting entry predates the defect-link,
import, history and report operations). This change removes 6 (`get`/`post` on each of the three
bare collection paths) and adds 9 (`get`+`post` on each new collection path, `delete` on each new
item path): **71 operations**, which is the new pin.

New `operationId`s follow the existing project-scoped naming — `list`/`add`/`remove` +
`Project` + resource: `listProjectTestRuns`, `addProjectTestRun`, `removeProjectTestRun`,
`listProjectMilestones`, `addProjectMilestone`, `removeProjectMilestone`,
`listProjectConfigurations`, `addProjectConfiguration`, `removeProjectConfiguration`. Each carries
the `Projects` tag, as the suite and case equivalents do. Response sets mirror the templates:
the `POST`s mirror `POST /projects/{id}/test_suites` (`201 CreateResponse`, `400
InvalidIdOrRequest`, `401`, `403`, `404`, `409`, `413 BodyTooLarge`) with the resource's existing
`*CreateRequest` body schema; the `GET`s mirror `GET /projects/{id}/test_suites` (`200` bare array,
`400 InvalidId`, `401`, `403`, `404`); the `DELETE`s mirror
`DELETE /projects/{id}/test_suites/{suite_id}` (`200 MessageResponse`, `400 InvalidId`, `401`,
`403`, `404`).

### Decision 7 — one reference-resolution rule

References stored in a document and ids named by a request now resolve by the same algorithm, which
is the only place home-preference exists:

```text
resolve(resource, id, home) ->
  1. home is Some(p) and exists_at(resource, Parent::Project(p), id)  ->  that occurrence
  2. homes = locate(resource, id)
  3. homes.len() == 0  ->  404 not_found
     homes.len() == 1  ->  that occurrence
     otherwise         ->  409 conflict (ambiguous)
```

`home` is the acting document's project when it has one, and `None` for a bare id in a request:

| Caller | `home` |
| --- | --- |
| `milestone_progress` resolving `testRunIds` | the milestone's home |
| `access::milestone_projects` resolving a milestone's references | the milestone's home |
| `link_configuration_to_run` / `unlink_configuration_from_run` resolving `configId` | the run's home |
| `summary_report` resolving a `milestoneId` filter | `None` |
| every global document route, `duplicate`, attachments | `None` |
| `first_document` for `?filter=` / `?tags=` / `?configuration=` | first occurrence, as today |

Home-preference is what keeps a milestone in project A referencing `nightly.json` meaning A's run
instead of failing on an id that another project also happens to use. It never *hides* an ambiguity
that matters: when the home does not hold the id, the global rule applies and two occurrences
answer `409`.

### Decision 8 — no migration; a legacy layout is refused at startup, loudly

Following the Issue #65 precedent ("Existing data directories created by earlier builds are
development artifacts and are not migrated"), and because the crate is pre-1.0 with no captured
fixtures:

- **Nothing is migrated, and no migration is scripted.** No startup rewrite, no sidecar, no
  `scripts/migrate-*`.
- `FileRepository::new` creates only `projects/`. `Resource::ROOT_DIRS` becomes `[Projects]`.
- The three root collections are never read, written, listed or deleted. **They are never removed
  either** — an operator's data is not this service's to destroy.
- `FileRepository::new` **fails** when a legacy root collection still holds documents. The check is
  read-only, runs once, and counts `*.json` entries in `<data>/test_runs`, `<data>/milestones` and
  `<data>/configurations`, ignoring the atomic-write temporary pattern `.tucano-*.tmp`. The error is
  an `io::Error` that `src/main.rs` already propagates (`FileRepository::new(data_dir)?`), so the
  process aborts before it binds a port. Message, naming relative directories only and no absolute
  path:

  > `legacy flat storage layout detected: test_runs/ holds 3 document(s), milestones/ holds 1;
  > layout v3 stores runs, milestones and configurations inside their project folder — move each
  > document into projects/<project>/<collection>/ and restart
  > (docs/deployment/deployment-guide.md)`

  An empty legacy directory — which every existing volume has, because today's build creates all
  three on startup — is **not** an error.
- A refusal rather than silence is the point. Ignoring 500 run documents and answering `[]` from
  `GET /test_runs` is the silent wrong answer this repository's contracts refuse everywhere else; a
  container that will not start is an operator-visible, actionable, reversible condition.
- **Operator recipe**, documented in `docs/deployment/deployment-guide.md`: stop the service, take a
  copy of the volume, create `projects/<project>/{test_runs,milestones,configurations}/`, move each
  document into the project that owns it, restart. A run whose `projects` array is empty and a
  configuration — which names no project at all — need a human decision about which project they
  belong to; that decision is the reason no script is provided.
- **Rollback.** Rolling the image back to a v2 build against a v3 volume leaves that build with no
  root collections to read: it answers `404` for runs, milestones and configurations, and
  `GET /milestones/{id}/progress` reports zeros because it skips the runs it cannot find. Full
  rollback therefore means restoring the pre-change snapshot the promotion runbook
  ([`docs/deployment/canary-validation-and-rollback.md`](../deployment/canary-validation-and-rollback.md))
  already takes. **No `formatVersion` bump is involved**, because no document's shape changed — the
  marker answers "is this document from the future", not "is this document where I expect it".

### Decision 9 — what deliberately does not change

- **The `Repository` trait's signatures.** `parent: Option<&Parent>` already exists on `exists_at`,
  `read_at`, `write_at` and `delete_at`; `Parent::Project` is already a variant. What changes is
  their meaning for three resources: `Some(Parent::Project(_))` becomes required (`None` and
  `Parent::Suite` answer `io::ErrorKind::InvalidInput`, "resource requires a project parent", the
  message `project_parent()` already produces). `locate()` and `list_children()` gain arms instead
  of refusing. Its doc comment in `src/storage/mod.rs` — "flat resources pass `None` and rely on a
  globally unique identifier" — is rewritten.
- **`place()`** stays suites-and-cases only. The three resources are **not** placeable: a
  parent-scoped `POST` is create-only, accepts no `mode`, and a body carrying one is refused as an
  unknown field by the existing payload validation. Moving a run between projects is not a
  requirement, and copying one is what `POST /test_runs/{id}/duplicate` is for.
- **`duplicate()`** keeps its derived-id rule (`{base}-copy-{nanos}.json`) and now writes the copy
  into the source's resolved home.
- **Attachments, steps, revisions, case history, imports, defect links, coverage report** — all
  untouched. `coverage_report` walks projects, suites and cases only.
- **`summary_report`** keeps its filters and its skip-on-unreadable tolerance, and changes only in
  how it enumerates: projects × `list_children(project, Runs)` instead of the de-duplicated global
  `list(Runs)`, so two runs named `nightly.json` in different projects are both counted.
  `reports::run_is_in_scope` gains the home as an additional match for `?projectId=` (a run whose
  `projects` array omits its own home now matches it — a widening), and `reports::run_reachable`
  gains the home as a requirement, matching `filter_list`.
- **No 404 or 409 message text changes.** Where a helper needs a missing-document message for one of
  the three resources it uses the string `access::missing()` already returns for it.

## Rationale

- **The folder is the only home that cannot lie.** A stored `projectId` is a claim; a folder is a
  fact the listing, the authorisation and the delete cascade all read from. The v2 layout already
  proved the model for suites and cases, and this decision extends it rather than inventing a second
  mechanism for three resources.
- **Freezing the document shape keeps the change off the compatibility cliff.** With
  `additionalProperties: false` in the legacy schemas, Option B would break every legacy consumer
  for a field that duplicates what the path already says. Option A breaks paths and routes — which
  is real, but is what #66 already did, is what the parity tests already model, and needs no
  `formatVersion` bump.
- **The #66 precedent is the cheapest correct wire surface.** Parent-scoped creation, global reads,
  `409` on ambiguity, retired flat creation answering `400` with the replacement's name: a caller
  that learned it for suites and cases needs to learn nothing new here.
- **Union authorisation buys the ticket without a disclosure regression.** Scoping a run to its home
  alone would expose embedded snapshots of projects the caller cannot reach. Requiring home *and*
  covered projects is strictly at least today's guarantee.
- **A loud startup refusal beats an empty listing.** Silent invisibility of existing data is the one
  failure mode this repository's contracts name repeatedly; a crashloop with a message that says
  what to move and where is recoverable in one step.
- **Per-project uniqueness is the honest reading of "stored within a project".** Global uniqueness
  enforced across projects would keep lists exact, but it would answer `409` on a name that is free
  in the caller's own project and would leak the existence of another project's identifier to a
  caller who cannot reach it. The reports and the listing filter are reworked instead, which is
  where the exactness was actually needed.

## Consequences

### Positive

- The on-disk tree reads like the domain for all six resources; `TUCANO_DATA_DIR` has one root
  collection instead of four.
- Deleting a project removes its runs, milestones and configurations with it, through the
  `remove_dir_all` cascade that already exists — today they survive their project.
- Authorisation stops being derived from document contents for these three resources and starts
  being derived from where they live, which is the same rule the rest of the tree uses.
- The configuration model gains ownership, so a project's environments are visible and writable
  only to that project's members.
- No document shape changes, so no legacy-schema break and no format-version bump.

### Negative / accepted risks

- **The wire contract breaks for creation.** `POST /test_runs`, `POST /milestones` and
  `POST /configurations` stop creating. The GUI must move to the parent-scoped routes; it is an
  equal citizen, so this is a coordinated change with
  [TucanoTestGUI](https://github.com/TucanoTechnology/TucanoTestGUI), not a server-only one.
- **Three published global scans leave `openapi.json`** while remaining served, exactly as
  `GET /test_suites` did. A generated client loses them and must enumerate projects instead.
- **`409` becomes possible on run, milestone and configuration document routes** that could never
  be ambiguous before.
- **Existing volumes stop booting** until an operator moves the documents. Accepted: the alternative
  is serving an empty view of a full volume.
- **A global listing can no longer distinguish two same-named runs.** Accepted for the compatibility
  surface (it is what suites and cases already do); the parent-scoped list, the reports and the
  listing filter all iterate per project and are exact.
- **Milestone authorisation tightens and its validation loosens** at the same time: a reference-less
  milestone becomes legal, and any milestone now needs `Owner` in its home. Both are recorded.

### Not done here

- The `milestone_progress` skip-on-undeserialisable-run hazard — tracked by
  [#98](https://github.com/TucanoTechnology/TucanoTestAPI/issues/98).
- Placement (`mode: copy|move`) for runs, milestones or configurations.
- Environment-matrix semantics for configurations — still
  [#34](https://github.com/TucanoTechnology/TucanoTestAPI/issues/34) and
  [#51](https://github.com/TucanoTechnology/TucanoTestAPI/issues/51).

## Sequencing against other tickets

| Ticket | Relationship |
| --- | --- |
| [#182](https://github.com/TucanoTechnology/TucanoTestAPI/issues/182) — extract a backend-agnostic `Repository` boundary | **Not revived and not blocked.** This ADR changes the *meaning* of the existing `parent` argument, not the trait's shape, and the [object-storage ADR](./adr-object-storage.md) already cancelled #182 on the grounds that `Repository` stays an internal seam. Note that #182–#185 are still **open** although that ADR cancelled them; closing them is housekeeping for that ADR, out of scope here. |
| [#183](https://github.com/TucanoTechnology/TucanoTestAPI/issues/183)–[#185](https://github.com/TucanoTechnology/TucanoTestAPI/issues/185) — S3 backend, configuration, conformance tests | Cancelled by the object-storage ADR; layout v3 does not touch that decision. Were an object-store backend ever proposed, one more project-prefixed key segment is no additional obstacle. |
| [#186](https://github.com/TucanoTechnology/TucanoTestAPI/issues/186) — document the storage backends | Re-scoped to the mirror option by the object-storage ADR. If pursued, it must describe layout v3, not v2. |
| [#98](https://github.com/TucanoTechnology/TucanoTestAPI/issues/98) / [#101](https://github.com/TucanoTechnology/TucanoTestAPI/issues/101) — format versioning and rollback drills | Unaffected in substance: `SUPPORTED_FORMAT_VERSION` stays `1`. #98's stored-path table changes (this ADR's second deliverable), and #101's drills must use v3 paths. |
| [#169](https://github.com/TucanoTechnology/TucanoTestAPI/issues/169) / [#195](https://github.com/TucanoTechnology/TucanoTestAPI/issues/195) — seed dataset and its Compose wiring | **Directly dependent.** `scripts/seed.mjs`, `teardown.mjs`, `clear-data.mjs`, `smoke.sh` and `docs/testing/seed-dataset-spec.md` all create these three resources without a project today and must follow the five-edit order `README.md` prescribes. |
| [#176](https://github.com/TucanoTechnology/TucanoTestAPI/issues/176)–[#178](https://github.com/TucanoTechnology/TucanoTestAPI/issues/178) — security audits | Should be run against the post-v3 surface; the authorisation rules this ADR fixes are input to the IDOR review. |
| [#165](https://github.com/TucanoTechnology/TucanoTestAPI/issues/165) — project wiki | The three wiki pages named in *Decision 5* change; a wiki task in flight must rebase on them. |

**Order:** this ADR (S1) → storage layout (S2) → domain (S3) → HTTP and authorisation (S4) →
legacy-layout refusal (S8) → contract (S6) and tests (S7) → prose, wiki and dataset (S5, S9). S2–S9
are separate tickets; none of them may reopen a decision recorded here.

## Sub-task decomposition

Every row is a GitHub sub-issue of [#215](https://github.com/TucanoTechnology/TucanoTestAPI/issues/215).
"Depends on" is a merge order, not a suggestion: a row whose dependency has not landed cannot satisfy
its Definition of Done.

| # | Area | Scope | Model | Complexity | Depends on |
| --- | --- | --- | --- | --- | --- |
| S1 | Architecture | **This ADR** plus the *Breaking change accounting* entry and the versioning-plan path table | `model:high` | M | — |
| S2 | Storage | `src/storage/{layout,fs,mod}.rs`: `ROOT_DIRS = [Projects]`, `project_dir_name()`, `project_collection_dir()`, `project_document_path()`, reserved project child names, `document()`/`folder()` arms, `list()`, `locate()`, `list_children()`, `delete_at`, and their unit tests | `model:high` | L | S1 |
| S3 | Domain | `src/domain/{service,resources,reports}.rs`: retired `create()` arms, home-resolving `save`/`load`/`assembled`/`owner_for_write`/`first_document`/`document`, `resolve(resource, id, home)`, `ambiguous()` messages, `milestone_progress`, `duplicate` home, per-project report enumeration, `run_is_in_scope`/`run_reachable` home | `model:high` | L | S2 |
| S4 | API | `src/api/{mod,crud,runs,milestones,configurations,access}.rs`: six project-scoped routes and handlers, `ROUTES`, `UNDOCUMENTED_ROUTES`, `projects_of`, `guard_project_create`, removal of the configuration short-circuits, `require_run`, `filter_list` | `model:high` | L | S1, S3 |
| S5 | Contracts and prose | `README.md` (storage tree, HTTP API table, the Authentication sentences quoted in *Decision 5*, property 3), `docs/contracts/api-compatibility.md` v2-layout section, `docs/deployment/deployment-guide.md` legacy-layout recipe, `docs/wiki/{test-runs-and-results,milestones,tags-and-configurations,projects-suites-and-cases}.md` | `model:docs` | M | S4 |
| S6 | OpenAPI | `openapi.json` and `swagger.html`: three path keys removed, six added, 71 operations, the `operationId`s, tags and response sets of *Decision 6* | `model:mid` | M | S4 |
| S7 | Tests | `tests/{service,runs,milestones,configurations,reports,auth,common/mod}.rs` and the layout/fs unit tests: project-scoped creation, per-project uniqueness and `409`, retired-route `400`s, authorisation union, report exactness, the new `71` pin | `model:mid` | L | S4, S6 |
| S8 | Storage startup | `FileRepository::new`: create only `projects/`, detect a legacy flat layout, refuse with the exact message of *Decision 8*, never delete anything; tests for empty legacy dirs, populated ones, and `.tucano-*.tmp` files | `model:mid` | S | S2 |
| S9 | Seed dataset | `docs/testing/seed-dataset-spec.md` (§1 rows 10–13, 18–20; §2 target tree; §3 steps 1, 6, 9; §3 step 12 assertions) then `scripts/{seed,teardown,clear-data}.mjs` and `scripts/smoke.sh`, in the five-edit order `README.md` prescribes | `model:mid` | M | S4 |

## References

- [Issue #215 — this decision](https://github.com/TucanoTechnology/TucanoTestAPI/issues/215)
- [Issue #65 — Hierarchy and real-home storage (layout v2)](https://github.com/TucanoTechnology/TucanoTestAPI/issues/65),
  [Issue #66 — real-parent creation](https://github.com/TucanoTechnology/TucanoTestAPI/issues/66),
  [Issue #67 — copy/move inclusion](https://github.com/TucanoTechnology/TucanoTestAPI/issues/67),
  [Issue #69 — configurations activation](https://github.com/TucanoTechnology/TucanoTestAPI/issues/69)
- [`AGENTS.md`](../../AGENTS.md) — Core Project Philosophy (storage mirrors the organisation, one real
  home, global reads)
- [`README.md`](../../README.md) — Storage concept, HTTP API, Authentication
- [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md) — the compatibility
  authority and *Breaking change accounting*
- [`docs/contracts/file-format-versioning-plan.md`](../contracts/file-format-versioning-plan.md) —
  why no `formatVersion` bump is involved, and the stored-path table this ADR updates
- [`docs/architecture/adr-object-storage.md`](./adr-object-storage.md) — the `Repository` seam and
  the file-based invariant this layout stays inside
- [`docs/architecture/rust-service-core.md`](./rust-service-core.md) — the layering S2–S4 follow
- [`docs/deployment/canary-validation-and-rollback.md`](../deployment/canary-validation-and-rollback.md)
  — the pre-change snapshot the rollback story depends on
- `src/storage/layout.rs`, `src/storage/fs.rs`, `src/domain/service.rs`, `src/domain/reports.rs`,
  `src/api/access.rs`, `src/api/crud.rs`, `src/api/suites.rs` — the code this decision constrains
