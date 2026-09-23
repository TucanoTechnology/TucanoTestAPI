# Security Audit S1 — HTTP surface (Issue #176)

*Every route in the served contract: authentication and authorization (authn/authz), object-level authorization and IDOR, identifier validation and path traversal, request-body and upload abuse, error-envelope and header disclosure, session and token lifecycle.*

This is the S1 checkpoint of the security audit epic [#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166), tracked by its own ticket [#176](https://github.com/TucanoTechnology/TucanoTestAPI/issues/176). It is the first of four surface reports — S1 *HTTP surface*, S2 *storage and filesystem* ([#177](https://github.com/TucanoTechnology/TucanoTestAPI/issues/177)), S3 *container and deployment* ([#178](https://github.com/TucanoTechnology/TucanoTestAPI/issues/178)), S4 *dependencies and supply chain* ([#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179)). It carries findings only; nothing here is fixed.

> **Status of this file: the finished S1 report.**

- **Affected revision (the pinned target):** `096835bc108fae2428786c7900b5cf02e92a5dcf`
- **Method:** [audit-scope.md § 6](audit-scope.md#6-how-the-audit-tasks-run), steps 1–7.
- **Findings so far:** 3 — F-176-1 (High), F-176-2 (Medium), F-176-3 (Low).
- **Pass entries so far:** thirty.
- **Executed:** S1-1 (enumerate and contract-diff), S1-2 (classify routes), S1-3 (anonymous + random-token probe), S1-4 (public surface), S1-5 (token lifecycle), S1-6 (role matrix), S1-7 (cross-project references and run scope), S1-8 (path traversal), S1-9 (upload abuse), S1-10 (error envelope, headers, logs), S1-11 (bounded DoS), S1-12 (sign-in enumeration, timing, throughput), S1-13 (boundary 9), S1-14 (auth-off parity). All fourteen sub-tasks executed.
- **Throwaway stack:** torn down — see § 7. The operator's `tucano-test-api-1` / `tucano-test-gui-1` / `open-webui` containers were up before, during, and after the audit and were never touched.

---

## 1. Revision pinned

| Item | Value |
| --- | --- |
| Commit | `096835bc108fae2428786c7900b5cf02e92a5dcf` |
| `origin/main` at the time | identical (`096835b…5dcf`); the audit ran at the merge base |
| `Cargo.lock` sha256 | `6bf59cc58933203d17f6fca19b3992217a108fdcef78e51c1334559a72fe3484` |
| `openapi.json` sha256 | `5d95ee1254c0bad98a2089dfba0fe5853a84086df8ea54a7e40fd6e3c52e81dd` |
| Image id (audit build) | `sha256:34cb67183ce6fdea8548e295e0fba64e0e3358bebd9614019dbe0852c6a33078` |
| Worktree | `/home/emanuelec/.ao/data/worktrees/tucanotestapi/tucanotestapi-176` |
| Throwaway data dir | `/tmp/audit-rev/data` → container `/data` (`TUCANO_DATA_DIR=/data`) |

**Note on the image id.** The image was built from this revision and tagged `tucano-test-audit:<rev>`; the id above is the immutable digest, so the container id reported by Docker cannot drift across re-tags. The image is retained (not pruned) so the tear-down is reproducible; see § 7.

Three throwaway containers were stood up for the three authentication configurations the report needs:

- **`audit-a`** — `TUCANO_AUTH_REQUIRED` **off** (the "anonymous" arm), bound to `127.0.0.1:3210`.
- **`audit-b`** — `TUCANO_AUTH_REQUIRED` **on** with a bootstrap system-admin account, bound to `127.0.0.1:3211`.
- **`audit-c`** — `TUCANO_AUTH_REQUIRED` on plus `TUCANO_ACCESS_TOKEN_TTL=1s` (the token-lifecycle arm), bound to `127.0.0.1:3212`.

All three run with a read-only root filesystem, a `tmpfs` `/tmp`, `no-new-privileges`, and the shared bind-mounted data dir.

---

## 2. Throwaway target (step 2)

The target is a throwaway installation of the pinned revision, seeded by [`scripts/seed.mjs`](../../scripts/seed.mjs) (issue [#193](https://github.com/TucanoTechnology/TucanoTestAPI/issues/193)), which builds the fixture projects, documents, folders, and attachments **through the HTTP API only**. The one documented exception is the auth accounts and grants of spec § 5, written by the binary's `seed-auth` subcommand via `TUCANO_SEED_AUTH_CMD` (for example `target/release/tucano-test seed-auth`).

Provisioning (verbatim):

```sh
# /tmp/audit-up.sh
docker rm -f audit-a audit-b audit-c
docker run -d --name audit-a --label ao.session="$AO_SESSION_ID" \
  -p 127.0.0.1:3210:3000 -v "$AUDIT_DATA":/data --read-only --tmpfs /tmp \
  --security-opt no-new-privileges:true "$AUDIT_IMAGE"
# audit-b identical on 3211, plus:
#   -e TUCANO_AUTH_REQUIRED=true -e TUCANO_JWT_SECRET=… -e TUCANO_BOOTSTRAP_USERNAME=auditor -e TUCANO_BOOTSTRAP_PASSWORD=…
# audit-c identical on 3212, plus:
#   -e TUCANO_ACCESS_TOKEN_TTL=1s
# … then a health-wait loop, then `docker ps --filter label=ao.session=…`
```

```sh
# /tmp/audit-acl.sh — the two documented deviations from design A.4
# Rootless Docker remaps container uid 10001 -> host uid 110000, so `chown` cannot be used;
# a POSIX ACL is substituted:
setfacl -m u:110000:rwx -m d:u:110000:rwx -m g:100998:rwx -m d:g:100998:rwx -m d:mask::rwx "$DATA"
chmod 0770 "$DATA"
# And clearing must run inside the container, because the host cannot write the remapped uid:
docker run --rm --label "ao.session=$AO_SESSION_ID" -v "$DATA":/data --entrypoint sh "$IMAGE" \
  -c 'rm -rf /data/* /data/.[!.]* 2>/dev/null || true'
```

The two deviations from the design's appendix A.4 are consequences of rootless Docker's uid remap (container `10001` → host `110000`): an ACL is used in place of `chown`, and data clearing is performed from inside the container rather than from the host.

Seeded identities:

| Account | Password source | Role | Project grant |
| --- | --- | --- | --- |
| `auditor` | `TUCANO_BOOTSTRAP_PASSWORD` (bootstrap) | `systemAdmin` | all (global bypass) |
| `viewer` | `TUCANO_SEED_VIEWER_PASSWORD` | `owner` | `checkout.json` |
| `editor` | `TUCANO_SEED_EDITOR_PASSWORD` | `editor` | `checkout.json` |

No seeded account holds a grant on `payments.json`, and **no API route creates an account or sets a grant** — accounts and grants are provisioned out of band under `TUCANO_DATA_DIR/auth/`, read from `auth/projects/<project>.json`. This is a recorded observation (O-176-9), not a finding.

---

## 3. Served surface (S1-1, S1-2)

The served contract is the `openapi.json` document, which was enumerated and cross-checked against the router's own route registrations. Both copies of `openapi.json` (the served document and the repository file) are byte-equal (140,892 bytes).

**Measured: 59 paths, 85 operations.** Seven are public (no `security` requirement); 78 carry `security: bearerAuth` and are guarded. Two of the guarded routes — `POST /auth/logout` and `GET /auth/me` — restate their guard via the `Principal` axum extractor rather than a role check, so their guard is "any authenticated caller".

> **Drift from the design brief.** The design (§ 2.5) estimated `49 paths / 71 operations / 5 public`. The measured surface is `59 / 85 / 7`. This is a fact about the design brief, not a finding: the contract grew between the brief and the pinned revision. The two public routes added beyond the design's five are the two Swagger mounts (`GET /api-docs`, registered as both `/api-docs` and `/api-docs/`).

| `METHOD` | `path` | `operationId` | classification | guard | guard site |
| --- | --- | --- | --- | --- | --- |
| `GET` | `/health` | `getHealth` | public | none | `mod.rs:221 health` |
| `GET` | `/ready` | `getReady` | public | none | `mod.rs:231 ready` |
| `GET` | `/openapi.json` | `getOpenApiDocument` | public | none | `mod.rs:280 openapi` |
| `GET` | `/api-docs` | `getApiDocs` | public | none | `mod.rs:288 swagger_ui (mounted /api-docs and /api-docs/ at :205,206)` |
| `GET` | `/diagnostics` | `getDiagnostics` | public | none | `mod.rs:252 diagnostics` |
| `POST` | `/auth/login` | `login` | public | none | `auth.rs:246 login` |
| `POST` | `/auth/logout` | `logout` | guarded | Principal extractor (bearer authentication, any authenticated caller) | `auth.rs:282 logout` |
| `GET` | `/auth/me` | `getCurrentUser` | guarded | Principal extractor (bearer authentication, any authenticated caller) | `auth.rs:297 me` |
| `POST` | `/auth/refresh` | `refreshSession` | public | none | `auth.rs:263 refresh` |
| `GET` | `/projects` | `listProjects` | guarded | access::scope + access::filter_list | `crud.rs:37,39 (list_projects)` |
| `POST` | `/projects` | `createProject` | guarded | access::guard_create -> require_admin (Resource::Projects) | `crud.rs:69 + access.rs:348` |
| `DELETE` | `/projects/{id}` | `deleteProject` | guarded | access::guard_delete | `crud.rs:99` |
| `GET` | `/projects/{id}` | `getProject` | guarded | access::guard_get | `crud.rs:52` |
| `PUT` | `/projects/{id}` | `updateProject` | guarded | access::guard_update | `crud.rs:84` |
| `GET` | `/projects/{id}/configurations` | `listProjectConfigurations` | guarded | access::require(project, Role::Viewer) | `configurations.rs:41` |
| `POST` | `/projects/{id}/configurations` | `addProjectConfiguration` | guarded | access::guard_project_create(Configurations, Editor) | `configurations.rs:52` |
| `DELETE` | `/projects/{id}/configurations/{config_id}` | `removeProjectConfiguration` | guarded | access::require(project, Role::Editor) | `configurations.rs:64` |
| `POST` | `/projects/{id}/duplicate` | `duplicateProject` | guarded | access::guard_duplicate -> require_admin (Resource::Projects) | `crud.rs:116 + access.rs:483` |
| `GET` | `/projects/{id}/milestones` | `listProjectMilestones` | guarded | access::require(project, Role::Viewer) | `milestones.rs:43` |
| `POST` | `/projects/{id}/milestones` | `addProjectMilestone` | guarded | access::guard_project_create(Milestones, Owner) | `milestones.rs:54` |
| `DELETE` | `/projects/{id}/milestones/{milestone_id}` | `removeProjectMilestone` | guarded | access::require(project, Role::Owner) | `milestones.rs:66` |
| `GET` | `/projects/{id}/test_cases` | `listProjectTestCases` | guarded | access::require(project, Role::Viewer) | `cases.rs:49` |
| `POST` | `/projects/{id}/test_cases` | `addProjectTestCase` | guarded | access::guard_composition(Cases, target project, Editor) | `cases.rs:60` |
| `DELETE` | `/projects/{id}/test_cases/{case_id}` | `removeProjectTestCase` | guarded | access::require(project, Role::Editor) | `cases.rs:77` |
| `POST` | `/projects/{id}/test_cases/{case_id}/attachments` | `uploadProjectTestCaseAttachment` | guarded | case_in_project -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_project)` |
| `DELETE` | `/projects/{id}/test_cases/{case_id}/attachments/{filename}` | `deleteProjectTestCaseAttachment` | guarded | case_in_project -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_project)` |
| `GET` | `/projects/{id}/test_cases/{case_id}/attachments/{filename}` | `downloadProjectTestCaseAttachment` | guarded | case_in_project -> access::require(project, Role::Viewer) | `cases.rs:277 (case_in, via case_in_project)` |
| `GET` | `/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `listProjectTestCaseStepAttachments` | guarded | case_in_project -> access::require(project, Role::Viewer) | `cases.rs:277 (case_in, via case_in_project)` |
| `POST` | `/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `uploadProjectTestCaseStepAttachment` | guarded | case_in_project -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_project)` |
| `DELETE` | `/projects/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}` | `deleteProjectTestCaseStepAttachment` | guarded | case_in_project -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_project)` |
| `GET` | `/projects/{id}/test_runs` | `listProjectTestRuns` | guarded | access::require(project, Role::Viewer) | `runs.rs:43` |
| `POST` | `/projects/{id}/test_runs` | `addProjectTestRun` | guarded | access::guard_project_create(Runs, Editor, + every project in body.projects) | `runs.rs:54` |
| `DELETE` | `/projects/{id}/test_runs/{run_id}` | `removeProjectTestRun` | guarded | access::require(project, Role::Editor) | `runs.rs:66` |
| `GET` | `/projects/{id}/test_suites` | `listProjectTestSuites` | guarded | access::require(project, Role::Viewer) | `suites.rs:41` |
| `POST` | `/projects/{id}/test_suites` | `addProjectTestSuite` | guarded | access::guard_composition(Suites, target project, Editor) | `suites.rs:52` |
| `DELETE` | `/projects/{id}/test_suites/{suite_id}` | `removeProjectTestSuite` | guarded | access::require(project, Role::Editor) | `suites.rs:72` |
| `DELETE` | `/test_suites/{id}` | `deleteTestSuite` | guarded | access::guard_delete | `crud.rs:99` |
| `GET` | `/test_suites/{id}` | `getTestSuite` | guarded | access::guard_get | `crud.rs:52` |
| `PUT` | `/test_suites/{id}` | `updateTestSuite` | guarded | access::guard_update | `crud.rs:84` |
| `POST` | `/test_suites/{id}/duplicate` | `duplicateTestSuite` | guarded | access::guard_duplicate | `crud.rs:116` |
| `GET` | `/test_suites/{id}/test_cases` | `listTestSuiteCases` | guarded | access::require(suite's project, Role::Viewer) | `suites.rs:83` |
| `POST` | `/test_suites/{id}/test_cases` | `addTestSuiteCase` | guarded | access::guard_composition(Cases, suite's project, Editor) | `suites.rs:95` |
| `DELETE` | `/test_suites/{id}/test_cases/{case_id}` | `removeTestSuiteCase` | guarded | access::guard_removal(Suites, Editor) | `suites.rs:112` |
| `POST` | `/test_suites/{id}/test_cases/{case_id}/attachments` | `uploadTestSuiteTestCaseAttachment` | guarded | case_in_suite -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_suite)` |
| `DELETE` | `/test_suites/{id}/test_cases/{case_id}/attachments/{filename}` | `deleteTestSuiteTestCaseAttachment` | guarded | case_in_suite -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_suite)` |
| `GET` | `/test_suites/{id}/test_cases/{case_id}/attachments/{filename}` | `downloadTestSuiteTestCaseAttachment` | guarded | case_in_suite -> access::require(project, Role::Viewer) | `cases.rs:277 (case_in, via case_in_suite)` |
| `GET` | `/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `listTestSuiteTestCaseStepAttachments` | guarded | case_in_suite -> access::require(project, Role::Viewer) | `cases.rs:277 (case_in, via case_in_suite)` |
| `POST` | `/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments` | `uploadTestSuiteTestCaseStepAttachment` | guarded | case_in_suite -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_suite)` |
| `DELETE` | `/test_suites/{id}/test_cases/{case_id}/steps/{step_index}/attachments/{filename}` | `deleteTestSuiteTestCaseStepAttachment` | guarded | case_in_suite -> access::require(project, Role::Editor) | `cases.rs:277 (case_in, via case_in_suite)` |
| `DELETE` | `/test_runs/{id}` | `deleteTestRun` | guarded | access::guard_delete | `crud.rs:99` |
| `GET` | `/test_runs/{id}` | `getTestRun` | guarded | access::guard_get | `crud.rs:52` |
| `PUT` | `/test_runs/{id}` | `updateTestRun` | guarded | access::guard_update (Runs: home + every body/store project, Editor) | `crud.rs:84 + access.rs:412` |
| `POST` | `/test_runs/{id}/configurations` | `addTestRunConfiguration` | guarded | access::require_run_configuration(Role::Editor) | `runs.rs:182` |
| `DELETE` | `/test_runs/{id}/configurations/{config_id}` | `removeTestRunConfiguration` | guarded | access::require_run(Role::Editor) | `runs.rs:200` |
| `POST` | `/test_runs/{id}/duplicate` | `duplicateTestRun` | guarded | access::guard_duplicate | `crud.rs:116` |
| `POST` | `/test_runs/{id}/import/json` | `importJsonResults` | guarded | access::require_run(Role::Editor) | `runs.rs:172` |
| `POST` | `/test_runs/{id}/import/junit` | `importJUnitResults` | guarded | access::require_run(Role::Editor) | `runs.rs:160` |
| `POST` | `/test_runs/{id}/results` | `recordTestRunResult` | guarded | access::require_run(Role::Editor) | `runs.rs:113` |
| `GET` | `/test_runs/{id}/results/{case_id}/defects` | `listResultDefects` | guarded | access::require_run(Role::Viewer) | `runs.rs:123` |
| `POST` | `/test_runs/{id}/results/{case_id}/defects` | `linkResultDefect` | guarded | access::require_run(Role::Editor) | `runs.rs:134` |
| `DELETE` | `/test_runs/{id}/results/{case_id}/defects/{link_id}` | `unlinkResultDefect` | guarded | access::require_run(Role::Editor) | `runs.rs:147` |
| `POST` | `/test_runs/{id}/test_cases` | `addTestRunTestCase` | guarded | access::require_run_source(Runs->run projects, testCaseId project, Editor) | `runs.rs:95` |
| `POST` | `/test_runs/{id}/test_suites` | `addTestRunTestSuite` | guarded | access::require_run_source(Runs->run projects, suiteId project, Editor) | `runs.rs:77` |
| `DELETE` | `/test_cases/{id}` | `deleteTestCase` | guarded | access::guard_delete | `crud.rs:99` |
| `GET` | `/test_cases/{id}` | `getTestCase` | guarded | access::guard_get | `crud.rs:52` |
| `PUT` | `/test_cases/{id}` | `updateTestCase` | guarded | access::guard_update | `crud.rs:84` |
| `POST` | `/test_cases/{id}/attachments` | `uploadTestCaseAttachment` | guarded | require_test_case + access::require(project, Role::Editor) | `cases.rs:97 (via upload_attachment)` |
| `DELETE` | `/test_cases/{id}/attachments/{filename}` | `deleteTestCaseAttachment` | guarded | require_test_case + access::require(project, Role::Editor) | `cases.rs:155` |
| `GET` | `/test_cases/{id}/attachments/{filename}` | `downloadTestCaseAttachment` | guarded | require_test_case + access::require(project, Role::Viewer) | `cases.rs:127` |
| `POST` | `/test_cases/{id}/duplicate` | `duplicateTestCase` | guarded | access::guard_duplicate | `crud.rs:116` |
| `GET` | `/test_cases/{id}/history` | `listTestCaseHistory` | guarded | access::require(project, Role::Viewer) | `cases.rs:452` |
| `GET` | `/test_cases/{id}/history/{version}` | `getTestCaseVersion` | guarded | access::require(project, Role::Viewer) | `cases.rs:463` |
| `GET` | `/test_cases/{id}/steps/{step_index}/attachments` | `listStepAttachments` | guarded | require_test_case + access::require(project, Role::Viewer) | `cases.rs:183` |
| `POST` | `/test_cases/{id}/steps/{step_index}/attachments` | `uploadStepAttachment` | guarded | require_test_case + access::require(project, Role::Editor) | `cases.rs:212` |
| `DELETE` | `/test_cases/{id}/steps/{step_index}/attachments/{filename}` | `deleteStepAttachment` | guarded | require_test_case + access::require(project, Role::Editor) | `cases.rs:242` |
| `DELETE` | `/milestones/{id}` | `deleteMilestone` | guarded | access::guard_delete | `crud.rs:99` |
| `GET` | `/milestones/{id}` | `getMilestone` | guarded | access::guard_get | `crud.rs:52` |
| `PUT` | `/milestones/{id}` | `updateMilestone` | guarded | access::guard_update (Milestones: home + referenced projects, Owner) | `crud.rs:84 + access.rs:422` |
| `POST` | `/milestones/{id}/duplicate` | `duplicateMilestone` | guarded | access::guard_duplicate | `crud.rs:116` |
| `GET` | `/milestones/{id}/progress` | `getMilestoneProgress` | guarded | access::guard_get(Resource::Milestones) | `milestones.rs:76` |
| `GET` | `/reports/coverage` | `getCoverageReport` | guarded | access::scope + access::require(project, Role::Viewer) | `reports.rs:47,51` |
| `GET` | `/reports/summary` | `getSummaryReport` | guarded | access::scope + access::require(project, Role::Viewer) | `reports.rs:65,69` |
| `DELETE` | `/configurations/{id}` | `deleteConfiguration` | guarded | access::guard_delete | `crud.rs:99` |
| `GET` | `/configurations/{id}` | `getConfiguration` | guarded | access::guard_get | `crud.rs:52` |
| `PUT` | `/configurations/{id}` | `updateConfiguration` | guarded | access::guard_update | `crud.rs:84` |

---

## 4. Findings

### F-176-1: Bound element-nesting depth in the JUnit import so an over-deep document is rejected instead of aborting the process

- **Severity:** High
- **In scope:** S1 — trust boundary *JSON payload to domain model* (boundary 2; the JUnit import body is XML, but it crosses the same untrusted-request-body boundary the threat model names).
- **Where:** `POST /test_runs/{id}/import/junit` — `src/domain/import.rs:66` (`Document::parse(xml)`), reached by `src/api/runs.rs:160`.
- **Affected revision:** `096835bc108fae2428786c7900b5cf02e92a5dcf`
- **Reproduction:** POST a raw XML body `"<r>" + "<a>"*depth + "</a>"*depth + "</r>"` (7×(depth+1) bytes) with `content-type: application/xml` to a scratch run's `/import/junit`. Depths below ~3,405 elements answer `200` (imported: 0 or 1); at or beyond ~3,410 the process aborts. A flat document of the same byte count is accepted (`400 Malformed JUnit XML` or `200 imported: 0`) with the process alive — the crash is driven by *nesting*, not size.
- **Observed:** the ladder `100→200 (701 B)`, `1000→200 (7,001 B)`, `2000→200 (14,001 B)`, `3300→200 (23,107 B)`, `3400→200 (23,807 B)`, `3405→200 (23,842 B)`, then **`3410→abort (23,877 B)`**, and `3415`, `3420`, `3440`, `3450`, `3460`, `3600`, `4000`, `5000`, `10000` all abort. The control flat document (28,045 B) answered `400 Malformed JUnit XML` with the process alive; a flat 28,007 B document answered `200 imported: 0`; an *unclosed* document of 12,001 B aborts too. The container exits with status 139, `RestartCount` unchanged (no auto-restart), and `docker logs` records `thread 'tokio-rt-worker' (n) has overflowed its stack` plus `fatal runtime error: stack overflow, aborting`. The arm-A (auth off, no credentials) run at depth 4000 aborts identically. JSON import is safe: a 10,000-item JSON body imports (200, `imported: 10000`) and a 10,000-deep nested JSON object is refused (`400 "Import body must be valid JSON"`). DOCTYPE/XXE/billion-laughs and bad UTF-8 are already refused (`400 "Malformed JUnit XML"` / `"JUnit XML must be valid UTF-8"`).
- **Expected:** a request body that is too deeply nested must be refused with a stable, safe client error, not crash the process. Invariant 3 — "Client-visible errors use stable codes and safe messages with request IDs." An abort sends no code at all: the connection is reset, and every other tenant loses availability.
- **Impact:** remote denial of service on the whole installation, not confined to the caller. With `TUCANO_AUTH_REQUIRED` on (the shipped default) the caller needs a granted `editor` role on the run's project — exploitability Moderate; the crash takes down every tenant — impact **Severe**. Moderate × Severe = **High**. With auth off the base cell is Trivial × Severe = Critical, de-escalated one level (the project no longer recommends auth off) → also High.
- **Suggested fix:** enforce a maximum element-nesting depth in the JUnit importer (roxmltree bounds entity/reference depth at 10 but not element depth), so an over-deep document is rejected as `400 invalid_request` before recursion exhausts the worker stack. Add a regression test that posts an over-deep JUnit document and asserts `400` (the `tests/` import test that would close this gap does not yet exist).
- **CWE:** CWE-674 (Uncontrolled Recursion).
- **Duplicates / prerequisites:** none. The JUnit import path is distinct from JSON import, and DOCTYPE/XXE handling is already correct, so this is a separate gap.

### F-176-2: Make creation atomic so concurrent identical creates cannot each answer 201

- **Severity:** Medium
- **In scope:** S1 — trust boundary *HTTP client to service* (boundary 1); the mechanism is the unlocked `exists_at` on the create path, which is S2's `F-177-3`.
- **Where:** `src/storage/fs/crud.rs:154` — `create_at` calls `exists_at` (a `document(...).is_file()` check) **before** taking the lock, then `write_marker`/`write_at`/`write_json`.
- **Affected revision:** `096835bc108fae2428786c7900b5cf02e92a5dcf`
- **Reproduction:** fire N concurrent `POST /projects/{id}/test_cases` (or any create route) for the **same** identifier, arriving together. Because every request passes `exists_at` while the target does not yet exist, several proceed to write.
- **Observed:** across 11 rounds, 6 rounds returned more than one `201` (per-round winner counts `1,2,4,1,1,3,4,3`, maximum 4). Exactly one folder exists on disk afterwards; the stored document is one winner's title (version 1); the losing requesters' titles are `0` on disk (silently discarded); every `201` body is `{"id":"<same id>","message":"Test case created"}`. Each of the N clients is told its create succeeded while N−1 of them had their content dropped.
- **Expected:** a create against an identifier is exclusive. Invariant 2 — "Persistence publishes complete documents atomically; failed writes do not replace valid data." Here N clients are each acknowledged as successful, and N−1 complete documents are silently discarded — the same unlocked `exists_at` that S2's `F-177-3` records, whose own `Expected` (a duplicate create answers `409 conflict`) this finding falsifies on the HTTP surface.
- **Impact:** the lost content lies inside the caller's own authorization scope (an editor racing themselves), so impact **Moderate**; exploitability **Moderate** (a granted `editor` role plus a concurrent pair, reproduced on demand in 6 of 11 rounds — a wide window, not a narrow race). Moderate × Moderate = **Medium** with auth on. With auth off the cell is Trivial × Moderate = High, de-escalated one level (auth off is not recommended) → also Medium.
- **Suggested fix:** serialize the existence check and the write under the same lock (or otherwise make `create_at` atomic), so a create against an identifier that already exists answers `409 conflict`. Add a concurrency test that fires N simultaneous creates for one id and asserts exactly one `201` and `409` for the rest (the test that would close this gap does not yet exist).
- **CWE:** CWE-362 (Race Condition / TOCTOU).
- **Duplicates / prerequisites:** S2's `F-177-3` (storage-side sibling); this finding is its HTTP-observable face and falsifies that finding's `Expected`.

### F-176-3: Reject over-long identifiers and names with a client error instead of a 500 storage error

- **Severity:** Low
- **In scope:** S1 — trust boundary *Resource ID or filename to filesystem* (boundary 3).
- **Where:** `src/storage/layout.rs:367` (`validate_component` — rejects empty/`.`/`..`/`/`/`\`/NUL but has **no length bound**); `folder_name` (`:155`) and `validate_document_id` (`:236`) inherit it. Representative routes: `POST /projects/{id}/test_runs` with a `name` of 251 bytes; `GET /projects/{id}` with a 256-byte path identifier; `POST …/attachments` with a 304-byte client filename.
- **Affected revision:** `096835bc108fae2428786c7900b5cf02e92a5dcf`
- **Reproduction:** three faces. (a) *Name-derived create* — a `name` that makes the stored identifier exceed the filesystem `NAME_MAX` (255): 250 bytes → `201`, 251 → `500`. (b) *Path parameter* — only when auth is enforced, because `grant_path` builds the grants filename from the identifier before route validation; 251 bytes → `500` on arm B, `400 invalid_id` on arm A (auth off). (c) *Attachment* — a 304-byte client filename → `500` on both the case and step attachment routes; a 6-byte `ok.bin` → `201` (stored under a generated prefix).
- **Observed:** every `500` carries the constant, information-free body `{"error":{"code":"storage_error","message":"Storage operation failed", …}}`. The `ENAMETOOLONG`/over-long final component is not translated to a client error: `src/domain/error.rs` has no `InvalidFilename` arm, so it surfaces as `storage_error`. Controls hold elsewhere: the 255- and 256-byte path-parameter control on arm A answers `400 invalid_id` for all four lengths tested (255/256/4096/16384), and `name_bytes` 249/250 answer `201`.
- **Expected:** an over-long identifier is client input error. Invariant 3 — "Client-visible errors use stable codes and safe messages with request IDs." It must answer `400` (`invalid_id` or `invalid_request`), exactly as the auth-off path-parameter control and the 250-byte name already do — never `500`.
- **Impact:** confined to the caller's own request; the body leaks nothing and no state is corrupted — impact **Limited**. Exploitability **Moderate** with auth on (a granted role is needed to reach the route); Moderate × Limited = **Low**. With auth off the cell is Trivial × Limited = Medium, de-escalated one level → Low.
- **Suggested fix:** bound identifier and name length in `validate_component` (or translate over-long components to `invalid_id`/`invalid_request` in `error.rs`), and add a boundary test asserting that a 251-byte name and a 256-byte path identifier answer `400`, not `500` (the test that would close this gap does not yet exist).
- **CWE:** CWE-20 (Improper Input Validation).
- **Duplicates / prerequisites:** same class as S2's `F-177-4` (over-long identifier → 500).

### Recorded observations (not findings)

These are verified behaviors the audit could not turn into a finding — either because they are correct-but-surprising, or because they are owned by another surface's finding. They are recorded so the next surface does not re-derive them.

- **O-176-1 — Run-result recording loses acknowledged writes under concurrency.** 64 concurrent `recordTestRunResult` calls yielded 34×`200` + 30×`503` (`WouldBlock` lock timeout — an honest refusal), yet the result list length stayed `20001`, and only 6 of 64 markers persisted: **at least 28 acknowledged `200`s were silently discarded**. This is an extension of S2's `F-177-3` (which owns it); it is listed here because it is reachable from the HTTP result-recording route.
- **O-176-2 — The axum body-limit boundary answers in plain text, outside the JSON error envelope.** An over-limit body is refused with `413` and the literal `length limit exceeded` (21 bytes, `content-length: 21`), not a `{"error": …}` document; the `x-request-id` header is still present. This is the framework boundary, not the domain error translator.
- **O-176-3 — The effective upload ceiling is 2 MiB, not the declared 50 MiB.** `MAX_ATTACHMENT_BYTES = 50 MiB` (`src/domain/mod.rs:29`) and `MAX_BODY_BYTES = MAX_ATTACHMENT_BYTES` (`src/api/mod.rs:49`) are declared, but the served ceiling is axum's `DefaultBodyLimit` of 2 MiB (`grep DefaultBodyLimit src/` → absent, never raised). Measured: 2,097,152 bytes → `201`; 2,097,153 → `400`; 52,428,800 → `400`; 52,428,801 → `413`. The served `BodyTooLarge` description implies 50 MiB is reachable; it is not. The failure is fail-closed (no oversized file is ever stored).
- **O-176-4 — `users.json` grows without a per-account cap.** 283 tokens / 52,688 bytes observed with no per-account bound; refresh tokens are pruned only by the 14-day `DEFAULT_REFRESH_TTL`.
- **O-176-5 — An access token stays valid after logout.** Logout revokes the refresh token (`refresh` after logout → `401`), but the stateless JWT access token continues to answer `200` until it expires. This is inherent to stateless JWTs and is recorded as a lifecycle fact, not a finding.
- **O-176-6 — The client-chosen `x-request-id` is echoed back** when the client supplies it; the service generates one only when it is absent.
- **O-176-7 — `Project.testSuites` and the run body disagree.** A contract-shaped entry carrying `projects` is refused (`400 "Field \`projects\` is invalid"`), while the same body plus `testSuites` is accepted (`201`). A run's reachable projects are its `projects` array plus the suites it names; the two schemas describe the composition differently.
- **O-176-8 — Listings are unpaginated by design.** No `limit`/`offset` appears anywhere in `openapi.json`; `/projects` (2 entries) answers in ~1 ms and a 2 MB run in ~32 ms, so this is a documented design choice rather than a defect.
- **O-176-9 — The seeded grant is `owner` for `viewer`, and there is no grant-admin route.** The seed writes `viewer` as **owner** of `checkout.json` (not a plain viewer), `editor` as editor, and no grant on `payments.json`; accounts/grants are provisioned out of band, so a project can exist with no grant-holder until an administrator adds one.
- **O-176-10 — Identifier-error behavior diverges between auth on and off.** A no-grant caller to an ambiguous/unknown identifier receives `403` with auth on but `404`/`400`/`500` with auth off, because authorization is decided before existence and `grant_path` builds the grants filename before route validation. The `409 conflict` on a globally-ambiguous identifier (an id held by two projects) is returned to both `viewer` and `systemAdmin`, which closes the existence oracle for scoped reads.
- **O-176-11 — Sign-in is not throttled and does not distinguish accounts.** 200 sequential failures for a wrong password and for a non-existent user are indistinguishable (p50 ≈ 0.0199 s vs 0.0198 s, one `401 invalid_credentials` shape; a decoy Argon2 hash is verified for unknown users). 200 parallel valid logins all succeed with no `429`. This is an Info-level observation (CWE-307) and is carried as such rather than as a finding.
- **O-176-12 — The `/reports/summary` existence oracle is a credited control.** `?milestoneId=<no-such>` answers `404`, but a no-grant caller probing a *real* project is answered `403` before existence is checked, so the report routes cannot be used to enumerate milestones/projects.
- **O-176-13 — S3's `F-178-1` Critical premise was already fixed before the audited revision.** The design brief and S3 recorded that the shipped default was `TUCANO_AUTH_REQUIRED` off; commit `5b69ba3` made the default **on** (`docker-compose.yml:17 TUCANO_AUTH_REQUIRED: "${TUCANO_AUTH_REQUIRED:-true}"`). This report's severity axes for auth-off are therefore always reported as de-escalated (the project no longer recommends that mode).

---

## 5. Pass entries

A control the audit tested and could not break is recorded as a pass entry, with the test that proves it. Entries reference the trust boundary and security invariant they exercise; where the automated suite does not yet pin the control, the gap names the test that would close it.

1. **Anonymous callers are refused everywhere except the public surface.** Every guarded operation answered `401 missing_token` + `WWW-Authenticate: Bearer realm="Tucano Test API"` to an unauthenticated request (S1-3). *Boundary 1 / Invariant 3.* Pinned by the hand-marked public surface in `tests/service.rs` and the anonymous-refusal matrix in `tests/auth.rs`.
2. **A random 32-byte bearer is refused everywhere.** 78/78 guarded operations answered `401 invalid_token` to a well-formed but unissued token. *Boundary 1.*
3. **The public surface is exactly seven routes.** `GET /health`, `GET /ready`, `GET /openapi.json`, `GET /api-docs`, `GET /diagnostics`, `POST /auth/login`, `POST /auth/refresh` — nothing more. *Boundary 1.* Pinned by the five-public-endpoints tests in `tests/auth.rs` (two Swagger mounts are additional, recorded in § 3 drift).
4. **Token integrity is enforced.** Tampered payload, tampered signature, `alg=none` (four spellings), HS256 with a wrong or empty secret, a missing signature, and a non-base64 signature all answered `401 invalid_token`. *Boundary 1 / Invariant 7.*
5. **Access-token expiry is enforced.** On arm `audit-c` (`TUCANO_ACCESS_TOKEN_TTL=1s`) the login returns `expiresIn: 1`, the token is accepted at t+0 and rejected at t+2 as `401 token_expired` with `WWW-Authenticate: … error="invalid_token"`. *Boundary 1.*
6. **Refresh tokens are single-use.** A second spend of the same refresh token answered `401 invalid_refresh_token`; two concurrent spends produced exactly one `200` (5/5 and 6/6 rounds). *Boundary 1 / Invariant 7.*
7. **Logout revokes the refresh token.** `refresh` after `logout` answered `401` (the still-valid access token is O-176-5). *Boundary 1.*
8. **The role matrix holds.** `viewer`/`editor` read `checkout.json` (`200`) and are refused `payments.json` (`403`, except a run whose id points there → `404`); `systemAdmin` reads everything; write attempts across the grant boundary (`viewer` PUT/DELETE payments, `editor` PUT/DELETE checkout) answered `403`. *Boundary 1.* Pinned by `tests/auth.rs`.
9. **Cross-project IDOR is closed.** A caller with a grant on one project is answered `403` for read and write operations that name another project's resources (S1-7). *Boundary 1.*
10. **Run scope cannot be widened by a non-admin.** An editor/owner widening a run answered `403 "This account needs the editor role in the project"`; `systemAdmin` widen answered `200`; an owner narrowing answered `200`; re-widening again `403`. *Boundary 1.* Pinned by `tests/auth.rs` (run-scope matrix).
11. **Report routes scope-filter rather than leak.** A no-grant caller to `/reports/coverage` and `/reports/summary` (no filter) answered `200` with scope-filtered data; `?projectId=payments.json` answered `403`. *Boundary 1.*
12. **Path-traversal reads are contained.** 47 templates × 29 hostile values = 2,088 requests answered `400 invalid_id` ×1502, `400 invalid_request` ×80, `400 empty-body` ×162, `404 not_found` ×164, `404 route-mismatch` ×180 — **zero `500`s** and no file outside the configured root. *Boundary 3 / Invariant 1.* Pinned by `tests/security_tests.rs`.
13. **Path-traversal writes are contained.** 8 name-derived creates × 20 hostile values = 160 requests: `201` ×48, `400` ×74, `409` ×33, `500` ×5 (the five are the F-176-3 over-long face, not a traversal). Reserved project-child names answered `409`; every write stayed inside the entity's own folder. *Boundary 3 / Invariant 1.*
14. **The path-parameter control holds where auth is off.** 255/256/4096/16384-byte identifiers answered `400 invalid_id` on arm A. *Boundary 3.*
15. **Multipart abuse is contained.** No part → `400 missing_file`; a traversal/absolute/forbidden-separator filename → `404 not_found`; `.`/`..` and `..%2F..%2Fescape.txt` → `201` stored under a generated prefix; a 1000-part body answered `201` with surplus parts ignored. *Boundary 5.*
16. **Attachments are served as downloads, not inline content.** `.html`, `.svg`, `.js`, `.txt` all returned `content-type: application/octet-stream` + `content-disposition: attachment`, removing the stored-XSS-via-MIME vector. *Boundary 5.*
17. **Upload size fails closed.** 2,097,152 bytes → `201`; 2,097,153 → `400 invalid_multipart`; 52,428,800 → `400`; 52,428,801 → `413 length limit exceeded`. No oversized file is ever stored (the 2 MiB vs 50 MiB discrepancy is O-176-3). *Boundary 5.*
18. **The error envelope and request id hold.** A corrupted run document (truncated JSON, invalid UTF-8, empty, mode `000`) answered `500 storage_error "Storage operation failed"` with a matching `x-request-id`; a directory in place of a file answered `404`; restoring the control document answered `200`. No path, OS error, stack trace, or file content appeared in any response or log. *Boundary 8 / Invariant 3, 6.*
19. **Logs exclude secrets and raw payloads.** The design's "highest-probability S1 finding" (S1-10.2, path/stack/content leak in logs) did **not** reproduce in any response or log. *Boundary 8 / Invariant 6.* Pinned by the log-redaction tests and the startup-error redaction test (`#188`–`#190`).
20. **Credentials are never stored or logged in the clear.** Passwords are persisted as Argon2id PHC hashes and refresh tokens as SHA-256 digests (verified in the seeded `auth/` files). *Boundary 8 / Invariant 7.* Pinned by the plaintext-absence tests in `tests/auth.rs`.
21. **Concurrency is bounded and refuses honestly.** 64 concurrent result recordings answered 34×`200` + 30×`503` (`WouldBlock` lock timeout) with no corruption and no crash (the silently-discarded `200`s are O-176-1, owned by S2). *Boundary 4.*
22. **Sign-in timing and response shape do not distinguish accounts.** Wrong-password and no-user failures are indistinguishable (one `401 invalid_credentials` shape; decoy Argon2 hash at `session.rs:135`). *Boundary 1.* Pinned by the identical-sign-in-failure-answer tests in `tests/auth.rs`.
23. **Sign-in throughput is unbounded but stable.** 200 parallel valid logins all answered `200` with no `429` and no restart. The absence of throttling is O-176-11 (Info, CWE-307) — a gap the sign-in test suite does not yet pin.
24. **Boundary 9 (GUI → storage) is not an attack surface.** The GUI reaches storage only through the documented HTTP API; no route grants direct filesystem access. *Boundary 9 / Invariant 4.*
25. **Auth-off parity was measured and cross-referenced, not re-reported.** 626 probes on arm A distributed `288×400, 158×401, 90×404, 58×405, 16×403, 15×200, 1×500`, cross-referenced to S3's `F-178-1`/S3-3 per the design (S1 does not write a finding whose only content is "auth is optional"). *Boundary 1.*
26. **Globally-ambiguous identifiers fail closed.** `GET /test_cases/TC-LOGIN-1` and `GET /test_suites/portable.checkout.json` (an id held by two projects) answered `409 conflict` to both `viewer` and `systemAdmin`; a bare `/test_runs/` answered `404`; a malformed run/result identifier answered `400 invalid_id`. *Boundary 3.*
27. **Authorization precedes existence on scoped routes.** A no-grant caller `PUT` of an unknown case with an invalid body answered `403` (not `400`/`404`), so the route does not leak existence to callers without a grant. *Boundary 1.*
28. **The JUnit import rejects known-XML abuse.** DOCTYPE/XXE/billion-laughs answered `400 "Malformed JUnit XML"`; bad UTF-8 answered `400 "JUnit XML must be valid UTF-8"`. The remaining nesting gap is F-176-1. *Boundary 2.*
29. **The JSON import is bounded and safe.** A 10,000-item JSON body imported (`200 imported: 10000`); a 10,000-deep nested JSON object was refused (`400 "Import body must be valid JSON"`). *Boundary 2.*
30. **Attachment step-index validation holds.** Step index `99` → `400 out of range`, `-1` → `400 non-negative`, `abc` → `400 non-negative`. *Boundary 2.*

**Pass-entry gaps (tests that would close each):** F-176-1 (a JUnit over-deep document → `400`), F-176-2 (N concurrent identical creates → one `201` + rest `409`), F-176-3 (251-byte name and 256-byte path identifier → `400`, not `500`), O-176-11 (a sign-in throttling / timing-equality test), O-176-5 (an explicit "access token valid after logout" lifecycle test that documents the stateless behavior). Each is stated as a **recommendation** in the finding's `Suggested fix` or the observation above; this audit PR writes no test.

---

## 6. Calibration

Severity is read off the rubric in [audit-scope.md § 5](audit-scope.md#5-severity-rubric): Exploitability (Trivial / Moderate / Difficult) × Impact (Severe / Moderate / Limited), with the escalation and de-escalation rules applied.

- **F-176-1 — High.** Moderate exploitability (a granted `editor` role and one request) × Severe impact (whole-installation availability loss, beyond the caller's scope) = High. The auth-off cell is Trivial × Severe = Critical, de-escalated one level because the project no longer recommends auth off → High. One `Severity:` value (High); the cell is stated in the finding body.
- **F-176-2 — Medium.** Moderate × Moderate = Medium. Auth-off cell Trivial × Moderate = High, de-escalated → Medium.
- **F-176-3 — Low.** Moderate × Limited = Low. Auth-off cell Trivial × Limited = Medium, de-escalated → Low.

The two worked examples in the rubric were reproduced as a sanity check, and the same matrix is applied unchanged above. `Critical` remains reserved for the top row of the matrix with a `Severe` impact; none of the three S1 findings reaches that cell once the (unrecommended) auth-off mode is de-escalated.

---

## 7. Tear-down

The throwaway stack was removed at the end of the audit. Results:

| Step | Command | Result |
| --- | --- | --- |
| Stop the three arms | `docker rm -f audit-a audit-b audit-c` | succeeded |
| Clear the data volume (from inside the container, per § 2) | `docker run --rm … sh -c 'rm -rf /data/* /data/.[!.]* …'` | succeeded |
| Remove the host throwaway dir | `rm -rf /tmp/audit-rev/data && rm -rf /tmp/audit-rev` | `ls -d /tmp/audit-rev` → "No such file or directory" |
| Confirm no tracked containers remain | `docker ps -a --filter label=ao.session=…` | empty |

**Live at the end of the audit:** the operator's `tucano-test-api-1`, `tucano-test-gui-1`, and `open-webui` containers were `Up` before, during, and after the audit and were never touched. The audit image `tucano-test-audit:096835bc…` (id `34cb67183ce6`) is retained for reproducibility, as are the earlier `tucano-test-audit:latest`, `tucano-test-audit:c5e9943…`, and `tucano-test-audit-177:c5e9943` tags. The repository working tree was pristine throughout: `git status --porcelain` is empty at `096835bc…`, and the seeded data tree (62 files / 61 directories) was restored to a name set identical to the seed listing before teardown.

---

## What the sub-tasks of this checkpoint do not cover

S1 is scoped to the HTTP surface. The storage-and-filesystem behavior that S1 touched only to reach the HTTP routes — the advisory lock, atomic-write machinery, symlink handling, and corruption recovery — is S2's ([`audit-s2-storage-and-filesystem.md`](audit-s2-storage-and-filesystem.md), #177). The container and deployment posture is S3's ([`audit-s3-container-and-deployment.md`](audit-s3-container-and-deployment.md), #178), and dependency/supply-chain is S4's ([`audit-s4-dependencies-and-supply-chain.md`](audit-s4-dependencies-and-supply-chain.md), #179). Those surfaces are **cross-referenced here, not re-reported**: O-176-1 and F-176-2 defer to S2's `F-177-3`, and the auth-off severity axis defers to S3's `F-178-1`.
