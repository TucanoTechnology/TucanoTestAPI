# Wiki Structure, Source of Truth, and Publication (Issue #170)

Issue: [#170](https://github.com/TucanoTechnology/TucanoTestAPI/issues/170) — decide how the project
wiki relates to the in-repo [`docs/`](../) tree and how it is published. Parent epic:
[#165](https://github.com/TucanoTechnology/TucanoTestAPI/issues/165) — *Create project wiki*. This is
the blocking decision for the epic: it settles the structure, the source of truth and the publication
mechanism before any wiki page is authored.

It is a decision record. It adds no route, no field and no stored-document change, and it ships no
application code.

## Problem

The epic asks for a user-focused wiki — practical guides for new users, existing users, API users and
sysadmins — that stays consistent with [`openapi.json`](../../openapi.json) and the existing `docs/`
tree. Two constraints already bind the answer:

1. **The repository-root rule in [`AGENTS.md`](../../AGENTS.md).** Only `README.md` and `AGENTS.md`
   belong at the repository root. Every other document lives under `docs/<category>/` and is linked
   from the README documentation table. A wiki that introduced a parallel top-level content tree, or a
   second copy of the same facts, would violate that rule.
2. **`openapi.json` is normative for the HTTP contract and is already checked against the router.**
   [`docs/architecture/gui-client-boundary.md`](gui-client-boundary.md) fixes the single-source-of-truth
   rule for clients, and `tests/service.rs` asserts the document matches the registered routes. A wiki
   page that restated the contract by hand would be a third copy that drifts.

So the question is not *whether* to document, but *where the prose lives* and *how a published surface
is produced from it without creating a second source of truth*.

## Decision

| Aspect | Decision |
| --- | --- |
| Canonical prose source | The in-repo `docs/` tree. Every wiki page is a file under `docs/<category>/`, reviewed in a normal pull request and versioned with the code. |
| Published surface | A **generated site built from `docs/` with [mdBook](https://rust-lang.github.io/mdBook/)**. The wiki is a build artifact, never an editor. |
| Relationship to GitHub Wiki | A **read-only mirror**, never an editor. `docs/wiki/` stays canonical; on a merge to `main` a publish job copies the pages into the wiki repository. Editing a wiki page directly is a mistake that the next publish overwrites. |
| README link | The README documentation table links the decision document and the generated wiki's entry point; the README remains the orientation page, not the wiki. |
| HTTP contract | `openapi.json` stays normative. Wiki API pages link the served contract and the Swagger UI at `/api-docs`; they never re-type routes, parameters or schemas. |
| Drift prevention | Hand-written prose lives in `docs/` and is reviewed like code; generated reference (the route/operation index) is produced from `openapi.json` at build time; a CI job regenerates and fails on any diff. Rules stated in *Drift prevention* below. |

### Why a generated mdBook site over a GitHub Wiki

| Criterion | mdBook site from `docs/` | GitHub Wiki | Wiki that mirrors `docs/` |
| --- | --- | --- | --- |
| Single source of truth | The `docs/` tree — one copy, already the repository's documentation home | A second repository; a page and its `docs/` twin diverge silently | Two copies kept in sync by hand or a script; the sync itself becomes a maintenance surface |
| Reviewed with the code | Yes — every page is a normal pull request in this repository | No — separate repository, separate history, no PR into this repo's checks | Pages mirror `docs/`, but the mirror step is unreviewed |
| Checked by this repository's CI | Yes — the build and the drift check run in `.github/workflows/` | No — CI here cannot see the wiki repository | Partially — only the `docs/` half is visible to CI |
| Offline / exportable | Yes — `mdbook build` produces a static site usable offline and from a volume | No — hosted-only | Only the `docs/` half |
| Consistency with the README table | Native — both read the same `docs/<category>/` paths | Requires duplicating every link target | Requires duplicating every link target |
| Cost to start | One `book.toml` plus a CI job; pages are the `docs/` files that already exist | Zero tooling, but the divergence cost is paid continuously | Highest — a bespoke sync tool to own |

The deciding trade-off: a GitHub Wiki is the cheapest surface to *create* and the most expensive to
*keep true*, because it is the only option that puts the prose outside the repository whose CI and
review process guard it. The epic's own Definition of Done requires that *"no page contradicts
`openapi.json` or `docs/`"* and that *"a stated update rule keeps the wiki current"* — both are
enforceable only when the source is in this repository. mdBook satisfies that; the other two do not
without a synchronisation mechanism that would itself need maintaining.

The mirror was subsequently adopted as a **one-way publication step**, which resolves that objection
rather than ignoring it: the wiki is a *destination* the build writes to, exactly like the mdBook site,
so the one-write-path rule still holds and the sync is a script
([`scripts/sync-github-wiki.mjs`](../../scripts/sync-github-wiki.mjs)) rather than a second editing
surface. Two properties make it safe where a hand-maintained mirror would not be:

- **Nothing is authored in the wiki.** The publish job flattens `docs/wiki/*.md` into the wiki's flat
  page namespace and overwrites whatever is there, so a page edited in the browser is transient by
  construction. The only durable edit is a pull request against `docs/`.
- **The mapping is checked in CI.** `node scripts/sync-github-wiki.mjs --check` runs in the same `docs`
  job as the other drift checks, so a new page whose name would collide in the flat namespace, or a
  cross-page link without a wiki target, fails on the pull request that introduces it.

The residual cost is the one the table names: only the `docs/` half is visible to CI here — the CI job
cannot verify what the wiki repository actually contains after a push. That is accepted because the
wiki holds no fact the `docs/` tree does not, and the publish job runs from the merged revision, so a
divergence can only ever be a stale copy awaiting the next push, never an authoritative one.

## Information architecture

Six sections, ordered by audience, matching the epic's scope. The heading levels map to mdBook
`SUMMARY.md` entries; each page lists its source of truth — what is authoritative for its content and
what it must not restate.

| Section | Page | Source of truth |
| --- | --- | --- |
| **1. Overview** | `README.md` (existing) | `README.md`; the storage concept and the HTTP surface table are already here |
| | Wiki index / landing page | Links only — every entry points at a `docs/` page; the index itself is generated from `SUMMARY.md` |
| **2. Install** | Installation and getting started | `docs/deployment/deployment-guide.md` (Compose configuration, the JSON volume mount, container hardening) plus the *Prerequisites* and *Application container* sections of `README.md` |
| | First project (create a project, suite and case) | `openapi.json` (the create routes) — the page links them and never re-types the schema |
| **3. Feature how-to** | Feature guides for the core workflow | `docs/architecture/rust-service-core.md` (layers and delivery status) and `README.md` (the storage concept: projects, suites, cases, runs, milestones, configurations) |
| | Test-case versioning and revision history | `docs/contracts/test-case-versioning-plan.md` |
| | Storage format and compatibility | `docs/contracts/api-compatibility.md`, `docs/contracts/file-format-versioning-plan.md` |
| **4. API and auth** | API and authentication quickstart | `openapi.json` (normative), the Swagger UI at `/api-docs`, and the *Authentication* section of `README.md`; the role matrix is enforced by `tests/auth.rs` |
| | Route and operation reference | Generated from `openapi.json` at build time — see *Drift prevention* |
| **5. Sysadmin / operations** | Deployment and operations guide | `docs/deployment/deployment-guide.md` |
| | Scaling and the shared volume | `README.md` (statelessness and shared-storage rule) and `docs/deployment/deployment-guide.md` |
| | Canary validation and rollback | `docs/deployment/canary-validation-and-rollback.md` |
| | Backup, restore and data inspection | `docs/deployment/deployment-guide.md` (the volume is the only state) and the storage concept in `README.md` |
| | Security model and scanning | `docs/security/threat-model.md`, `docs/security/authentication-decision.md`, `docs/security/scanning-policy.md` |
| **6. Troubleshooting** | Troubleshooting FAQ | The error envelope and request limits in `README.md`; `docs/contracts/api-compatibility.md`; the deployment guide's diagnosis sections. Answers must be reducible to a documented code or command — an FAQ entry that invents behaviour is not admissible |

Every planned page is a `docs/<category>/` file. The `docs/` categories that already exist —
`architecture/`, `contracts/`, `deployment/`, `security/` — keep their current contents; the wiki adds
the user-facing guides (`docs/guides/` for the how-to pages) and links the existing decision documents
rather than duplicating them. Child tasks [#171](https://github.com/TucanoTechnology/TucanoTestAPI/issues/171)
through [#174](https://github.com/TucanoTechnology/TucanoTestAPI/issues/174) each author one section and
add their pages to `SUMMARY.md` and the README table.

## Publication mechanism

One command builds the site; CI builds it and publishes it. The site is a **static artifact** derived
from `docs/`; nothing is authored in the published surface.

- **Local build (the one command):**

  ```sh
  mdbook build docs
  ```

  `docs/book.toml` configures the book (`src = "."`, so the existing `docs/` tree *is* the source), and
  `docs/SUMMARY.md` is the table of contents. The output lands in `docs/book/`, which the task that
  adds `book.toml` must exclude in `.gitignore`; the generated site is never committed, so a stale
  build is never reviewable as source.

- **CI path:** a job in `.github/workflows/` runs `mdbook build docs` together with the drift check
  below. On `main` and on tags, the built site is published as a static host artifact (a GitHub Actions
  Pages deployment or the release workflow's static asset), so the published surface is always produced
  from the merged revision. Pull requests run the same build and drift check but publish nothing.

- **No database, no service:** consistent with the project philosophy in [`AGENTS.md`](../../AGENTS.md),
  publishing is a build step over files, not a running system. A reader can build the same site locally
  from a checkout, offline.

- **GitHub Wiki mirror (the second destination):** a `wiki` job in
  [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) runs on a push to `main` only, stages the
  pages with `node scripts/sync-github-wiki.mjs --out <dir>`, and pushes them to the repository's wiki
  repository. Two details follow from how GitHub models a wiki:

  - A wiki is a **flat namespace** — `Home.md`, `Getting-started.md` — with no subdirectories, and a
    link between pages is a bare page name (`[[Getting started]]`), not a relative path. The script
    flattens `docs/wiki/` accordingly and rewrites each cross-page link; a link to a page that is *not*
    mirrored (a decision record, `AGENTS.md`, the README) is rewritten to an absolute URL in this
    repository, so it still reaches the real file.
  - GitHub creates the wiki repository when its **first page is saved**, so it may not exist yet. A
    shallow clone handles both states — a repository with no commits clones successfully with an unborn
    `HEAD`, and the publish commit seeds it — while a clone that genuinely fails, such as a missing
    repository or a token without access, fails the job under `set -e` instead of being mistaken for an
    empty wiki.

  The mirror is one-way by construction: the job deletes the staged pages' predecessors and copies
  afresh, so nothing written in the browser survives a publish. Publishing requires a `WIKI_TOKEN`
  secret with write access to the wiki repository — the built-in `GITHUB_TOKEN` cannot write to another
  repository. When the secret is absent the job logs that publishing was skipped rather than failing, so
  a merge is never blocked by an unconfigured mirror.

## Drift prevention

The rule that keeps the wiki true has two halves — what is hand-written and what is generated — plus the
CI check that enforces the boundary.

**Hand-written (reviewed as code):** every prose page under `docs/`. It is edited in a pull request,
reviewed, and merged like application code. A prose change that contradicts `openapi.json` is a review
rejection, not a runtime surprise.

**Generated (never edited by hand):**

- The **route and operation reference** — `docs/generated/operations-reference.md`, rendered from
  `openapi.json` by [`scripts/generate-operations-reference.mjs`](../../scripts/generate-operations-reference.mjs).
  It is a *view* of the contract, not a copy: the page is committed so it is reviewable in the diff, and
  CI re-renders it and fails on any difference.
- The **wiki index** is hand-written prose that links only pages that exist; CI keeps it exhaustive (see
  check 3).

**What CI checks** (the `docs` job in [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml)):

1. **Build must succeed** — `mdbook build docs` runs on every push and pull request; a broken link or a
   page missing from `SUMMARY.md` fails the job.
2. **Generated reference must be current** — `node scripts/generate-operations-reference.mjs --check`
   re-renders the operation reference from `openapi.json` and **fails if it differs** from the committed
   page. This is the same "generate, then fail on drift" pattern the generated-client strategy already
   uses in [`docs/architecture/gui-client-boundary.md`](gui-client-boundary.md): a contract change that
   is not reflected in the published reference is a red build, not a stale page. It runs in a plain
   container without rebuilding the crate, so it is cheap on every pull request.
3. **README linkage and links must stay complete** — `node scripts/check-docs-links.mjs` asserts that
   every relative link in every tracked Markdown document resolves, that every `docs/<category>/*.md`
   page appears in both `SUMMARY.md` and the README documentation table, and that the wiki index links
   every wiki page and no page that does not exist. A new page cannot be added without being
   discoverable, and the index cannot link a page that is missing — the defect issue
   [#236](https://github.com/TucanoTechnology/TucanoTestAPI/issues/236) reported. This mirrors the
   linkage requirement in [`AGENTS.md`](../../AGENTS.md).
4. **The GitHub Wiki mapping must stay valid** — `node scripts/sync-github-wiki.mjs --check` asserts that
   every mirrored page still maps onto the flat wiki namespace without a name collision. It runs in the
   `docs` job so the failure lands on the pull request that introduces it, rather than on the publish
   job after it has merged.

**The update rule, stated once:** *when a feature, route or deployment step changes, the same pull
request updates the `docs/` page that documents it and regenerates the reference; CI refuses the merge
if the regenerated reference or the README linkage is out of date.* Documentation and code change
together, in one reviewed revision.

## Consequences

- The wiki is a second *view* of `docs/`, never a second *copy*. There is exactly one place to edit a
  fact, and it is versioned with the code that implements it.
- Publication is a build step, so it adds no service, no database and no deployment concern.
- The tooling this record called for is in place: `docs/book.toml`, `docs/SUMMARY.md`,
  `scripts/generate-operations-reference.mjs`, `scripts/check-docs-links.mjs`, and the `docs` job in
  `.github/workflows/ci.yml`. The GitHub Wiki mirror added later reuses that guarantee rather than
  weakening it: `scripts/sync-github-wiki.mjs` is a one-way publisher, and its mapping check rides in
  the same `docs` job.
- The `wiki` job needs a `WIKI_TOKEN` secret to publish; without it the job is a no-op that says so.
  That is a deliberate trade — the wiki feature being enabled does not by itself grant the repository a
  credential that can write to another repository, and a missing credential should not fail a merge.
- The child tasks of [#165](https://github.com/TucanoTechnology/TucanoTestAPI/issues/165) author pages
  against this structure; this record unblocks them and is superseded only by another decision record
  under `docs/architecture/`.
