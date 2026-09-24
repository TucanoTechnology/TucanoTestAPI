# Demo / seed dataset specification

This document specifies the dataset the test-data generator seeds into a
deployment, the tree it produces below `TUCANO_DATA_DIR`, the API calls that
produce it, and what teardown removes. It is the contract the generator
implementation (the sibling children of the tracking issue) is written against,
and the checklist that decides whether the dataset still covers the feature set.

Nothing here is hand-written into the data directory. Every document, folder and
attachment in the target tree is produced by an HTTP call against the running
API, so the generator exercises the same rules a client does and cannot drift
into a shape the API would never write.

- **Tracking issue:** [#192](https://github.com/TucanoTechnology/TucanoTestAPI/issues/192)
- **Parent epic:** [#169 — Create test data generation script](https://github.com/TucanoTechnology/TucanoTestAPI/issues/169)
- **Coverage matrix:** [below](#1-feature-coverage-matrix)
- **Target tree:** [below](#2-target-tree-below-tucano_data_dir)
- **Generating calls:** [below](#3-generating-api-calls)
- **Teardown scope:** [below](#4-teardown-scope)

## Conventions used in this document

| Placeholder | Meaning |
| --- | --- |
| `$API` | Base URL of the deployment, e.g. `http://localhost:3000`. |
| `$TOKEN` | Access token from `POST /auth/login`, sent as `Authorization: Bearer $TOKEN`. |
| `<name>.json` | An identifier the API derived from a resource `name`. Named resources are always `<name>.json`; a test case is named by its own `testCaseId` verbatim. |

Identifiers are supplied explicitly in the call sequence below, and always in
the form the API itself would derive, so the target tree is deterministic and
the seeded identifiers are quoted in this document without a lookup step. Every
`<name>.json` identifier is a single path segment; a value that is not one is
rejected as `invalid_id`.

## 1. Feature coverage matrix

Each feature must have at least one seeded example, and each example names the
call that creates it and the on-disk evidence it leaves. A matrix row with no
example is a gap, not a deferral.

| # | Feature | Seeded example | Producing call | On-disk evidence |
| --- | --- | --- | --- | --- |
| 1 | Projects | `checkout.json` (tags, three suites and two directly owned cases) and `payments.json` (a suite, a directly owned case, and the copies composition places into it) | `POST /projects` ×2 | `projects/checkout/project.json`, `projects/payments/project.json` |
| 2 | Suites inside a project | `smoke.checkout.json`, `regression.checkout.json` (left holding no case at all) and `portable.checkout.json` in `checkout.json`; `smoke.payments.json` in `payments.json` | `POST /projects/{id}/test_suites` ×4, plus the suite half of row 22 | `projects/<p>/<suite>/suite.json` |
| 3 | Cases directly in a project | `TC-PROJECT-1` and `TC-ORDERS-1` in `checkout.json`; `TC-CATALOG-1` in `payments.json` | `POST /projects/{id}/test_cases` ×3 | `projects/<p>/<case>/test-case.json` |
| 4 | Cases inside a suite | `TC-LOGIN-1`, `TC-LOGIN-2`, `TC-CART-1` and `TC-MOVE-1` in `smoke.checkout.json`; `TC-SEARCH-1` in `smoke.payments.json` | `POST /test_suites/{id}/test_cases` ×5 | `projects/<p>/<suite>/<case>/test-case.json` |
| 5 | Steps on a case | every seeded case carries an ordered `steps` array — `TC-LOGIN-1` and `TC-LOGIN-2` have two steps each, the other six have one | `PUT /test_cases/{id}`, once per case (8 calls) | the `steps` array inside each `test-case.json` |
| 6 | Case attachments | one on every seeded case — `login-flow.txt` on `TC-LOGIN-1`, `move-trace.txt` on `TC-MOVE-1`, `checkout-page.txt` on `TC-PROJECT-1`, and one more per remaining case | `POST /test_cases/{id}/attachments` (multipart) ×8 | a `<stamp>-<file>` beside `test-case.json` in each case folder |
| 7 | Step attachments | seven uploads across six of the eight cases, e.g. `step-1.txt` on step index 0 of `TC-LOGIN-2` and `step-2.txt` on its step index 1 | `POST /test_cases/{id}/steps/{index}/attachments` ×7 | `…/<case>/steps/<index>/<stamp>-<file>` |
| 8 | Tags on projects, suites, cases and runs | `checkout`/`regression` on the project; `smoke` on the suite; `auth` on a case; `nightly` on the run | the create/update calls that carry `tags`, then the seeded run read back through the tags filter (`tags=nightly`) | the `tags` array in each stored document |
| 9 | Case versioning and revision history | every seeded case is updated once after creation by the `steps` write of row 5, so each one carries a revision | `PUT /test_cases/{id}` once per case, then `GET /test_cases/TC-LOGIN-2/history` | a `revisions/v1.json` in every case folder; `version`/`lastModified` in each `test-case.json` |
| 10 | Configurations (one per project) | `chrome-linux.json` in `checkout.json`; `firefox-linux.json` in `payments.json` | `POST /projects/{id}/configurations` ×2 | `projects/checkout/configurations/chrome-linux.json`, `projects/payments/configurations/firefox-linux.json` |
| 11 | Linking a configuration to a run | `chrome-linux.json` linked to `nightly.json` | `POST /test_runs/{id}/configurations` | the `configurations` reference array in `projects/checkout/test_runs/nightly.json` |
| 12 | Runs (point-in-time snapshots) | `nightly.json` covering the `checkout.json` project and its smoke suite, whose snapshot carries the four cases the suite held when it was added | `POST /projects/{id}/test_runs`, then `POST /test_runs/{id}/test_suites` | `projects/checkout/test_runs/nightly.json` |
| 13 | Run case membership pinned from a template | run carries `TC-LOGIN-1`, `TC-LOGIN-2` as copies | `POST /test_runs/{id}/test_cases` | `test_cases` array in `projects/checkout/test_runs/nightly.json` |
| 14 | Recorded results — every status | `TC-LOGIN-1` `Passed`, `TC-LOGIN-2` `Failed` (with notes and `durationMs`), `TC-PROJECT-1` `Blocked`, `TC-CART-1` `Retest`; `Untested` is never recorded — it is the status of a declared case with no result, so its bucket reads 0 | `POST /test_runs/{id}/results` per case (the route merges into an earlier result for the same case, and each case is one the run holds) | `results` array in `projects/checkout/test_runs/nightly.json` |
| 15 | Result re-record (merge) | `TC-LOGIN-2` recorded `Blocked`, then recorded again as `Failed`; the second call merges, so `status` is replaced and a field the second body leaves out would keep its stored value | same `POST /test_runs/{id}/results` twice | one `Failed` entry for `TC-LOGIN-2` |
| 16 | Defect links — all four trackers | one link per tracker type on the failed result of `TC-LOGIN-2` | `POST /test_runs/{id}/results/{case_id}/defects` ×4 | `defectLinks` array inside the `TC-LOGIN-2` result |
| 17 | Defect link removal | the GitHub link of row 16, linked then unlinked | `POST …/defects` then `DELETE …/defects/{link_id}` | the removed link is absent from `defectLinks` |
| 18 | JUnit XML import | `nightly-import.json` run, importing a fixture for two cases | `POST /test_runs/{id}/import/junit` | `results` array in `projects/checkout/test_runs/nightly-import.json` |
| 19 | JSON result import | `nightly-import.json`, importing `Passed` and `Failed` entries | `POST /test_runs/{id}/import/json` | `results` array in `projects/checkout/test_runs/nightly-import.json` |
| 20 | Milestones and derived progress | `v1.0.json` referencing `nightly.json` | `POST /projects/{id}/milestones`, then `GET /milestones/v1.0.json/progress` | `projects/checkout/milestones/v1.0.json` |
| 21 | Duplication | a suite duplicate kept under its project, its derived identifier read back from the project's suite listing | `POST /test_suites/smoke.checkout.json/duplicate` | `projects/checkout/<copy>/suite.json` |
| 22 | Copy vs. move composition | copy is the default: `TC-LOGIN-1` (suite → project), `TC-ORDERS-1` (project → project), `TC-CATALOG-1` (project → suite) and `TC-SEARCH-1` (suite → suite) are each placed into a second parent while the source keeps its home. `"mode":"move"` relocates instead: `TC-MOVE-1` passes through all four directions (`smoke.checkout.json` → `checkout.json` → `payments.json` → `regression.checkout.json` → `smoke.payments.json`) and ends in the suite of the other project, and `TC-PROJECT-1` moves onto the project that already owns it, a no-op. The suite `portable.checkout.json` is moved into `payments.json` and then copied back, so it ends with one home in each — every parent pair is covered by one copy and one move | `POST /projects/{id}/test_cases`, `POST /test_suites/{id}/test_cases` and `POST /projects/{id}/test_suites`, each carrying `testCaseId`/`suiteId`, with `mode` selecting move (`mode` omitted is copy) | the copies in `projects/payments/` and `projects/checkout/smoke.checkout/` beside their sources in `projects/checkout/` and `projects/payments/`; `projects/payments/smoke.payments/TC-MOVE-1/` where the four-hop move ends; `portable.checkout.json` under both projects |
| 23 | Coverage report | `GET /reports/coverage`, global and `?projectId=checkout.json` | the report routes | n/a (read-only — no new files) |
| 24 | Summary report | `GET /reports/summary` and `?configurationId=chrome-linux.json` | the report routes | n/a (read-only — no new files) |
| 25 | Auth users: system administrator | `admin` — the bootstrap account | `TUCANO_BOOTSTRAP_USERNAME`/`TUCANO_BOOTSTRAP_PASSWORD` at startup; `POST /auth/login` | `auth/users.json` (**not** produced by an API call — see [§5](#5-known-gap-auth-accounts-and-role-grants)) |
| 26 | Auth users: non-admin accounts | `viewer` and `editor` — two stored accounts with no `systemAdmin` flag | written through the `AuthStore` path by the generator; `POST /auth/login` | `auth/users.json` (**not** produced by an API call) |
| 27 | Role grants per project | `viewer` holds `owner` on `checkout.json` and `editor` holds `editor` on the same project; both hold **no grant at all** on `payments.json`. `admin` holds **no grant at all** — a system administrator is authorized without one (see [§5](#5-known-gap-auth-accounts-and-role-grants)) | written through the `AuthStore` grant path; verified by `GET /auth/me` as `viewer` and as `editor`, and for `admin` by an authorized write and by `systemAdmin: true` | `auth/projects/checkout.json` holding both grants (and no `auth/projects/payments.json`) |
| 28 | Authorization enforcement | an `editor` token writes the content inside its project — `POST /projects/{id}/test_suites` is accepted — while `PUT /projects/{id}` on the same project is refused with `forbidden`, because the project document needs `owner`. The `viewer`-scoped token proves reads succeed and a write needing a grant it does not hold is refused | any guarded write with the scoped token | n/a (the acceptance and the refusal are the evidence) |
| 29 | Sessions | sign in, refresh (rotating the refresh token once), sign out, `GET /auth/me` | `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`, `GET /auth/me` | n/a (`auth/users.json` carries the revocable refresh tokens) |
| 30 | Service surface | `GET /health`, `GET /ready`, `GET /diagnostics`, `GET /openapi.json`, `GET /metrics` | the service routes | n/a (read-only) |

Rows 23, 24, 28, 29 and 30 are coverage of behaviour rather than of stored
documents: their evidence is the response the call returns, and they exist so
the generator's validation step has something to assert beyond the file tree.

### Stale-matrix check

The matrix is only checkable if something fails when it goes stale. Two checks
share that job, and they are deliberately split by cost (issue #195):

| Check | Where it runs | What it catches |
| --- | --- | --- |
| Route coverage (below) | every pull request, in CI | a feature route that exists in `openapi.json` but reaches no matrix row, and a row that names a route the contract does not publish |
| Seed validation ([§3 step 12](#step-12--validation-of-the-seeded-environment)) | the local one-command path | a producing call that fails, or on-disk evidence a row promises but the seed did not leave |

The seeded half must assert, for every row marked with a producing call, that
the call succeeds and that the named on-disk evidence exists after the seed run
— and must fail, loudly and non-zero, when a row's evidence is missing. It needs
a running stack, so it is **not** a CI job: starting a service on every pull
request was considered and rejected on CI cost and machine load, and the
one-command path in [§7](#7-implementation-status) is where it lives instead.

### Route coverage

The static half runs as a dependency-free check over two files — `openapi.json`
and this document — with no build, no network and no started service. It fails,
non-zero, on either of these:

1. **Route without a row.** An operation in `openapi.json` no matrix row
   accounts for, and which the exemption table below does not exempt.
2. **Row without a route.** A producing call in a row that resolves to no
   operation in `openapi.json` — a typo, or a route that was renamed away.

Matching is by resolved route shape: a matrix entry such as
`POST /test_suites/{id}/test_cases` matches the contract's operation of the same
method and segment count, where a `{…}` parameter matches whatever the document
wrote in that position, so `{index}` and `{step_index}` are the same slot.

A row's *Seeded example* and *Producing call* columns are both read, because the
"Producing call" column names the writing call and some features are read-only.

#### Exempt operations

The contract publishes operations that no seed can exercise, because they are
the read, replace and delete companions of a route the seed does create, or they
are not part of the seeded data model at all. Exempting them is a decision, not
a default, so each one is listed here with its reason. The check reads this
table; an operation that is neither accounted for by a row nor listed here fails
the check.

| Operation | Why no seeded example |
| --- | --- |
| `GET /configurations` | list companion of the configuration routes. The seeded configurations live in their projects (row 10); [§3 step 12](#step-12--validation-of-the-seeded-environment) reads them back through this filtered listing as well as through each project's own |
| `GET /configurations/{id}` | read companion of row 10, and the global document route [§3 step 12](#step-12--validation-of-the-seeded-environment) reads each seeded configuration back through |
| `PUT /configurations/{id}` | replace companion of row 10 |
| `DELETE /configurations/{id}` | delete companion of row 10; teardown removes a seeded configuration through its project's own delete instead ([§4](#4-teardown-scope)) |
| `GET /milestones` | list companion of `POST /projects/{id}/milestones` (row 20); each seeded milestone is read back through its project's own listing |
| `GET /milestones/{id}` | read companion of row 20 |
| `PUT /milestones/{id}` | replace companion of row 20 |
| `DELETE /milestones/{id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `POST /milestones/{id}/duplicate` | duplication is seeded for a suite (row 21); the same route shape for milestones is not |
| `GET /projects` | list companion of `POST /projects` (row 1) |
| `GET /projects/{id}` | read companion of row 1 |
| `PUT /projects/{id}` | replace companion of row 1 |
| `DELETE /projects/{id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `POST /projects/{id}/duplicate` | duplication is seeded for a suite (row 21), not for a project |
| `GET /projects/{id}/configurations` | list companion of `POST /projects/{id}/configurations` (row 10); [§3 step 12](#step-12--validation-of-the-seeded-environment) reads each project's configuration back through it |
| `DELETE /projects/{id}/configurations/{config_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `GET /projects/{id}/milestones` | list companion of `POST /projects/{id}/milestones` (row 20); [§3 step 12](#step-12--validation-of-the-seeded-environment) reads the project's milestone back through it |
| `DELETE /projects/{id}/milestones/{milestone_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `GET /projects/{id}/test_runs` | list companion of `POST /projects/{id}/test_runs` (row 12); [§3 step 12](#step-12--validation-of-the-seeded-environment) reads each project's runs back through it |
| `DELETE /projects/{id}/test_runs/{run_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `DELETE /projects/{id}/test_cases/{case_id}` | teardown-scope call; placement is row 22 |
| `GET /projects/{id}/test_cases` | list companion of the case routes; [§3 step 12](#step-12--validation-of-the-seeded-environment) reads each parent's cases back through it — both projects' listings, with `payments.json` carrying the copies composition placed into it |
| `GET /projects/{id}/test_cases/{case_id}/attachments/{filename}` | read-back companion of row 6, addressed through the project that holds the case: the seed uploads and reads back through the bare route, which reaches a case by its identifier alone, while every case it attaches to still has one home |
| `DELETE /projects/{id}/test_cases/{case_id}/attachments/{filename}` | teardown is scoped to the case folder, not to individual attachments |
| `GET /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | read companion of the step-attachment upload (row 7), addressed through the project that holds the case |
| `DELETE /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}` | teardown is scoped to the case folder |
| `POST /projects/{id}/test_cases/{case_id}/attachments` | upload companion of row 6, addressed through the project that holds the case: the seed uploads through the bare route, which reaches a case by its identifier alone, while every case it uploads to still has one home |
| `POST /projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | upload companion of row 7, addressed through the project that holds the case |
| `GET /projects/{id}/test_suites` | list companion of `POST /projects/{id}/test_suites` (row 2); the seed reads this listing to learn the duplicate's derived identifier, and after row 22 `portable.checkout.json` appears in **both** projects' listings |
| `DELETE /projects/{id}/test_suites/{suite_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `GET /test_cases/{id}` | read companion of the case routes, but only for a case with one home. Row 22 composes four cases — `TC-LOGIN-1`, `TC-ORDERS-1`, `TC-CATALOG-1` and `TC-SEARCH-1` — each into a second parent while its source keeps its home, so a bare `GET /test_cases/{id}` for any of the four is answered `409` by design and [§3 step 12](#step-12--validation-of-the-seeded-environment) asserts that refusal and reads them back through their parents' listings instead |
| `DELETE /test_cases/{id}` | teardown-scope call, by parent ([§4](#4-teardown-scope)) |
| `GET /test_cases/{id}/attachments/{filename}` | read-back companion of row 6 |
| `DELETE /test_cases/{id}/attachments/{filename}` | teardown is scoped to the case folder, not to individual attachments |
| `POST /test_cases/{id}/duplicate` | duplication is seeded for a suite (row 21); a case is placed, not duplicated (row 22) |
| `GET /test_cases/{id}/history/{version}` | read companion of `GET /test_cases/{id}/history` (row 9) |
| `GET /test_cases/{id}/steps/{step_index}/attachments` | read companion of the step-attachment upload (row 7) |
| `DELETE /test_cases/{id}/steps/{step_index}/attachments/{filename}` | teardown is scoped to the case folder |
| `GET /test_runs` | list companion of `POST /projects/{id}/test_runs` (row 12); [§3 step 12](#step-12--validation-of-the-seeded-environment) and the filter assertions ([§3 step 12](#step-12--validation-of-the-seeded-environment)) read it across every reachable project |
| `GET /test_runs/{id}` | read companion of row 12; [§3 step 12](#step-12--validation-of-the-seeded-environment) reads both seeded runs back |
| `PUT /test_runs/{id}` | replace companion of row 12 |
| `DELETE /test_runs/{id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `DELETE /test_runs/{id}/configurations/{config_id}` | unlink companion of row 11; the seed links without unlinking |
| `POST /test_runs/{id}/duplicate` | duplication is seeded for a suite (row 21), not for a run |
| `GET /test_runs/{id}/results/{case_id}/defects` | list companion of the defect-link creation (row 16) |
| `DELETE /test_runs/{id}/results/{case_id}/defects/{link_id}` | row 17 unlinks the GitHub link the seed created; it writes the route with the link id it read back from the create response, which the check matches as the same shape |
| `GET /test_suites/{id}` | read companion of the suite routes, exercised only for a suite with one home; [§3 step 12](#step-12--validation-of-the-seeded-environment) reads the single-homed seeded suites back this way and asserts that the dual-homed `portable.checkout.json` is refused `409` |
| `PUT /test_suites/{id}` | replace companion of row 2 |
| `DELETE /test_suites/{id}` | teardown-scope call, by parent ([§4](#4-teardown-scope)) |
| `GET /test_suites/{id}/test_cases` | list companion of `POST /test_suites/{id}/test_cases` (row 4), exercised for the single-homed suites; the dual-homed `portable.checkout.json` is refused `409`, which [§3 step 12](#step-12--validation-of-the-seeded-environment) asserts |
| `DELETE /test_suites/{id}/test_cases/{case_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `GET /test_suites/{id}/test_cases/{case_id}/attachments/{filename}` | read-back companion of row 6, addressed through the suite that holds the case; the seed reaches a case by its identifier alone while every case it attaches to still has one home |
| `DELETE /test_suites/{id}/test_cases/{case_id}/attachments/{filename}` | teardown is scoped to the case folder, not to individual attachments |
| `GET /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | read companion of the step-attachment upload (row 7), addressed through the suite that holds the case |
| `DELETE /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}` | teardown is scoped to the case folder |
| `POST /test_suites/{id}/test_cases/{case_id}/attachments` | upload companion of row 6, addressed through the suite that holds the case: the seed uploads through the bare route, which reaches a case by its identifier alone, while every case it uploads to still has one home |
| `POST /test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | upload companion of row 7, addressed through the suite that holds the case |
| `GET /api-docs` | the Swagger UI page, not part of the data model |
| `GET /openapi.json` | the contract itself; row 30 covers it as a service-surface assertion |

Teardown-scope operations count as accounted for because [§4](#4-teardown-scope)
is the document that specifies them, and the check verifies they reach it.

## 2. Target tree below `TUCANO_DATA_DIR`

The tree below is what a successful seed run leaves behind, with the fixed
identifiers this document specifies. `<stamp>` is the unique prefix the API puts
in front of a stored attachment's filename, and `<suffix>` is the identifier the
API's duplicate route derives; both are read back from the API's own response
rather than assumed.

```
$TUCANO_DATA_DIR/
├── auth/
│   ├── users.json                            # accounts, password hashes, refresh tokens
│   └── projects/
│       └── checkout.json                     # {"grants": {"<viewer id>": "owner", "<editor id>": "editor"}}
├── .tucano.lock                              # advisory lock, created by the API
├── projects/
│   ├── checkout/
│   │   ├── project.json                      # {"projectId","name","tags":["checkout","regression"]}
│   │   ├── smoke.checkout/                   # a suite folder: three originals, two placed copies
│   │   │   ├── suite.json
│   │   │   ├── TC-LOGIN-1/
│   │   │   │   ├── test-case.json            # tags, ordered steps, version 2
│   │   │   │   ├── revisions/
│   │   │   │   │   └── v1.json               # snapshot written by the qualifying steps update
│   │   │   │   ├── <stamp>-login-flow.txt    # case attachment
│   │   │   │   └── steps/
│   │   │   │       └── 0/
│   │   │   │           └── <stamp>-step-1.txt        # step attachment
│   │   │   ├── TC-LOGIN-2/                   # the locked-account case
│   │   │   │   ├── test-case.json            # carries two ordered steps
│   │   │   │   ├── revisions/
│   │   │   │   │   └── v1.json
│   │   │   │   ├── <stamp>-lock-message.txt
│   │   │   │   └── steps/
│   │   │   │       ├── 0/
│   │   │   │       │   └── <stamp>-step-1.txt
│   │   │   │       └── 1/
│   │   │   │           └── <stamp>-step-2.txt
│   │   │   ├── TC-CART-1/
│   │   │   │   ├── test-case.json
│   │   │   │   ├── revisions/
│   │   │   │   │   └── v1.json
│   │   │   │   ├── <stamp>-cart-state.txt
│   │   │   │   └── steps/
│   │   │   │       └── 0/
│   │   │   │           └── <stamp>-step-2.txt
│   │   │   ├── TC-CATALOG-1/                 # copied here from payments.json, which keeps
│   │   │   │   ├── test-case.json            # the source home; no step attachment
│   │   │   │   ├── revisions/
│   │   │   │   │   └── v1.json
│   │   │   │   └── <stamp>-catalog-snapshot.txt
│   │   │   └── TC-SEARCH-1/                  # copied here from smoke.payments.json
│   │   │       ├── test-case.json
│   │   │       ├── revisions/
│   │   │       │   └── v1.json
│   │   │       ├── <stamp>-search-response.txt
│   │   │       └── steps/
│   │   │           └── 0/
│   │   │               └── <stamp>-step-1.txt
│   │   ├── regression.checkout/              # second suite in this project, a move waypoint
│   │   │   └── suite.json                    # left holding no case — the empty-suite shape
│   │   ├── portable.checkout/                # moved into payments.json, then copied back
│   │   │   └── suite.json                    # created empty: a suite placement carries its cases
│   │   ├── smoke.checkout-copy-<suffix>/     # the duplicate suite, id from the response
│   │   │   └── suite.json
│   │   ├── TC-PROJECT-1/                     # a case owned directly by the project, which
│   │   │   ├── test-case.json                # the no-op move in §3, step 11 leaves here
│   │   │   ├── revisions/
│   │   │   │   └── v1.json
│   │   │   └── <stamp>-checkout-page.txt     # no step attachment on this case
│   │   ├── TC-ORDERS-1/                      # owned directly by the project and copied into
│   │   │   ├── test-case.json                # payments.json, which keeps this source home
│   │   │   ├── revisions/
│   │   │   │   └── v1.json
│   │   │   ├── <stamp>-orders-payload.txt
│   │   │   └── steps/
│   │   │       └── 0/
│   │   │           └── <stamp>-step-2.txt
│   │   ├── test_runs/                        # reserved child of this project
│   │   │   ├── nightly.json                  # suite snapshot, pinned cases, results, defects, config link
│   │   │   └── nightly-import.json           # results arrived by import
│   │   ├── milestones/                       # reserved child of this project
│   │   │   └── v1.0.json                     # references nightly.json
│   │   └── configurations/                   # reserved child of this project
│   │       └── chrome-linux.json
│   └── payments/
│       ├── project.json
│       ├── smoke.payments/                   # a suite folder; the four-hop move ends in it
│       │   ├── suite.json
│       │   ├── TC-MOVE-1/                    # the whole folder travelled, steps and attachments intact
│       │   │   ├── test-case.json
│       │   │   ├── revisions/
│       │   │   │   └── v1.json
│       │   │   ├── <stamp>-move-trace.txt
│       │   │   └── steps/
│       │   │       └── 0/
│       │   │           └── <stamp>-step-1.txt
│       │   └── TC-SEARCH-1/                  # copied into smoke.checkout.json; this stays the source
│       │       ├── test-case.json
│       │       ├── revisions/
│       │       │   └── v1.json
│       │       ├── <stamp>-search-response.txt
│       │       └── steps/
│       │           └── 0/
│       │               └── <stamp>-step-1.txt
│       ├── portable.checkout/                # the copy placed back out of checkout.json
│       │   └── suite.json
│       ├── TC-LOGIN-1/                       # a copy placed into this project; the source keeps
│       │   ├── test-case.json                # its home in smoke.checkout.json
│       │   ├── revisions/
│       │   │   └── v1.json
│       │   ├── <stamp>-login-flow.txt        # a copy carries the steps and attachments too
│       │   └── steps/
│       │       └── 0/
│       │           └── <stamp>-step-1.txt
│       ├── TC-ORDERS-1/                      # a copy placed into this project
│       │   ├── test-case.json
│       │   ├── revisions/
│       │   │   └── v1.json
│       │   ├── <stamp>-orders-payload.txt
│       │   └── steps/
│       │       └── 0/
│       │           └── <stamp>-step-2.txt
│       ├── TC-CATALOG-1/                     # owned directly by this project and copied into
│       │   ├── test-case.json                # smoke.checkout.json, which keeps this home
│       │   ├── revisions/
│       │   │   └── v1.json
│       │   └── <stamp>-catalog-snapshot.txt
│       └── configurations/                   # reserved child of this project
│           └── firefox-linux.json
```

Three details of this tree are easy to get wrong and are called out
deliberately:

- **A project and a suite always contain what the API put there, and nothing
  else.** Suites are folders inside their project; cases are folders inside a
  project or inside a suite. There is no top-level pool of standalone entities,
  and the generator must never create one. On top of its entries a project owns
  three folder children holding its runs, its milestones and its configurations;
  `test_runs`, `milestones` and `configurations` are reserved names inside a
  project folder, so neither a suite nor a case may take one.
- **`test_runs/`, `milestones/` and `configurations/` are still single
  documents, not folders.** A run, milestone or configuration is one `<id>.json`
  file, never a folder, and none of them owns a subtree — but the folder holding
  it is a child of the project that owns it, not the root of the data directory.
- **A placement carries the whole folder, and only `move` removes a home.** A
  case folder is its document plus `revisions/`, `steps/` and its attachment
  files, and composition acts on the folder, not on the document: `copy`
  duplicates all of it under the target parent while the source keeps its home,
  and `move` relocates it so the target becomes the case's only physical home.
  A suite placement works the same way at suite scale, which is why
  `portable.checkout.json` is created empty — the cases inside a suite would
  travel with it. An identifier with two homes no longer resolves to one, so
  the four copies above are addressed through their parents' listings rather
  than by a bare `GET /test_cases/{id}` (see §1).

## 3. Generating API calls

Every step is an HTTP call with the token from step 0. Steps 1–4 must run in
order — step 2 puts a configuration into a project step 1 creates, and steps 3
and 4 create their suites and cases inside those projects; the later steps
depend only on the resources named in them. Two steps cannot be moved earlier:
step 5 attaches files to a case by its bare identifier, and step 11 composes
cases and a suite into a second parent, both of which are only unambiguous
while the target still has a single home — so the placements run last, after
every lookup by identifier is finished.

### Step 0 — session

Auth is optional at runtime: the service's own default is off, and the shipped
`docker-compose.yml` turns it on with the four settings below drawn from `.env`.
Either way the seed needs it on, because the whole sequence below carries a
token and step 1 creates a project, which only a system administrator may do.
The deployment must therefore be started with the four settings this step
depends on:

| Variable | Value the seed needs |
| --- | --- |
| `TUCANO_AUTH_REQUIRED` | `true`, so every guarded route enforces a token. |
| `TUCANO_JWT_SECRET` (or `TUCANO_JWT_SECRET_FILE`) | A signing secret of at least 32 bytes. |
| `TUCANO_BOOTSTRAP_USERNAME` | Together, the account created once on a store that already holds no accounts. |
| `TUCANO_BOOTSTRAP_PASSWORD` | The password for that account. |

With `TUCANO_AUTH_REQUIRED` off, a server starts with no signing secret at all
and every `POST /auth/login` answers `storage_error` ("auth is not configured
with a signing secret") instead of a session, so step 0 must run first and fail
loudly.

```sh
curl -sS -X POST "$API/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"username":"'"$TUCANO_BOOTSTRAP_USERNAME"'","password":"'"$TUCANO_BOOTSTRAP_PASSWORD"'"}'
```

The response carries `accessToken`, `refreshToken`, `tokenType` and
`expiresIn`. Send `Authorization: Bearer <accessToken>` on every call below.

### Step 1 — projects

Only a caller with the system-administrator flag may create a project, which is
why the seed uses the bootstrap account here.

```sh
curl -sS -X POST "$API/projects" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"projectId":"checkout.json","name":"checkout","description":"Checkout demo project","tags":["checkout","regression"]}'
curl -sS -X POST "$API/projects" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"projectId":"payments.json","name":"payments"}'
```

A project is created with no configurations, runs or milestones: those are
added to it by the steps below.

### Step 2 — configurations inside the project that owns them

A configuration belongs to one project, so it is created through that project's
own route, after step 1 has created the project. `chrome-linux.json` belongs to
`checkout.json`, which is the project the seed's run executes, and
`firefox-linux.json` belongs to `payments.json`; neither project can see the
other's configuration.

```sh
curl -sS -X POST "$API/projects/checkout.json/configurations" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"configId":"chrome-linux.json","name":"chrome-linux"}'
curl -sS -X POST "$API/projects/payments.json/configurations" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"configId":"firefox-linux.json","name":"firefox-linux","browser":"firefox","os":"linux"}'
```

`POST /configurations` is retired and answers 400; a configuration is always
created through the project it belongs to.

### Step 3 — suites inside their projects

Two suites are created empty and stay that way: `regression.checkout` is the
suite `TC-MOVE-1` passes through in step 11, and `portable.checkout` is the
suite step 11 places into the other project. A suite carries its cases with it
when it is placed, so leaving them empty keeps the placement to one folder.

```sh
curl -sS -X POST "$API/projects/checkout.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"smoke.checkout"}'
curl -sS -X POST "$API/projects/checkout.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"regression.checkout"}'
curl -sS -X POST "$API/projects/checkout.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"portable.checkout"}'
curl -sS -X POST "$API/projects/payments.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"smoke.payments"}'
```

`POST /test_suites` and `POST /test_cases` are retired and answer 400; a suite or
a case is always created through the parent it lives in.

### Step 4 — cases in both parents, and their steps

```sh
# Inside a suite: a body with `title` creates.
curl -sS -X POST "$API/test_suites/smoke.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-LOGIN-1","title":"Sign in with a valid account","expectedResult":"Session is established","tags":["auth"]}'
curl -sS -X POST "$API/test_suites/smoke.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-LOGIN-2","title":"Sign in with a locked account","expectedResult":"Sign-in is refused with a message"}'
curl -sS -X POST "$API/test_suites/smoke.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-CART-1","title":"Add an item to the cart","expectedResult":"Cart shows one item"}'
curl -sS -X POST "$API/test_suites/smoke.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-MOVE-1","title":"Keep an item in the cart across sign-in","expectedResult":"Cart still shows the item after signing in"}'

# Directly inside a project: the same route shape, a different parent.
curl -sS -X POST "$API/projects/checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-PROJECT-1","title":"Reach the checkout page","expectedResult":"Checkout page renders"}'
curl -sS -X POST "$API/projects/checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-ORDERS-1","title":"List the orders of an account","expectedResult":"Every order of the account is listed with its status"}'

# The other project's suite and its directly owned case.
curl -sS -X POST "$API/test_suites/smoke.payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-SEARCH-1","title":"Search the catalog for an item","expectedResult":"The matching item is returned"}'
curl -sS -X POST "$API/projects/payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-CATALOG-1","title":"Browse the catalog page by page","expectedResult":"Each page holds the page size asked for"}'

# Steps are the ordered `steps` array on the case, written by an update: one
# per case, carrying that case's own steps.
curl -sS -X PUT "$API/test_cases/TC-LOGIN-1" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"steps":[{"action":"Open the sign-in form","expectedResult":"The form is shown"},{"action":"Submit a valid account","expectedResult":"The dashboard is shown"}]}'
curl -sS -X PUT "$API/test_cases/TC-CART-1" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"steps":[{"action":"Add an item to the cart","expectedResult":"The cart badge shows 1"}]}'
```

The seed issues one `PUT /test_cases/{id}` per case, eight in all:
`TC-LOGIN-1` and `TC-LOGIN-2` carry two steps and the other six one. Every one
of them is a qualifying update: it stamps `version` and `lastModified` and
writes the first snapshot to `revisions/v1.json`, which is what row 9 of the
matrix checks. The writes happen here rather than later because they address
the case by its bare identifier, which stops being unambiguous once step 11
has given a case a second home.

### Step 5 — attachments

```sh
curl -sS -X POST "$API/test_cases/TC-LOGIN-1/attachments" -H "Authorization: Bearer $TOKEN" \
  -F 'file=@fixtures/login-flow.txt;type=text/plain'
curl -sS -X POST "$API/test_cases/TC-LOGIN-2/steps/0/attachments" -H "Authorization: Bearer $TOKEN" \
  -F 'file=@fixtures/step-1.txt;type=text/plain'
curl -sS -X POST "$API/test_cases/TC-LOGIN-2/steps/1/attachments" -H "Authorization: Bearer $TOKEN" \
  -F 'file=@fixtures/step-2.txt;type=text/plain'
```

Every one of the eight cases gets a case-level attachment — `login-flow.txt` on
`TC-LOGIN-1`, `lock-message.txt` on `TC-LOGIN-2`, `cart-state.txt` on
`TC-CART-1`, `move-trace.txt` on `TC-MOVE-1`, `checkout-page.txt` on
`TC-PROJECT-1`, `orders-payload.txt` on `TC-ORDERS-1`,
`catalog-snapshot.txt` on `TC-CATALOG-1` and `search-response.txt` on
`TC-SEARCH-1` — so fifteen uploads in all: eight on cases and seven on steps,
spread over six of the eight cases (`TC-PROJECT-1` and `TC-CATALOG-1` carry
none). Each upload must land before step 11, while the case still has one home.

A multipart body with no file part answers `missing_file`; the generator's
fixtures are small text files committed beside it.

### Step 6 — the run, its pinned membership and its configuration link

The run is created inside the project that owns it, `checkout.json`. Its
`projects` array stays: it records which projects the run covered, which is not
the same thing as where the run lives.

```sh
curl -sS -X POST "$API/projects/checkout.json/test_runs" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testRunId":"nightly.json","name":"nightly","timestamp":"1757800000","tags":["nightly"],"projects":[{"projectId":"checkout.json","name":"checkout","testSuites":[]}]}'

curl -sS -X POST "$API/test_runs/nightly.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"suiteId":"smoke.checkout.json"}'
curl -sS -X POST "$API/test_runs/nightly.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-LOGIN-1"}'
curl -sS -X POST "$API/test_runs/nightly.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-PROJECT-1"}'

curl -sS -X POST "$API/test_runs/nightly.json/configurations" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"configId":"chrome-linux.json"}'
```

Adding a suite or a case to a run records a **copy**, so the source keeps its
home and the run's `caseVersions` pins the revision it saw. Nothing here moves
the source cases.

### Step 7 — results, including the re-record and every status

`POST /test_runs/{id}/results` is the only route that writes a result, and it
only accepts a case the run holds. A second call for the same case in the same
run **merges** into the first: `status` and `timestamp` are replaced, and a
field the body leaves out keeps its stored value. That is how the merge in row
15 is seeded — the second `TC-LOGIN-2` call carries `notes` and `durationMs`
explicitly, so nothing is left to carry over.

```sh
# Passed
curl -sS -X POST "$API/test_runs/nightly.json/results" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-LOGIN-1","status":"Passed","notes":"signed in","durationMs":1200}'
# Blocked first, then re-recorded as Failed — one stored result, merged.
curl -sS -X POST "$API/test_runs/nightly.json/results" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-LOGIN-2","status":"Blocked"}'
curl -sS -X POST "$API/test_runs/nightly.json/results" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-LOGIN-2","status":"Failed","notes":"lock message missing","durationMs":800}'
curl -sS -X POST "$API/test_runs/nightly.json/results" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-PROJECT-1","status":"Blocked"}'
curl -sS -X POST "$API/test_runs/nightly.json/results" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-CART-1","status":"Retest"}'
```

`Untested` is deliberately not recorded anywhere: it is the status a case has
when it is declared in the run and no result has been written for it, so the
milestone progress report derives it rather than being told it.

### Step 8 — defect links, one per tracker, then one removed

```sh
LINK=$(curl -sS -X POST "$API/test_runs/nightly.json/results/TC-LOGIN-2/defects" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"defectId":"BUG-101","defectUrl":"https://acme.atlassian.net/browse/BUG-101","trackerType":"jira","title":"Lock message missing","status":"Open"}')

curl -sS -X POST "$API/test_runs/nightly.json/results/TC-LOGIN-2/defects" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"defectId":"101","defectUrl":"https://github.com/acme/checkout/issues/101","trackerType":"github"}'
curl -sS -X POST "$API/test_runs/nightly.json/results/TC-LOGIN-2/defects" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"defectId":"42","defectUrl":"https://gitlab.com/acme/checkout/-/issues/42","trackerType":"gitlab"}'
curl -sS -X POST "$API/test_runs/nightly.json/results/TC-LOGIN-2/defects" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"defectId":"OPS-7","defectUrl":"https://tracker.example/OPS-7","trackerType":"custom"}'

# The GitHub link above is then unlinked, so the tree keeps three of the four
# tracker types plus evidence that removal works. The create response body is
# {"message":"Defect linked to test result","id":"<link id>"}, so the generator
# reads the link id from the `id` key — never a value it invented.
curl -sS -X DELETE "$API/test_runs/nightly.json/results/TC-LOGIN-2/defects/<link id>" \
  -H "Authorization: Bearer $TOKEN"
```

`linkId` and `linkedAt` are derived by the API and rejected if a client supplies
them. A link whose URL does not match its tracker type, and a second link to the
same `defectId`, are both 400/409 respectively and are exercised by the
generator's negative checks, not by the seed itself.

### Step 9 — the imported run

The imported run lives in `checkout.json` too, so it is created through the same
project route as `nightly.json`, under its own identifier.

```sh
curl -sS -X POST "$API/projects/checkout.json/test_runs" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testRunId":"nightly-import.json","name":"nightly-import","tags":["nightly"],"projects":[{"projectId":"checkout.json","name":"checkout","testSuites":[]}]}'

curl -sS -X POST "$API/test_runs/nightly-import.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-LOGIN-1"}'
curl -sS -X POST "$API/test_runs/nightly-import.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-CART-1"}'

curl -sS -X POST "$API/test_runs/nightly-import.json/import/json" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"results":[{"testCaseId":"TC-LOGIN-1","status":"Passed"},{"testCaseId":"TC-CART-1","status":"Failed","notes":"item missing"}]}'
curl -sS -X POST "$API/test_runs/nightly-import.json/import/junit" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/xml' --data-binary @fixtures/junit-nightly.xml
```

An import accepts only `Passed`, `Failed` and `Blocked`; `Untested` and `Retest`
are answered `invalid_status`. The JUnit fixture deliberately contains both a
passing and a failing case so the import and the run's own results agree.

### Step 10 — milestone and duplication

The milestone lives in the run's project, `checkout.json`, and references the
seeded run by identifier.

```sh
curl -sS -X POST "$API/projects/checkout.json/milestones" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"milestoneId":"v1.0.json","name":"v1.0","status":"open","testRunIds":["nightly.json"]}'
curl -sS -X GET "$API/milestones/v1.0.json/progress" -H "Authorization: Bearer $TOKEN"

curl -sS -X POST "$API/test_suites/smoke.checkout.json/duplicate" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{}'
curl -sS -X GET "$API/projects/checkout.json/test_suites" -H "Authorization: Bearer $TOKEN"
```

The milestone must reference at least one project through `testSuiteIds` or
`testRunIds`; the referenced run supplies the project, so the seed needs no
separate grant step here. The duplicate's identifier is derived and is read back
from the listing call.

### Step 11 — placement (copy is the default)

The four copies come first, one per parent pair, each with `mode` omitted:

```sh
curl -sS -X POST "$API/projects/payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-LOGIN-1"}'
curl -sS -X POST "$API/projects/payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-ORDERS-1"}'
curl -sS -X POST "$API/test_suites/smoke.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-CATALOG-1"}'
curl -sS -X POST "$API/test_suites/smoke.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-SEARCH-1"}'
```

A copy duplicates the case folder into the target parent and leaves the source
where it was, so `TC-LOGIN-1` now sits both inside
`projects/checkout/smoke.checkout/` where it was created and inside
`projects/payments/`. The table is the whole copy half of the matrix:

| Case | Direction | Source | Target |
| --- | --- | --- | --- |
| `TC-LOGIN-1` | suite → project | `projects/checkout/smoke.checkout/` | `projects/payments/` |
| `TC-ORDERS-1` | project → project | `projects/checkout/` | `projects/payments/` |
| `TC-CATALOG-1` | project → suite | `projects/payments/` | `projects/checkout/smoke.checkout/` |
| `TC-SEARCH-1` | suite → suite | `projects/payments/smoke.payments/` | `projects/checkout/smoke.checkout/` |

`TC-MOVE-1` then walks all four move directions. It keeps one identifier and
one home throughout, which is what makes the chain possible: each call
relocates the folder, so the next call still resolves the identifier to exactly
one parent.

```sh
curl -sS -X POST "$API/projects/checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-MOVE-1","mode":"move"}'
curl -sS -X POST "$API/projects/payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-MOVE-1","mode":"move"}'
curl -sS -X POST "$API/test_suites/regression.checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-MOVE-1","mode":"move"}'
curl -sS -X POST "$API/test_suites/smoke.payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-MOVE-1","mode":"move"}'
```

Those four calls are suite → project, project → project, project → suite and
suite → suite, and they end with the case in
`projects/payments/smoke.payments/`, its `revisions/v1.json` and both
attachments intact: a move relocates the whole folder rather than the document
alone. `regression.checkout.json` is where the third hop leaves it and the
fourth takes it away, so it ends empty.

```sh
# A move onto the parent that already holds the case: accepted, and a no-op.
curl -sS -X POST "$API/projects/checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-PROJECT-1","mode":"move"}'

# A placeable suite, created empty in step 3 precisely for these two calls.
curl -sS -X POST "$API/projects/payments.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"suiteId":"portable.checkout.json","mode":"move"}'
curl -sS -X POST "$API/projects/checkout.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"suiteId":"portable.checkout.json"}'
```

`TC-PROJECT-1` is moved onto `checkout.json`, the parent that already holds it:
the route accepts it and the folder is untouched. The two suite calls then place
`portable.checkout.json` — a `move` into `payments.json`, a `copy` back — so the
suite ends with one home in each project while every case identifier stays
unique. This is the only suite composition in the dataset, and the only
placement aimed at a suite.

[`mode`](#conventions-used-in-this-document) is spelled out only when a test
needs `move`, which relocates the entity instead of duplicating it. Note the two
constraints the placement routes carry, both of which the generator must respect:

- A placement addresses the entity by its bare identifier, so it must resolve to
  **exactly one** home. Copying `TC-LOGIN-1` a second time — into
  `checkout.json` after the copy above already gave it a second home — answers
  `409` (`This identifier is used by 2 parents …`), not `201`. Placement is
  therefore a one-way operation from the entity's original home; the generator
  records where each entity lives and never re-addresses an ambiguous identifier.
- Placing **onto** an identifier the target parent already holds also answers
  `409` (`The target parent already holds a child with this identifier`), because
  the case folder is named by the `testCaseId` verbatim and a second copy would
  collide with the first. Running the seed twice onto one volume fails here by
  design; teardown runs first.

### Step 12 — validation of the seeded environment

The generator's validation step asserts, at minimum:

- `GET /health` answers `{"status":"ok","storage":"filesystem"}`.
- `GET /ready` answers `{"status":"ready","storage":"filesystem"}`, and
  `GET /diagnostics` reports the same store as `"ready": true` without naming
  its path.
- `GET /metrics` answers the Prometheus text exposition, and the series for a
  request the validation just served appears in it.
- Every document in the target tree above is present and readable back through a
  `GET` route that resolves it — its document route, or the listing of the
  parent that holds it for the cases and the suite row 22 gave a second home.
- Every seeded case carries ordered `steps` and at least one case-level
  attachment, and its step-level attachments number exactly those recorded in
  row 6 (1, 2, 1, 1, 0, 1, 0, 1 across the eight cases); each case records the
  step write as `version` ≥ 2, and `GET /test_cases/TC-LOGIN-2/history` reports
  the revision behind it. Each case is read back through the parent that holds
  it.
- The cases row 22 composed into a second home — `TC-LOGIN-1`, `TC-ORDERS-1`,
  `TC-CATALOG-1` and `TC-SEARCH-1` — answer `409` to a bare
  `GET /test_cases/{id}`, because the identifier resolves to two parents, and
  the two-homed suite `portable.checkout.json` is refused the same way on both
  `GET /test_suites/{id}` and `GET /test_suites/{id}/test_cases`. All of them
  still read back through their parents' listings.
- Each project's own case listing holds exactly the cases the seed left there:
  `checkout.json` holds the two it owns directly, `payments.json` the three it
  ends with, and `regression.checkout.json` holds no case at all. The suite
  listings include every suite the seed created — the empty one, the duplicate
  of row 21 and the two-homed `portable.checkout.json` under both projects.
- Each of the three project-owned resources — configurations, runs and
  milestones — reads back through **both** the listing of the project that owns
  it (`GET /projects/{id}/configurations`, `GET /projects/{id}/test_runs`,
  `GET /projects/{id}/milestones`) and its global document route
  (`GET /configurations/{id}`, `GET /test_runs/{id}`, `GET /milestones/{id}`).
  The two agree on where each one lives: both runs and the milestone read back
  under `checkout.json`, `chrome-linux.json` under `checkout.json` and
  `firefox-linux.json` under `payments.json`.
- A configuration created in one project is not visible to a caller restricted
  to another. The seeded **viewer** holds `owner` on `checkout.json` and no grant
  at all on `payments.json`, so for that caller `GET /configurations` lists
  `chrome-linux.json` and not `firefox-linux.json`, and
  `GET /projects/payments.json/configurations` answers `403 forbidden`.
- `GET /milestones/v1.0.json/progress` reports five buckets
  (`Passed`, `Failed`, `Blocked`, `Untested`, `Retest`) that partition
  `totalCases`. The population is every case `nightly.json` **holds** once: the
  two cases it pins (`TC-LOGIN-1`, `TC-PROJECT-1`), the four the linked
  `smoke.checkout.json` snapshot embeds (`TC-LOGIN-1`, `TC-LOGIN-2`,
  `TC-CART-1`, `TC-MOVE-1`) and the four it records results for, deduplicated by
  case id. `TC-MOVE-1` is held through the suite snapshot and has no result, so
  it is `Untested`; the other four carry the statuses step 7 recorded. The
  report is therefore `totalCases` 5 with `Passed` 1, `Failed` 1, `Blocked` 1,
  `Retest` 1 and `Untested` 1 — see *Milestone progress: the buckets partition
  `totalCases`* in
  [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md).
- `GET /reports/coverage` and `GET /reports/summary` answer for both scopes.
- `GET /test_runs?tags=nightly` and `GET /test_runs?configuration=chrome-linux.json`
  both return the seeded runs.
- `GET /auth/me` reports the seeded **viewer's** role (`owner`) on
  `checkout.json`, which is the project it was granted, and no grant on
  `payments.json`; the seeded **editor's** role (`editor`) on the same project
  and no grant on `payments.json` either; and for the bootstrap account
  `systemAdmin: true` with **no** grants at all — a system administrator needs
  none (see [§5](#5-known-gap-auth-accounts-and-role-grants)), which the
  authorized write below proves.
- The seeded **editor** exercises the middle of the ladder, which is the whole
  point of the account. `POST /projects/checkout.json/test_suites` with its
  token is accepted — the content inside a project needs `editor` — while
  `PUT /projects/checkout.json` with the *same* token answers `403 forbidden`,
  because the project document itself needs `owner`. The first call is the
  proof the token carries a real grant; the second is the proof the refusal is
  the role and not a missing grant, since an account with no grant at all would
  answer both the same way. The probe suite the first call creates is deleted
  again in the same step, so the seeded listing is left as it was found.
- A guarded write with a token that lacks the role answers `403 forbidden` —
  `POST /projects` is refused for the `viewer`, and no seeded session is
  accepted for a write above its rung. Creating a project is not a project
  resource: nothing exists yet for a grant to be scoped to, so this route is the
  system-administrator check rather than a grant lookup, and the refusal is the
  missing administrator flag, not the absent `payments.json` grant.

This step needs a seeded stack, so it runs on the documented one-command path
(`scripts/demo.sh`), not in CI. The static route-coverage check in
[§1](#route-coverage) is the half that runs on every pull request.

## 4. Teardown scope

Teardown is the inverse of the seed and nothing more. It removes only the
entities the generator itself created, and it is safe to run against a
deployment that holds other data.

### Removed

| Order | What | Calls |
| --- | --- | --- |
| 1 | Milestones the seed created | `DELETE /milestones/{id}` |
| 2 | Test runs the seed created | `DELETE /test_runs/{id}` |
| 3 | Suites the seed created, and their copies | `DELETE /projects/{id}/test_suites/{suite_id}` |
| 4 | Cases the seed created, including the placed copies | `DELETE /projects/{id}/test_cases/{case_id}` and `DELETE /test_suites/{id}/test_cases/{case_id}` |
| 5 | Configurations the seed created, read back from the project that owns each | `DELETE /projects/{id}/configurations/{config_id}` |
| 6 | Projects the seed created (with whatever is left below them) | `DELETE /projects/{id}` |
| 7 | Auth accounts and grants the seed wrote | through the same `AuthStore` path the seed used — [§5](#5-known-gap-auth-accounts-and-role-grants) |

The order matters: a run and a milestone hold references to suites, cases and
projects, and a deletion in dependency order avoids `409` conflicts and
half-removed trees. A configuration is removed before the project that owns it:
deleting a project cascades to the configurations inside it, so a configuration
taken after its project would already be gone and the teardown would report a
deletion it cannot resolve. Deleting a project removes everything below it, so
step 6 also cleans up anything steps 3 to 5 missed.

### Never touched

- **Anything the seed did not create.** Removal is by the exact identifiers the
  seed recorded when it created them, never by pattern, prefix or "clear the
  collection".
- **`configurations/` beyond the seed's own entries.** A configuration belongs to
  one project, so it is reachable only through the project that owns it: deleting
  the wrong project's copy of an identifier, or emptying a project's whole
  `configurations/` folder, would break that project's unrelated runs. (Note that
  the existing `scripts/clear-data.mjs` does not remove configurations at all —
  deleting a project cascades to them; the generator must delete exactly
  `chrome-linux.json` from `checkout.json` and `firefox-linux.json` from
  `payments.json`, and nothing else.)
- **The bootstrap account.** `admin` is created by the deployment at startup
  from `TUCANO_BOOTSTRAP_USERNAME`/`TUCANO_BOOTSTRAP_PASSWORD`, not by the seed,
  and removing it would lock out the next run. Teardown removes the seed's own
  non-admin accounts and its grants, and leaves the bootstrap account alone.
- **Anything outside `TUCANO_DATA_DIR`.** No fixture, no container volume, no
  file the generator did not write.
- **The data volume itself.** Teardown empties the seed, it does not drop the
  volume or truncate a file it did not create.

A teardown that cannot resolve whether it created an entity must leave it in
place and report it, rather than remove it. A missed deletion is recoverable; a
deleted project is not.

This is implemented by `scripts/teardown.mjs` (issue #194). It signs in as the
bootstrap account exactly as the seed does, walks the table above in order, and
scopes every removal to the identifiers the seed fixed:

- **Each removal is guarded.** A project, suite, run or milestone is read back
  from its collection route, a configuration from the listing of the project that
  owns it, and a case from its recorded parent's listing, before its `DELETE` is
  issued; anything that is not there is skipped, and anything that is there but
  is not one of the seed's is reported as kept.
- **Suites are enumerated, not assumed.** The duplicate step's identifier is
  derived by the API, so step 3 lists a project's suites and removes only the
  seed's named suite or an id carrying the copy prefix; any other suite is named
  as kept. The suite the composition placed in a second project is named against
  both projects and removed from each, because a suite removal is scoped to one
  project and leaving the second copy would strand its folder. Projects are
  listed for the same reason, and each project's
  configurations are read from that project's own listing: a foreign project or
  configuration is named as kept rather than silently walked past.
- **It reports and exits non-zero.** Every item left in place is printed with
  the reason, and the run exits `1`; a run that removed or confirmed the absence
  of everything exits `0`. A missing entity is a settled teardown, not a failure,
  so re-running over an already-clean volume is successful.
- **It never guesses its target.** `clear-data.mjs` falls back to the first
  candidate URL; teardown fails outright when no candidate answers `/health`,
  because a script that deletes must be sure where it is deleting from.

Step 7 runs the server binary's `unseed-auth` subcommand (the inverse of the
`seed-auth` of [§5](#5-known-gap-auth-accounts-and-role-grants)) over the volume
the server reads, because the API publishes no route for accounts or grants. It
removes each named account and the grants that account holds on the named
projects, refuses the bootstrap account, and requires at least one `--grant`
so it can never clear every grant an account holds. It ends with a summary line
— `unseed-auth: account=removed|absent|kept grants_removed=<n> grants_kept=<n>`
— which the teardown parses, so an account that was already gone reads as a
settled teardown rather than as a refusal. `grants_kept` counts only grants that
are really on the volume: a named project the account holds no grant on is
reported in the prose above the summary (`no grant on <project> to remove`) and
counted as neither removed nor kept, so a second run over an already-cleared
volume does not report a grant that was never there as one left behind. A grant
the account does hold on a project this call was *not* pointed at is counted as
kept, because removing the account does not remove it: the grant file keeps it
keyed on the identifier, where every lookup that goes through `auth/users.json`
stops seeing it.

The name it removes is the teardown's, not the seed's: the seed always writes
`viewer` and `editor`, while `TUCANO_SEED_VIEWER_USERNAME` and
`TUCANO_SEED_EDITOR_USERNAME` say which account that run should address, for a
volume whose accounts were created under other names by hand. An `absent`
outcome is therefore a settled teardown only while the seed's own name holds
nothing. The subcommand's read-only `--check` mode reports where an account and
the named grants stand, writes no state, and ends with its own summary line —
`unseed-auth: check account=… system_admin=… grants_present=… grants_absent=… orphans=…`
— whose `orphans` count is the grants still recorded, anywhere in the store,
against an account identifier `auth/users.json` does not hold. Opening the store
at all creates `auth/`, `auth/projects/` and an empty `.tucano.lock`, even on a
volume that holds nothing, so a check on a pristine data directory leaves those
three behind and nothing else. The teardown runs that check once per run, after
every account has been dealt with, whichever name it was configured with and
whichever branch each account took: an orphan it finds is reported as kept and
the run exits `1`, because a grant keyed on an account that is gone cannot be
found by looking a name up. When the configured name is not the seed's, the same
answer also says whether the seed's own account is still there: an account still
found under it — except a system administrator, which the seed never writes — is
reported as kept too, so a name that went astray cannot pass as a clean sweep
while the seeded account and its grant survive.

## 5. Known gap: auth accounts and role grants

Rows 25–27 of the matrix cover auth accounts and role grants, but the HTTP
surface does not expose a route that creates an account or sets a grant. The
`Auth` area publishes exactly four operations — `POST /auth/login`,
`POST /auth/refresh`, `POST /auth/logout`, `GET /auth/me` — and creating a
project deliberately starts it with no grants, because the caller who could
grant a role is the one who already holds `owner`.

The seed therefore has one unavoidable exception to "the generator drives the
API":

- **Accounts and grants are written through the auth store's own path** —
  `auth/users.json` and `auth/projects/<project>.json` — by the generator, using
  the same store the API uses rather than a hand-authored JSON blob, so the
  password hashes and refresh-token list stay in the format the server reads
  back. The generator never writes these files through the HTTP surface because
  it cannot.
- **The bootstrap account is not written by the generator.** It is created by the
  deployment at startup from `TUCANO_BOOTSTRAP_USERNAME`/`TUCANO_BOOTSTRAP_PASSWORD`,
  and the seed signs in as it to obtain the token every other call needs. Row 25
  lists it as a seeded example because the dataset is not complete without it;
  its producing call is that startup path, not a call the generator makes.
- **The bootstrap account holds no role grant, by design.** A system
  administrator is authorized without one: `authorize` and `require_milestone`
  short-circuit on the token's `systemAdmin` claim before they consult the grant
  store, which the unit test
  `a_system_administrator_is_authorized_without_any_grant` pins. So seeding a
  grant for `admin` would add an on-disk artifact the server never reads, and
  the grants the generator does write exist to give the *non-admin* accounts
  reach. Row 27 therefore seeds one grantee per rung the dataset has to
  exercise: the `viewer`, holding `owner` on `checkout.json`, and the `editor`,
  holding `editor` on the same project. One grantee would not do. With only a
  `viewer` holding `owner`, the dataset has no session whose role is high enough
  for a content write inside the project but too low for a project write, so a
  client cannot tell "the role is too low for this operation" from "the account
  holds no grant here at all" — the same caller gets the same `403` for both,
  and a client that tests one against the other learns nothing. The second rung
  is what makes the two answers distinguishable, and
  [§3 step 12](#step-12--validation-of-the-seeded-environment) asserts exactly
  that pair. `GET /auth/me` intentionally reports `"roles": {}` for the admin:
  `me` reports the account's grants, not its effective authority, so an admin
  with no grant legitimately reports none.
- **A configuration needs a project role, which is why the grant is not
  optional.** A configuration is a project resource, so reading one needs
  `Viewer` in the project that holds it and creating one needs `Editor`;
  `GET /configurations` answers with the configurations of the projects the
  caller reaches, and a project the caller holds no grant in answers
  `403 forbidden` to its own configuration listing. The seed's `viewer` holds
  `owner` on `checkout.json`, which subsumes `editor`, and that grant is what
  lets it see `chrome-linux.json` and not `firefox-linux.json` — the isolation
  [§3 step 12](#step-12--validation-of-the-seeded-environment) asserts. The
  editor's `editor` grant is lower on the ladder but still subsumes `Viewer`, so
  it reaches the same configurations through the middle rung. The bootstrap
  account is the only caller that needs no grant for any of this, by the
  short-circuit above.
- **`GET /auth/me` is the assertion that closes the loop.** After the seed, the
  validation step reads each seeded account's `roles` map and `systemAdmin` flag
  through the API, which proves the files the generator wrote are the ones the
  server actually honours.

This gap is recorded here rather than papered over because it is a genuine
asymmetry between "every action is available in the GUI and the API" and the
present `Auth` surface. Closing it is an API change with its own compatibility
rules (a new route, a new request schema, and the `403`/`409` answers it would
need), and is tracked by the epic rather than smuggled into this specification.
Until it is closed, the exception above is the documented, checkable behaviour.

## 6. Keeping this specification current

This document is the source of truth for the dataset, so three habits keep it
honest:

1. **A new feature adds a matrix row.** If a resource, a route or a stored field
   is added to `openapi.json`, the coverage matrix gains a row with a seeded
   example and producing call, or an explicit note saying why no example is
   needed. The route-coverage check in [§1](#route-coverage) fails the pull
   request until either is there, and the same check fails when a row names a
   route the contract no longer publishes.
2. **A new row adds an assertion.** The validation step in [§3 step 12](#step-12--validation-of-the-seeded-environment)
   covers the new row, so a stale matrix fails a check rather than being
   discovered by a reader.
3. **A new stored file is named here.** The target tree in [§2](#2-target-tree-below-tucano_data_dir)
   names every file the seed writes. A file the seed writes but this document
   does not name is a bug in one of the two.

Both the seed order and the teardown order are dependency orders, not
preferences: the tracking issues for the generator (#193), the teardown (#194),
and the Compose wiring plus freshness check (#195) implement the sequences
above.

## 7. Implementation status

`scripts/seed.mjs` implements [§3 steps 0–11](#3-generating-api-calls). It is
deliberately **not idempotent**: the identifiers in [§2](#2-target-tree-below-tucano_data_dir)
are fixed, and placing a case onto an identifier its target parent already holds
is a `409`, so a second run onto the same volume cannot complete. Instead the
script refuses up front when its own identifiers are already present and points
at `scripts/teardown.mjs`, which is the scoped "clear first" and leaves anything
on the volume it did not create.

The one exception is [§5](#5-known-gap-auth-accounts-and-role-grants)'s
accounts and grants: the API publishes no route for either, so the server binary
grows a `seed-auth` subcommand (`src/auth/seed.rs`) that writes them through the
same `AuthStore` the running server reads. Unlike the dataset itself it *is*
idempotent — an existing account keeps its password and only missing grants are
added. The script invokes it through `TUCANO_SEED_AUTH_CMD`, once per account
the dataset needs — the `viewer` at `owner` and the `editor` at `editor` — skips
the step with a notice when that is unset, and then performs the `GET /auth/me`
assertion for each, which closes the loop.

Teardown ([§4](#4-teardown-scope)) is implemented by `scripts/teardown.mjs`
(#194). Its auth half is the `unseed-auth` subcommand, the inverse of
`seed-auth`: it removes each named account and the grants that account holds on
the named projects, refuses the bootstrap account, and reports anything it could
not resolve instead of guessing. The subcommand ends with a summary line
(`unseed-auth: account=removed|absent|kept grants_removed=<n> grants_kept=<n>`)
that the JS half parses, and counts as kept only the grants that really are on
the volume — a named project the account holds nothing on is prose, not a kept
grant — which is what makes a second teardown over an already-clean volume exit
`0` rather than reporting a false refusal. The JS half also runs the
subcommand's read-only `--check` mode once per teardown, after every account has
been dealt with, and reads its `orphans` count for the grants left keyed on an
account the store no longer holds, whichever name the run was configured with
and whichever branch each account took ([§4](#4-teardown-scope)).

The freshness checks ([§1](#stale-matrix-check)) are implemented by
`scripts/check-matrix.mjs` (#195), and the two halves of the decision above are
deliberately split by cost. The static half is a dependency-free Node script
over `openapi.json` and this document; CI runs it as its own job
(`contract` in [`.github/workflows/lint.yml`](../.github/workflows/lint.yml))
that reads two files and starts nothing. The seeded half is the generator's
validation step, and it runs on the one-command path `scripts/demo.sh`, which
brings a stack up, seeds it, runs `scripts/smoke.sh`, then runs the validation
step (`scripts/validate-seed.mjs`) against the seeded data.

Starting a service on every pull request was considered and rejected: a
seed-and-validate job would add the container build, the service startup and the
whole seed sequence to every PR's CI cost and machine load. The static check
catches the drift a feature addition causes — a route that reaches no row — at
the cost of reading two files, and the seeded check catches a broken producing
call on the local path where a developer is already running a stack.

## 8. Validation status

| Piece | Issue | State |
| --- | --- | --- |
| Generator ([§3](#3-generating-api-calls) steps 0–11) | #193 | merged |
| Teardown ([§4](#4-teardown-scope)) | #194 | merged |
| Compose wiring, route-coverage check, one-command path | #195 | see [`scripts/demo.sh`](../scripts/demo.sh) |
| Editor grant and mid-ladder verification ([§1](#1-feature-coverage-matrix) rows 26–28, [§5](#5-known-gap-auth-accounts-and-role-grants)) | #281 | merged |
