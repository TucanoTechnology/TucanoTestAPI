# Documentation Strategy and Wiki Structure

Status: accepted
Decision ticket: [API #170](https://github.com/TucanoTechnology/TucanoTestAPI/issues/170)
Epic: [API #165](https://github.com/TucanoTechnology/TucanoTestAPI/issues/165)

This document decides where user-facing documentation lives, how it is published, which pages
make up the wiki, and how the pages are kept from drifting away from the implementation. It is the
blocking decision for the wiki epic: pages are authored in the structure defined here.

## Context

The repository already documents itself, but for the wrong audience. `README.md` is the front door
for a *developer* working in the repository: it covers the storage concept, the module layout, the
test suites, and the local check commands. `docs/` holds engineering records — architecture,
compatibility contracts, security decisions, deployment procedures — written for maintainers and
operators who already know the system.

What the epic asks for is a different artefact: a **user-focused wiki** that lets a prospective
user understand what the product does, lets a new user install it and create a first project, lets
an existing user look up how a feature works, lets an API consumer make an authenticated call, and
lets a sysadmin run the thing in production. That is a support surface, not an engineering record.

Two constraints shape the decision:

- `AGENTS.md` is binding: `README.md` and `AGENTS.md` are the only documents at the repository
  root, every other document lives under `docs/<category>/`, and each one is linked from the README
  documentation table.
- `openapi.json` is normative for the HTTP contract. No user guide may restate a schema it can link
  to instead, or name an operation the document does not contain.

## Decision

**The wiki is a hand-written, in-repo documentation section published from `docs/wiki/`, linked
from the README documentation table, and served in rendered form through the repository's normal
Git-based browsing (GitHub's Markdown renderer). There is no separate GitHub Wiki, and no
generated static site.**

Concretely:

| Aspect | Decision |
| --- | --- |
| Home | `docs/wiki/` in this repository, one Markdown file per page |
| Index | `docs/wiki/README.md`, which links every wiki page |
| Publishing | committed Markdown, rendered by GitHub; no build step and no publish workflow |
| Readership | end users, API consumers, sysadmins/operators |
| Contract source of truth | `openapi.json` for HTTP; the code and `tests/` for behaviour |
| Engineering records | stay in `docs/architecture/`, `docs/contracts/`, `docs/deployment/`, `docs/security/` |

### Why not GitHub Wiki

GitHub's built-in Wiki is a separate Git repository. Nothing in a pull request to this repository
would even mention a wiki edit, so a code change and its documentation could never be reviewed
together — precisely the drift the epic is trying to prevent. It also cannot be reviewed by the
same required checks, cannot be linked with a relative path from `README.md`, and is not present
when the repository is cloned, so the docs would be lost to anyone working offline or from a
checkout. The pages would also be outside the `docs/<category>/` rule that `AGENTS.md` imposes.

### Why not a generated site (mdBook or similar)

A generator adds a toolchain, a build step, and a deployment target to produce Markdown that GitHub
already renders. The content is the same either way. Nothing in the current scope needs search
across pages, versioned snapshots, or a custom theme — and the maintenance cost of a build pipeline
is paid on every future edit. If the corpus ever outgrows a single index page, adding a generator on
top of `docs/wiki/` remains possible without moving any content, because the source would still be
plain Markdown files under one directory. The generator is therefore deferred, not rejected.

### Why not mirror the wiki into `docs/`

Duplicating pages between a wiki location and `docs/` guarantees that one of the two copies is
stale. There is exactly one copy of each page, at `docs/wiki/<page>.md`.

### How this reconciles with `AGENTS.md`

The rule is satisfied rather than worked around. `docs/wiki/` is an ordinary `docs/<category>`
directory; the README documentation table gains one entry pointing at the wiki index, and the index
points at every page. No document is added to the repository root, and no `AGENTS.md` text needs to
change.

## Information architecture

Five audiences, each with a landing page. Every page is a file under `docs/wiki/`; `README.md`
inside that directory is the index that links all of them.

| # | Page | File | Audience | Source of truth |
| --- | --- | --- | --- | --- |
| 0 | Wiki index | `docs/wiki/README.md` | all | the page list below |
| 1 | Installation and first project | `docs/wiki/getting-started.md` | new user | `README.md` (Application container, Authentication), `docker-compose.yml`, `Dockerfile` |
| 2 | Projects, suites, and cases | `docs/wiki/projects-suites-and-cases.md` | user | `openapi.json`, `README.md` (Storage concept) |
| 3 | Steps and attachments | `docs/wiki/steps-and-attachments.md` | user | `openapi.json`, `README.md` (Storage concept) |
| 4 | Composing and duplicating | `docs/wiki/composing-and-duplicating.md` | user | `openapi.json`, `src/domain/` |
| 5 | Tags and configurations | `docs/wiki/tags-and-configurations.md` | user | `openapi.json`, `README.md` (HTTP API) |
| 6 | Test runs and results | `docs/wiki/test-runs-and-results.md` | user | `openapi.json`, `README.md` (Storage concept) |
| 7 | Result imports and reports | `docs/wiki/imports-and-reports.md` | user | `openapi.json`, `tests/runs.rs`, `tests/reports.rs` |
| 8 | Milestones | `docs/wiki/milestones.md` | user | `openapi.json`, `src/domain/` |
| 9 | Case versioning and history | `docs/wiki/case-versioning-and-history.md` | user | `openapi.json`, [test-case-versioning-plan](contracts/test-case-versioning-plan.md) |
| 10 | API and authentication quickstart | `docs/wiki/api-and-authentication.md` | API consumer | `openapi.json`, [authentication-decision](security/authentication-decision.md) |
| 11 | Operations and troubleshooting | `docs/wiki/operations-and-troubleshooting.md` | sysadmin/operator | [deployment-guide](deployment/deployment-guide.md), [canary-validation](deployment/canary-validation-and-rollback.md), [threat-model](security/threat-model.md) |

Pages 2–9 are the feature guides of task
[API #172](https://github.com/TucanoTechnology/TucanoTestAPI/issues/172); page 1 is
[API #171](https://github.com/TucanoTechnology/TucanoTestAPI/issues/171); page 10 is
[API #173](https://github.com/TucanoTechnology/TucanoTestAPI/issues/173); page 11 is
[API #174](https://github.com/TucanoTechnology/TucanoTestAPI/issues/174).

Every page opens with a one-line statement of who it is for and what it lets them do, and ends with
a "Related" section linking the neighbouring pages and the engineering record it derives from.

## Drift prevention

The wiki is hand-written on purpose: there is no generator to run, so a page is edited in the same
pull request as the change that makes it wrong. The following rules make that enforceable.

1. **`openapi.json` is the single source of truth for the HTTP contract.** Wiki pages must not
   reproduce request or response schemas. They name operations exactly as `openapi.json` names them
   (method plus path), and link to `/api-docs` for the schema. A worked example may show a request
   body, but the field list lives in Swagger.
2. **Each page names its sources.** Every file begins by naming the engineering records it derives
   from. When one of those records changes, the page that cites it is expected to change with it.
3. **The index is exhaustive.** `docs/wiki/README.md` links every page. A page that is not linked
   from the index does not exist for a reader, and a PR adding a page without adding it to the index
   is incomplete.
4. **The README links the wiki index.** The documentation table in `README.md` carries exactly one
   entry for the wiki; adding a page does not require a README change, adding a *section* does.
5. **Behaviour claims are verified against a running instance.** A guide's commands and example
   payloads are exercised before merge; a page that cannot be run is marked as illustrative rather
   than presented as a procedure.
6. **CI coverage is stated honestly.** No check in `.github/workflows/` currently validates the
   wiki: there is no link checker and no Markdown linter, and this decision does not add one. The
   existing gates (`cargo fmt`, `clippy`, `test`, `build`, and `actionlint`) do not read `docs/`.
   Review is therefore the mechanism that keeps the wiki accurate, and reviewers are expected to
   apply rules 1–5. Adding a link checker is reasonable follow-up work if broken links become a
   recurring problem; it is not assumed to exist.

## Consequences

- The epic ships only Markdown plus one README table row and this decision record. No application
  code, no build step, no new dependency, and no new CI job is introduced.
- Reviewers see a documentation change in the same diff as the code change it accompanies.
- The corpus is fully readable from a clone, offline, with no tooling.
- If a generator is wanted later, `docs/wiki/` can be fed to it as-is.
- The wiki's accuracy depends on review discipline rather than tooling; that trade-off is accepted
  knowingly, and the drift rule above makes the expectation explicit.

## Related

- [README.md](../../README.md) — the repository front door and developer reference
- [AGENTS.md](../../AGENTS.md) — the repository rules this decision complies with
- [docs/deployment/deployment-guide.md](../deployment/deployment-guide.md) — the deployment model
  the operations page summarises
- [docs/security/authentication-decision.md](../security/authentication-decision.md) — the
  authentication decision the API quickstart summarises
