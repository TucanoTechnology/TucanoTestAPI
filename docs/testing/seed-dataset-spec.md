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
| 1 | Projects | `checkout.json` (two suites, tags, a directly owned case) and `payments.json` (a suite only) | `POST /projects` ×2 | `projects/checkout.json/project.json`, `projects/payments.json/project.json` |
| 2 | Suites inside a project | `smoke.checkout.json` in `checkout.json`; `smoke.payments.json` in `payments.json` | `POST /projects/{id}/test_suites` | `projects/<p>/<suite>/suite.json` |
| 3 | Cases directly in a project | `TC-PROJECT-1` in `checkout.json` | `POST /projects/{id}/test_cases` | `projects/checkout.json/TC-PROJECT-1/test-case.json` |
| 4 | Cases inside a suite | `TC-LOGIN-1`, `TC-LOGIN-2`, `TC-CART-1` | `POST /test_suites/{id}/test_cases` | `projects/<p>/<suite>/<case>/test-case.json` |
| 5 | Steps on a case | `TC-LOGIN-2` carries ordered `steps` | `PUT /test_cases/TC-LOGIN-2` | inside `test-case.json` |
| 6 | Case attachments | `login-flow.txt` on `TC-LOGIN-1` | `POST /test_cases/{id}/attachments` (multipart) | `…/TC-LOGIN-1/login-flow.txt` |
| 7 | Step attachments | `step-1.txt` on step index 0 of `TC-LOGIN-2` | `POST /test_cases/{id}/steps/{index}/attachments` | `…/TC-LOGIN-2/steps/0/step-1.txt` |
| 8 | Tags on projects, suites, cases and runs | `checkout`/`regression` on the project; `smoke` on the suite; `auth` on a case; `nightly` on the run | the create/update calls that carry `tags`, then the seeded run read back through the tags filter (`tags=nightly`) | the `tags` array in each stored document |
| 9 | Case versioning and revision history | `TC-LOGIN-2` updated once after creation | `PUT /test_cases/TC-LOGIN-2`, then `GET /test_cases/TC-LOGIN-2/history` | `…/TC-LOGIN-2/revisions/v1.json`; `version`/`lastModified` in `test-case.json` |
| 10 | Configurations (one per project) | `chrome-linux.json` in `checkout.json`; `firefox-linux.json` in `payments.json` | `POST /projects/{id}/configurations` ×2 | `projects/checkout.json/configurations/chrome-linux.json`, `projects/payments.json/configurations/firefox-linux.json` |
| 11 | Linking a configuration to a run | `chrome-linux.json` linked to `nightly.json` | `POST /test_runs/{id}/configurations` | the `configurations` reference array in `projects/checkout.json/test_runs/nightly.json` |
| 12 | Runs (point-in-time snapshots) | `nightly.json` covering the `checkout.json` project and its smoke suite | `POST /projects/{id}/test_runs`, then `POST /test_runs/{id}/test_suites` | `projects/checkout.json/test_runs/nightly.json` |
| 13 | Run case membership pinned from a template | run carries `TC-LOGIN-1`, `TC-LOGIN-2` as copies | `POST /test_runs/{id}/test_cases` | `test_cases` array in `projects/checkout.json/test_runs/nightly.json` |
| 14 | Recorded results — every status | `TC-LOGIN-1` `Passed`, `TC-LOGIN-2` `Failed` (with notes and `durationMs`), `TC-PROJECT-1` `Blocked`, `TC-CART-1` `Retest`; `Untested` left implicit for the one declared case with no recorded result | `POST /test_runs/{id}/results` per case (the route replaces an earlier result for the same case) | `results` array in `projects/checkout.json/test_runs/nightly.json` |
| 15 | Result replacement (upsert) | `TC-LOGIN-2` recorded `Blocked` then replaced with `Failed` | same `POST /test_runs/{id}/results` twice | one `Failed` entry for `TC-LOGIN-2` |
| 16 | Defect links — all four trackers | one link per tracker type on the failed result of `TC-LOGIN-2` | `POST /test_runs/{id}/results/{case_id}/defects` ×4 | `defectLinks` array inside the `TC-LOGIN-2` result |
| 17 | Defect link removal | the GitHub link of row 16, linked then unlinked | `POST …/defects` then `DELETE …/defects/{link_id}` | the removed link is absent from `defectLinks` |
| 18 | JUnit XML import | `nightly-import.json` run, importing a fixture for two cases | `POST /test_runs/{id}/import/junit` | `results` array in `projects/checkout.json/test_runs/nightly-import.json` |
| 19 | JSON result import | `nightly-import.json`, importing `Passed` and `Failed` entries | `POST /test_runs/{id}/import/json` | `results` array in `projects/checkout.json/test_runs/nightly-import.json` |
| 20 | Milestones and derived progress | `v1.0.json` referencing `nightly.json` | `POST /projects/{id}/milestones`, then `GET /milestones/v1.0.json/progress` | `projects/checkout.json/milestones/v1.0.json` |
| 21 | Duplication | a suite duplicate kept under a project | `POST /test_suites/smoke.checkout.json/duplicate` | `projects/checkout.json/<copy>/suite.json` |
| 22 | Copy vs. move composition | `TC-LOGIN-1` copied into `payments.json` while the source stays in `smoke.checkout.json`; `TC-PROJECT-1` placed with `"mode":"move"` onto the project that already owns it, `checkout.json`, which leaves it there — the same route as the copy, with `mode` selecting move | `POST /projects/{id}/test_cases` with `testCaseId` (copy is the default; `mode` selects move) | `projects/payments.json/TC-LOGIN-1/` beside the source in `projects/checkout.json/smoke.checkout.json/TC-LOGIN-1/`; `projects/checkout.json/TC-PROJECT-1/` stays where [§2](#2-target-tree-below-tucano_data_dir) shows it |
| 23 | Coverage report | `GET /reports/coverage`, global and `?projectId=checkout.json` | the report routes | n/a (read-only — no new files) |
| 24 | Summary report | `GET /reports/summary` and `?configurationId=chrome-linux.json` | the report routes | n/a (read-only — no new files) |
| 25 | Auth users: system administrator | `admin` — the bootstrap account | `TUCANO_BOOTSTRAP_USERNAME`/`TUCANO_BOOTSTRAP_PASSWORD` at startup; `POST /auth/login` | `auth/users.json` (**not** produced by an API call — see [§5](#5-known-gap-auth-accounts-and-role-grants)) |
| 26 | Auth users: non-admin account | `viewer` — a stored account with no `systemAdmin` flag | written through the `AuthStore` path by the generator; `POST /auth/login` | `auth/users.json` (**not** produced by an API call) |
| 27 | Role grants per project | `viewer` holds `owner` on `checkout.json` and **no grant at all** on `payments.json`. `admin` holds **no grant at all** — a system administrator is authorized without one (see [§5](#5-known-gap-auth-accounts-and-role-grants)) | written through the `AuthStore` grant path; verified by `GET /auth/me` as `viewer`, and for `admin` by an authorized write and by `systemAdmin: true` | `auth/projects/checkout.json` (and no `auth/projects/payments.json`) |
| 28 | Authorization enforcement | the `viewer`-scoped token proves reads succeed and a write is refused with `forbidden` | any guarded write with the scoped token | n/a (the refusal is the evidence) |
| 29 | Sessions | sign in, refresh (rotating the refresh token once), sign out, `GET /auth/me` | `POST /auth/login`, `POST /auth/refresh`, `POST /auth/logout`, `GET /auth/me` | n/a (`auth/users.json` carries the revocable refresh tokens) |
| 30 | Service surface | `GET /health`, `GET /ready`, `GET /diagnostics`, `GET /openapi.json` | the service routes | n/a (read-only) |

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
| `GET /projects/{id}/test_cases` | list companion of the case routes; [§3 step 12](#step-12--validation-of-the-seeded-environment) reads each parent's cases back through it |
| `GET /projects/{id}/test_suites` | list companion of `POST /projects/{id}/test_suites` (row 2); the seed reads this listing to learn the duplicate's identifier |
| `DELETE /projects/{id}/test_suites/{suite_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `GET /test_cases/{id}` | read companion of the case routes, but only for a case with one home. Row 22 places `TC-LOGIN-1` into `payments.json` while its source stays in `smoke.checkout.json`, so a bare `GET /test_cases/TC-LOGIN-1` is answered `409` by design and [§3 step 12](#step-12--validation-of-the-seeded-environment) reads that case back through its parents' listings instead |
| `PUT /test_cases/{id}` | row 5 names the concrete `PUT /test_cases/TC-LOGIN-2` |
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
| `GET /test_suites/{id}` | read companion of the suite routes; [§3 step 12](#step-12--validation-of-the-seeded-environment) reads the seeded suites back |
| `PUT /test_suites/{id}` | replace companion of row 2 |
| `DELETE /test_suites/{id}` | teardown-scope call, by parent ([§4](#4-teardown-scope)) |
| `GET /test_suites/{id}/test_cases` | list companion of `POST /test_suites/{id}/test_cases` (row 4) |
| `DELETE /test_suites/{id}/test_cases/{case_id}` | teardown-scope call ([§4](#4-teardown-scope)) |
| `GET /api-docs` | the Swagger UI page, not part of the data model |
| `GET /openapi.json` | the contract itself; row 30 covers it as a service-surface assertion |

Teardown-scope operations count as accounted for because [§4](#4-teardown-scope)
is the document that specifies them, and the check verifies they reach it.

## 2. Target tree below `TUCANO_DATA_DIR`

The tree below is what a successful seed run leaves behind, with the fixed
identifiers this document specifies. `<epoch>` is the run's stamp and `<rand>`
is the test-case identifier the API's duplicate route derives; both are read
back from the API's own response rather than assumed.

```
$TUCANO_DATA_DIR/
├── auth/
│   ├── users.json                            # accounts, password hashes, refresh tokens
│   └── projects/
│       └── checkout.json                     # {"grants": {"<viewer id>": "owner"}}
├── .tucano.lock                              # advisory lock, created by the API
├── projects/
│   ├── checkout.json/
│   │   ├── project.json                      # {"projectId","name","tags":["checkout","regression"]}
│   │   ├── smoke.checkout.json/              # a suite folder
│   │   │   ├── suite.json
│   │   │   ├── TC-LOGIN-1/
│   │   │   │   ├── test-case.json
│   │   │   │   └── 1789393055091247267-login-flow.txt   # case attachment
│   │   │   ├── TC-LOGIN-2/
│   │   │   │   ├── test-case.json            # carries ordered steps
│   │   │   │   ├── revisions/
│   │   │   │   │   └── v1.json               # snapshot written by the update
│   │   │   │   └── steps/
│   │   │   │       └── 0/
│   │   │   │           └── 1789393055103254926-step-1.txt  # step attachment
│   │   │   ├── TC-CART-1/
│   │   │   │   └── test-case.json
│   │   ├── smoke.checkout-copy-<suffix>/     # the duplicate suite, id from the response
│   │   │   └── suite.json
│   │   ├── TC-PROJECT-1/                     # a case owned directly by the project;
│   │   │   └── test-case.json                # the move in §3, step 11 leaves it here
│   ├── test_runs/                            # reserved child of this project
│   │   ├── nightly.json                      # projects, suites, cases, results, defects, config link
│   │   └── nightly-import.json               # results arrived by import
│   ├── milestones/                           # reserved child of this project
│   │   └── v1.0.json                         # references nightly.json
│   └── configurations/                       # reserved child of this project
│       └── chrome-linux.json
└── payments.json/
    ├── project.json
    ├── smoke.payments.json/
    │   └── suite.json
    ├── TC-LOGIN-1/                           # the copy placed into this project
    │   ├── test-case.json
    │   └── 1789393055091247267-login-flow.txt   # the case's attachment travels with it
    └── configurations/                       # reserved child of this project
        └── firefox-linux.json
```

Two details of this tree are easy to get wrong and are called out deliberately:

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

## 3. Generating API calls

Every step is an HTTP call with the token from step 0. Steps 1–4 must run in
order — step 2 puts a configuration into a project step 1 creates, and steps 3
and 4 create their suites and cases inside those projects; the later steps
depend only on the resources named in them.

### Step 0 — session

Auth is optional at runtime and off by default. The seed needs it on, because
the whole sequence below carries a token and step 1 creates a project, which
only a system administrator may do. The deployment must therefore be started
with the four settings this step depends on:

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

```sh
curl -sS -X POST "$API/projects/checkout.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"smoke.checkout"}'
curl -sS -X POST "$API/projects/payments.json/test_suites" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"smoke.payments"}'
```

`POST /test_suites` and `POST /test_cases` are retired and answer 400; a suite or
a case is always created through the parent it lives in.

### Step 4 — cases, a directly owned case, and a case with steps

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

# Directly inside a project: the same route shape, a different parent.
curl -sS -X POST "$API/projects/checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-PROJECT-1","title":"Reach the checkout page","expectedResult":"Checkout page renders"}'

# Steps are the ordered `steps` array on the case, written by an update.
curl -sS -X PUT "$API/test_cases/TC-LOGIN-2" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"steps":[{"action":"Open the sign-in form","expectedResult":"The form is shown"},{"action":"Submit a locked account","expectedResult":"A lock message is shown"}]}'
```

The `PUT` on `TC-LOGIN-2` is a qualifying update: it stamps `version` and
`lastModified` and writes the first snapshot to `revisions/v1.json`, which is
what row 9 of the matrix checks.

### Step 5 — attachments

```sh
curl -sS -X POST "$API/test_cases/TC-LOGIN-1/attachments" -H "Authorization: Bearer $TOKEN" \
  -F 'file=@fixtures/login-flow.txt;type=text/plain'
curl -sS -X POST "$API/test_cases/TC-LOGIN-2/steps/0/attachments" -H "Authorization: Bearer $TOKEN" \
  -F 'file=@fixtures/step-1.txt;type=text/plain'
```

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

### Step 7 — results, including the replacement and every status

`POST /test_runs/{id}/results` is the only route that writes a result. A second
call for the same case in the same run replaces the first, which is how the
replacement in row 15 is seeded.

```sh
# Passed
curl -sS -X POST "$API/test_runs/nightly.json/results" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"testCaseId":"TC-LOGIN-1","status":"Passed","notes":"signed in","durationMs":1200}'
# Blocked first, then replaced with Failed — the same case, one stored result.
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

```sh
curl -sS -X POST "$API/projects/payments.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-LOGIN-1"}'
curl -sS -X POST "$API/projects/checkout.json/test_cases" -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' -d '{"testCaseId":"TC-PROJECT-1","mode":"move"}'
```

The first call copies the existing case into the named project and leaves the
source in place, which is why `TC-LOGIN-1` appears in exactly two places in the
target tree — inside `projects/checkout.json/smoke.checkout.json/` where it was
created, and inside `projects/payments.json/`.

[`mode`](#conventions-used-in-this-document) is spelled out only when a test
needs `move`, which relocates the entity instead of duplicating it. Note the two
constraints the placement routes carry, both of which the generator must respect:

- A placement addresses the entity by its bare identifier, so it must resolve to
  **exactly one** home. Copying `TC-LOGIN-1` a second time — into
  `checkout.json` after the copy above already gave it a second home — answers
  `409` (`This identifier is used by 2 parents …`), not `201`. Placement is
  therefore a one-way operation from the case's original home; the generator
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
- Every document in the target tree above is present and readable back through
  its `GET` route, with the identifiers the tree names.
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
  (`Passed`, `Failed`, `Blocked`, `Untested`, `Retest`) that count the results
  `nightly.json` records, while `totalCases` counts the cases that run
  **declares**. The two come from different places and need not agree: the
  seeded run declares two cases and records four results, so the buckets sum to
  4 while `totalCases` is 2. This is the documented, permissive legacy
  arithmetic — see *Milestone progress: `totalCases` and the buckets need not
  agree* in
  [`docs/contracts/api-compatibility.md`](../contracts/api-compatibility.md).
- `GET /reports/coverage` and `GET /reports/summary` answer for both scopes.
- `GET /test_runs?tags=nightly` and `GET /test_runs?configuration=chrome-linux.json`
  both return the seeded runs.
- `GET /auth/me` reports the seeded **viewer's** role (`owner`) on
  `checkout.json`, which is the project it was granted, and no grant on
  `payments.json`; and for the bootstrap account reports `systemAdmin: true` with
  **no** grants at all — a system administrator needs none (see
  [§5](#5-known-gap-auth-accounts-and-role-grants)), which the authorized write
  below proves.
- A guarded write with a token that lacks the role answers `403 forbidden`.

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
  as kept. Projects are listed for the same reason, and each project's
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
removes one named account and the grants that account holds on the named
projects, refuses the bootstrap account, and requires at least one `--grant`
so it can never clear every grant an account holds. It ends with a summary line
— `unseed-auth: account=removed|absent|kept grants_removed=<n> grants_kept=<n>`
— which the teardown parses, so an account that was already gone reads as a
settled teardown rather than as a refusal.

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
  the grants the generator does write exist to give the *non-admin* account
  reach. Row 27 therefore seeds exactly one grantee — the `viewer`, holding
  `owner` on `checkout.json` — and `GET /auth/me` intentionally reports
  `"roles": {}` for the admin: `me` reports the account's grants, not its
  effective authority, so an admin with no grant legitimately reports none.
- **A configuration needs a project role, which is why the grant is not
  optional.** A configuration is a project resource, so reading one needs
  `Viewer` in the project that holds it and creating one needs `Editor`;
  `GET /configurations` answers with the configurations of the projects the
  caller reaches, and a project the caller holds no grant in answers
  `403 forbidden` to its own configuration listing. The seed's `viewer` holds
  `owner` on `checkout.json`, which subsumes `editor`, and that single grant is
  what lets it see `chrome-linux.json` and not `firefox-linux.json` — the
  isolation [§3 step 12](#step-12--validation-of-the-seeded-environment)
  asserts. The bootstrap account is the only caller that needs no grant for any
  of this, by the short-circuit above.
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
added. The script invokes it through `TUCANO_SEED_AUTH_CMD`, skips the step with
a notice when that is unset, and then performs the `GET /auth/me` assertion that
closes the loop.

Teardown ([§4](#4-teardown-scope)) is implemented by `scripts/teardown.mjs`
(#194). Its auth half is the `unseed-auth` subcommand, the inverse of
`seed-auth`: it removes one named account and the grants that account holds on
the named projects, refuses the bootstrap account, and reports anything it could
not resolve instead of guessing. The subcommand ends with a summary line
(`unseed-auth: account=removed|absent|kept grants_removed=<n> grants_kept=<n>`)
that the JS half parses, which is what makes a second teardown over an
already-clean volume exit `0` rather than reporting a false refusal.

The freshness checks ([§1](#stale-matrix-check)) are implemented by
`scripts/check-matrix.mjs` (#195), and the two halves of the decision above are
deliberately split by cost. The static half is a dependency-free Node script
over `openapi.json` and this document; CI runs it as its own job
(`matrix-integrity` in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml))
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
