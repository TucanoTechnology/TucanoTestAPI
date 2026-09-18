# Audit design and decomposition — S1 (#176), S2 (#177), S3 (#178)

Epic: [#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — Carry out security audit.
The normative method is [`audit-scope.md`](audit-scope.md); the boundaries, abuse cases, and invariants
quoted below are from [`threat-model.md`](threat-model.md); the scan claims reviewed by #178 are from
[`scanning-policy.md`](scanning-policy.md). The report precedent this design copies is
[`audit-s4-dependencies-and-supply-chain.md`](audit-s4-dependencies-and-supply-chain.md) (#179, closed).

**What this document is.** A decomposition. It states, for each of #176, #177, and #178, exactly what the
audit covers, resolves every ambiguity in advance with a concrete decision, and lists the sub-tasks in the
order they must run so that a flash-tier execution model can carry each audit out with no further
questions.

**What this document is not.** It is not an audit. It contains no findings. It runs no probes, records no
observations, and asserts nothing about whether any control holds. Every "expected" line below is the
*control that must hold*, not a measured result — the audit's job is to measure and report the difference.
Where this document predicts a probable finding, it says **candidate** and still requires the executor to
reproduce it before it may be written as a finding.

**Source of truth.** Read the contract from `openapi.json` and the served Swagger UI, never from memory or
from this document. Read the current code, never a line number from this document alone: every line
reference here was true at the revision this design was written against and must be re-checked at the
audited revision with the grep given alongside it.

---

## Shared execution contract

Every one of #176, #177, and #178 runs inside the rules below. They are the verbatim commitments binding
this audit series; a sub-task that cannot be carried out inside them is not carried out.

> The audit is an **external penetration test of the repo "as deployed"**: an adversary who can reach the
> HTTP listener, send any body, upload any file, read any corner of the data volume.
>
> Target = the merge-base commit the audit records in its report. Never audit a moving `main`.
>
> A **throwaway** Compose project with its **own volume** and a **free host port** is mandatory for every
> audit. Never point an audit at the operator's long-lived instance: Compose project `tucano-test` /
> container `tucano-test-api-1`. **Never stop, rebuild, or point anything at it.**
>
> Fixes are **out of scope** for every audit task. An auditor records a finding and moves on.
>
> Baselines are **not** findings: the four `security.yml` CI jobs and the existing `tests/security_tests.rs`
> / `tests/auth.rs` coverage are the floor, not gaps. A finding must be a **gap or a bypass** — a path the
> tests do not exercise, a control that is present but not reached, or a documented control that does not
> hold.
>
> A claim **without a reproduction** is not a finding.
>
> The audit artifact is a standalone report document under `docs/security/`, shipped in a report PR, with
> one `###` section per finding using the verbatim template in `audit-scope.md` §4. The report must record
> the merge-base commit it ran against.
>
> Never edit `AgentRules/`. Never merge PRs. Assign every PR and issue you open to `ECiurleo`.

### 1. Revision to audit

```bash
# A.2 — pin the revision before anything else, and record all three values in the report.
git fetch origin
git rev-parse origin/main                  # the merge base; record it in full
git log -1 --format='%H %cI %s' origin/main
sha256sum Cargo.lock                       # the S4 report records this; keep the field
git status --porcelain                     # must be empty: never audit a dirty tree
```

Build and run only from that revision — never from a working tree that has moved:

```bash
export REV=$(git rev-parse origin/main)
export AUDIT_ROOT=/tmp/audit-rev
rm -rf "$AUDIT_ROOT"
git worktree add --detach "$AUDIT_ROOT" "$REV"
sha256sum "$AUDIT_ROOT/Cargo.lock"          # must equal the value above
```

If `origin/main` moves while the audit runs, **do not follow it**. The report states the revision it ran
against; a later commit is a different audit.

### 2. The throwaway target (mandatory, one per audit)

One image built at `$REV`, one private data directory shared by two containers, two arms so that every
control is exercised both with `TUCANO_AUTH_REQUIRED` off (the shipped default) and on.

**Why a host directory rather than a `docker volume`.** `scripts/seed.mjs` provisions accounts and grants
by writing JSON under `TUCANO_DATA_DIR` on the host (`TUCANO_SEED_AUTH_CMD` overrides this). A named volume
is not addressable from the seed script, so the throwaway "volume" is a dedicated directory that only this
audit creates, uses, and deletes. It is never `./data`, never the Compose volume of the operator's stack,
and never shared with any other audit. This is the *only* deviation this design makes from a literal
`docker volume create`, and it is recorded in the report's tear-down section.

```bash
# A.3 — build the audit image at $REV. Tag it so it can never be confused with the operator's.
export AUDIT_IMAGE=tucano-test-audit:$REV
docker build --file "$AUDIT_ROOT/Dockerfile" --build-arg BUILD_NUMBER=audit \
  --tag "$AUDIT_IMAGE" "$AUDIT_ROOT"

# A.4 — the private data directory ("own volume"). uid 10001 is the image's `tucano` user: the
# bind-mount ownership gotcha recorded by the S4 audit. Chown before starting anything.
export AUDIT_DATA=/tmp/audit-rev/data
rm -rf "$AUDIT_DATA"; mkdir -p "$AUDIT_DATA"
chown -R 10001:10001 "$AUDIT_DATA"
export TUCANO_DATA_DIR="$AUDIT_DATA"        # the seed/teardown scripts run in this shell

# A.5 — free host ports. 3210/3211 are free and are NOT 3100 (the operator's published port).
# Bind to 127.0.0.1 for #176 and #177: those audits probe the HTTP surface, not the network edge.
# #178 deliberately binds to all interfaces — see its own section.

# Arm A — the shipped default: TUCANO_AUTH_REQUIRED unset.
docker run -d --name audit-a \
  --label ao.session="$AO_SESSION_ID" \
  -p 127.0.0.1:3210:3000 -v "$AUDIT_DATA":/data \
  --read-only --tmpfs /tmp --security-opt no-new-privileges:true \
  "$AUDIT_IMAGE"

# Arm B — auth enforced. Secret and bootstrap credentials are generated, never typed by hand.
export AUDIT_JWT_SECRET=$(openssl rand -hex 32)          # 64 chars > the 32-byte minimum
export AUDIT_BOOTSTRAP_PASSWORD=$(openssl rand -hex 16)
docker run -d --name audit-b \
  --label ao.session="$AO_SESSION_ID" \
  -p 127.0.0.1:3211:3000 -v "$AUDIT_DATA":/data \
  --read-only --tmpfs /tmp --security-opt no-new-privileges:true \
  -e TUCANO_AUTH_REQUIRED=true \
  -e TUCANO_JWT_SECRET="$AUDIT_JWT_SECRET" \
  -e TUCANO_BOOTSTRAP_USERNAME=auditor \
  -e TUCANO_BOOTSTRAP_PASSWORD="$AUDIT_BOOTSTRAP_PASSWORD" \
  "$AUDIT_IMAGE"

export A=http://127.0.0.1:3210      # auth off (the default)
export B=http://127.0.0.1:3211      # auth on
```

**Guardrail, before and after every session.** The operator's long-lived instance must be a bystander.
Record its state and never touch it:

```bash
docker ps --filter name=tucano-test-api-1 --format '{{.Names}}\t{{.Status}}\t{{.Ports}}'
docker ps --filter name=tucano-test-api --format '{{.Names}}\t{{.Status}}'
# The report's tear-down section states this was Up before, during, and after. If it is ever not Up,
# stop and report; do not attempt to repair it.
```

**Seed once, through arm B.** `scripts/seed.mjs` refuses to run unless authentication is enforced, so both
#176 and #177 seed through arm B and then read the same data directory from either arm.

```bash
# A.6 — seed. Auth is enforced on arm B; the viewer account is granted `owner` on checkout.json only.
TUCANO_SEED_VIEWER_PASSWORD=$(openssl rand -hex 12) \
TUCANO_JWT_SECRET="$AUDIT_JWT_SECRET" \
node "$AUDIT_ROOT/scripts/seed.mjs" "$B"
# The seed is not idempotent, by design: it runs exactly once per throwaway data directory.
# Re-running it means deleting $AUDIT_DATA and starting from A.4.

# A.7 — smoke both arms. Arm A must pass with no caller; arm B must answer 401 `missing_token`.
bash "$AUDIT_ROOT/scripts/smoke.sh" "$A"
bash "$AUDIT_ROOT/scripts/smoke.sh" "$B"     # expected to FAIL (401s): that is the point
```

Record the seeded identifiers the probes depend on, because the whole audit references them:

| Seeded fact | Value | Used by |
| --- | --- | --- |
| Projects | `checkout.json`, `payments.json` | every role/IDOR probe |
| Viewer account | `TUCANO_SEED_VIEWER_PASSWORD` (generated above) | every 403 probe |
| Viewer grants | `owner` on `checkout.json`; **no grant** on `payments.json` | the cross-project probe |
| Suites | `smoke.checkout.json`; `regression.checkout.json` (left empty); `smoke.payments.json` | suite-level guards, the empty-collection case |
| Cases | `TC-LOGIN-1`, `TC-LOGIN-2`, `TC-CART-1`, `TC-MOVE-1`, `TC-PROJECT-1`, `TC-ORDERS-1`, `TC-CATALOG-1`, `TC-SEARCH-1` | case-level probes |
| Dual-home case | `TC-LOGIN-1` and `TC-ORDERS-1` copied into `payments.json` by seed step 11; `TC-MOVE-1` is *moved* off its first home four times and ends in `payments.json/smoke.payments.json` | the ambiguity/IDOR probe |
| Dual-home suite | `portable.checkout.json`, moved into `payments.json` and copied back, so it is held by both projects | the ambiguity probe on `GET /test_suites/{id}` |
| Attachments | one on each of the eight cases, plus seven on their steps: step 0 of `TC-LOGIN-1`, `TC-CART-1`, `TC-MOVE-1`, `TC-ORDERS-1` and `TC-SEARCH-1`, steps 0 and 1 of `TC-LOGIN-2` | attachment probes; `TC-PROJECT-1` and `TC-CATALOG-1` are the cases whose steps carry none |
| Run | `nightly.json` (+ imported `nightly-import.json`) | run guards, run scope |
| Milestone | `v1.0.json` | milestone guards |
| Configurations | `chrome-linux.json`, `firefox-linux.json` | configuration guards |

**Tear-down — step 7, "leave nothing running".** Do this even after a failed probe run.

```bash
node "$AUDIT_ROOT/scripts/teardown.mjs" "$B"     # or `docker rm -f` if the target is wedged
docker rm -f audit-a audit-b audit-178
rm -rf /tmp/audit-rev/data /tmp/audit-178
docker rmi "$AUDIT_IMAGE"
git worktree remove --force "$AUDIT_ROOT"
docker volume ls | grep -i audit                  # must be empty
docker ps -a --filter label=ao.session="$AO_SESSION_ID"   # must be empty
docker ps --filter name=tucano-test-api-1         # the operator's instance: still Up
```

### 3. Baselines that are not findings

Credit each of these by name in the report's pass entries — the task's DoD requires it. A finding must be
a *gap or bypass* on top of them.

| Baseline | Proves | Report it as |
| --- | --- | --- |
| `tests/security_tests.rs` (216 lines) | traversal (`..`, absolute, `..%2F..%2Fetc%2Fpasswd.json`, traversal in write, traversal through a case identifier, `../escape`/`/etc/passwd` in a case id), symlink escape, malformed JSON, schema validation at the API layer, atomic write on disk, concurrent writes do not corrupt (`Barrier::new(2)`) | pass entries, per control |
| `tests/auth.rs` (21 matrix tests) | anonymous refusal on every guarded operation, the five public endpoints, viewer/editor/owner/systemAdmin matrices, spent refresh token, logout revokes, foreign-secret token refused, accounts file free of plaintext/usable tokens, grants file shape, identical sign-in failure answers, flat create routes name their replacement | pass entries, per control |
| `tests/service.rs::openapi_document_matches_the_registered_routes` | the mounted router and the published contract agree (`api::ROUTES` minus `api::UNDOCUMENTED_ROUTES` == the documented paths, and every declared route is served) | the §6 "implementation and contract disagree" pass entry |
| `src/storage/layout.rs` unit tests | nothing outside the root is reachable, no non-`.json` file type in the project tree, reserved collection names, hostile components, symlinked project/collection directories refused | pass entries under S2 |
| `src/storage/fs.rs` atomic-write tests | same-directory temp, `sync_all()`, `rename` | invariant 2 pass entry |
| `.github/workflows/security.yml` (4 jobs) | `cargo audit`, gitleaks, trivy on the local build, SBOM generation + validation | the S3 CI review's *floor*; deltas only |
| `audit-s4-dependencies-and-supply-chain.md` (1 Low, 4 Info) | supply-chain findings **F-179-1 … F-179-5** | cited under `Duplicates / prerequisites` — never re-raised as new |

### 4. Evidence discipline and finding shape

Every finding is written with the **verbatim** template from `audit-scope.md` §4 — no field added, none
dropped:

```markdown
### F-<task>-<n>: <one-line title, imperative and specific>

- **Severity:** <Critical|High|Medium|Low|Info> — <Trivial|Moderate|Difficult> × <Severe|Moderate|Limited>
- **In scope:** S1 | S2 | S3 | S4 — <trust boundary, by the name used in threat-model.md>
- **Where:** `METHOD /route` or `path/to/file.rs:LINE`
- **Affected revision:** <full SHA>
- **Reproduction:** <fenced blocks, from a clean seeded deployment>
- **Observed:** <what happened>
- **Expected:** <the invariant text, quoted>
- **Impact:** <with authentication off or on stated explicitly>
- **Suggested fix:** <direction only; no code in the audit>
- **CWE:** CWE-<n>
- **Duplicates / prerequisites:** <or "none">
```

Rules the executor must not bend:

- **A claim without a reproduction is not a finding.** Paste the exact request, the exact response, and the
  command that proves the state of the volume or the container. "See the audit" is not valid.
- **Expected quotes the invariant text.** Copy the invariant from `threat-model.md` verbatim; do not
  paraphrase it.
- **Severity is scored, not felt.** Use the §5 rubric; every escalation or de-escalation states its reason
  in the finding. Escalate one level when the defect is remotely reachable in a default configuration (the
  shipped Compose stack or a bare `cargo run`); de-escalate one level for an unrecommended configuration or
  a threat-model-excluded precondition; **never de-escalate below the impact axis**.
- **Credit what already holds** as a one-line pass entry under the task's scope, naming the test or command
  that proves it.
- Fixes are out of scope. No file under `src/`, `tests/`, `.github/`, or `AgentRules/` is modified by an
  audit PR.

### 5. Report pull-request shape

Modelled field-for-field on `audit-s4-dependencies-and-supply-chain.md`, which is the accepted precedent.

```
docs/security/audit-s1-http-surface.md            (#176)
docs/security/audit-s2-storage-and-filesystem.md  (#177)
docs/security/audit-s3-container-and-deployment.md (#178)
```

Each report is a standalone document with this skeleton:

1. **Header** — `# Security Audit S<n> — <surface> (Issue #<n>)`, the Issue line, the Epic line
   (`#166`), the surface sentence quoted from `audit-scope.md` §2, and the sentence
   "It carries findings only; nothing here is fixed." Then a bullet block: Affected revision, Method,
   Findings summary (`<n>` findings: `a` Critical, …), Pass entries (`<n>`).
2. **§1 Revision pinned** — the table: repo revision (full SHA), `origin/main` at audit time = the
   merge base, `Cargo.lock` SHA-256, and for #178 the built image id. State that the audit did not follow a
   moving `main`.
3. **§2 Throwaway target (step 2)** — one row per arm: arm name, host port, `TUCANO_AUTH_REQUIRED`, the
   smoke outcome, and the container facts (`Config.User`, `ReadonlyRootfs`, `no-new-privileges`, `/tmp`
   tmpfs, the private data directory). State explicitly that the operator's instance was untouched.
4. **§3 Surface enumerated before probing (step 3)** — counts, not assertions. For #176: 49 documented
   paths / 71 documented operations / exactly 5 public operations, the full per-route classification
   table, and the router-vs-contract diff. For #177: the enumerated persistence paths, the enumerated
   lock/unlock pairs, the enumerated permission call sites. For #178: the enumerated Compose keys,
   Dockerfile instructions, and `security.yml` claim rows. **Re-run these enumerations at the audited
   revision** — the numbers below are the design-time values and are dated by definition.
5. **§4 Findings** — severity order, most severe first, each in the §4 template above.
6. **§5 Pass entries** — one line each, naming the proving test or command.
7. **§6 Calibration confirmed** — the §5 calibration examples from `audit-scope.md`, re-confirmed or
   corrected against this revision.
8. **§7 Tear-down** — every container, image, directory, and worktree removed; the operator's instance
   `Up` before, during, and after; any leftover reported rather than hidden.

**PR mechanics.** `gh pr create --base main --title "docs(security): S<n> audit report (#<n>)"`, body
linking the task, the epic `#166`, and this design document; **assign `ECiurleo`**
(`gh pr edit <pr> --add-assignee ECiurleo`); add the report row to the README documentation table
(`README.md` lines 704–709 hold the security table, where the S4 report is already listed) as the task's
DoD requires; do not merge. One PR per audit, or one stacked PR if the executor prefers — see the closing
section.

---

## #176 — S1: HTTP surface

### 1. Scope

The audit attacks the HTTP listener as an external adversary: it authenticates, or fails to, as any
principal it can obtain; it addresses every route the contract publishes with the identifiers, keys,
bodies, filenames, and encodings of its choosing; and it measures whether authentication, authorization,
input validation, upload limits, overflow handling, and error output hold under that pressure. In
`audit-scope.md` §2's words, S1 is the audit of the **HTTP surface**. Concretely: the mounted router in
`src/api/mod.rs` and the contract in `openapi.json`; the `Principal` extractor and the guards in
`src/api/access.rs`; the traversal surface of every identifier and filename in the request; the four
attachment routes plus the two run-import routes; the sign-in/refresh/logout lifecycle in `src/auth/`; and
the error envelope produced by `src/api/error.rs`. The acceptance criteria are the threat model's invariants
**1, 3, 6, and 7** and trust boundaries **1 (HTTP client → service), 2 (JSON payload → domain model),
3 (Resource ID or filename → filesystem), 5 (Attachment upload → storage), and 8 (service logs and audit
events)** — plus **boundary 9 (GUI → storage)** in the narrow sense §3 permits: the audit tests the claim
that the service provides no filesystem-access path to an HTTP client. Boundaries **4, 6, and 7** belong to
#177 and #178 and are not re-tested here.

### 2. Ambiguities, resolved

**2.1 What exactly is the HTTP surface?** The router registers **55 route entries** (`src/api/mod.rs`,
`api::ROUTES`) of which **6 are declared undocumented** (`api::UNDOCUMENTED_ROUTES`): the trailing-slash
Swagger alias `/api-docs/` and the five retired flat creation paths `/test_suites`, `/test_cases`,
`/test_runs`, `/milestones`, `/configurations`, kept only to explain where creation moved and still served
for global reads. `openapi.json` publishes **49 paths / 71 operations**. The router-vs-contract diff must be
re-derived at the audited revision:

```bash
python3 - <<'PY'
import json, re, pathlib
doc = json.load(open("openapi.json"))
ops = sum(len([m for m in v if m != "parameters"]) for v in doc["paths"].values())
print("paths", len(doc["paths"]), "operations", ops)
pub = [(m.upper(), p) for p, v in doc["paths"].items()
       for m, o in v.items() if isinstance(o, dict) and o.get("security") == []]
print("public", sorted(pub))
src = pathlib.Path("src/api/mod.rs").read_text()
def array(name):
    m = re.search(rf"pub const {name}: &\[&str\] = &\[(.*?)\];", src, re.S)
    return set(re.findall(r'"([^"]+)"', m.group(1)))
routes = array("ROUTES")
docs = {(m.upper(), p) for p, v in doc["paths"].items()
        for m, o in v.items() if isinstance(o, dict)}
print("served but undocumented:", sorted({("ANY", r) for r in routes} - docs))
print("documented but unserved:",
      sorted({(m, p) for m, p in docs if p not in routes}))
PY
```

Expected: `paths 49 operations 71`; the public set is exactly `GET /health`, `GET /openapi.json`,
`GET /api-docs`, `POST /auth/login`, `POST /auth/refresh`; served-but-undocumented is exactly the six
declared entries; documented-but-unserved is empty. If the numbers have moved, that is a fact to record,
not an error.

**2.2 Which routes are public and which are guarded, and which guard?** The task's DoD requires every
route classified as public or guarded *naming the guard it uses*. Public means exactly the five operations
carrying `security: []` above. Guarded means a handler that declares the `Principal` extractor and calls one
of these, all in `src/api/access.rs` — capture the line number at the audited revision with
`grep -n "pub(crate) fn \|pub fn " src/api/access.rs`:

| Guard | Governs | Role required | Notes |
| --- | --- | --- | --- |
| `guard_get` | reads of one resource | reachability of the resource's home project | listing reads are filtered, not refused |
| `guard_create` | creation inside a home | Projects/Milestones → owner; Suites/Cases/Runs/Configurations → editor | home is required at creation |
| `guard_project_create` | `POST /projects` | `systemAdmin` | |
| `guard_update` | updates | owner for Projects/Milestones; editor for Suites/Cases/Configurations; Runs → editor over the home **and** every project the run's `projects` array reaches | |
| `guard_delete` | deletions | same role as update for the resource | |
| `guard_duplicate` | `POST /projects/{id}/duplicate` | `require_admin` | |
| `guard_composition` | copy/move into a parent | editor of both source and target | `mode` defaults to `copy` |
| `guard_removal` | removing an entity from a home | editor of both | |
| `require_run` / `require_run_source` | run reads and run-source reads | editor/read per reached project | |
| `filter_list` / `all_within` | list filtering | — | a listing is filtered to reachable projects rather than refused |

Call sites to cross-check (`src/api/`): `crud.rs`, `cases.rs:55`, `runs.rs`, `configurations.rs:52`,
`milestones.rs:54,76`, `suites.rs:52,69,92,109`. The classification table goes into the report §3.

**2.3 What is the trust boundary for each probe?** Use the threat-model names verbatim in `In scope:`:
*HTTP client to service*; *JSON payload to domain model*; *Resource ID or filename to filesystem*;
*Attachment upload to storage*; *Service logs and audit events*; *GUI to storage*.

**2.4 Which invariants are acceptance criteria?**
1. "The configured data root is the only filesystem area the repository may read or write."
3. "Client-visible errors use stable codes and safe messages with request IDs."
6. "Security-sensitive events exclude credentials, tokens, raw payloads, attachment contents, and internal
   paths."
7. "Credentials are never stored or logged in the clear: passwords are persisted only as Argon2id PHC
   hashes and refresh tokens only as SHA-256 digests of an opaque value the client keeps."

Quote these verbatim in `Expected:`. Invariants 2, 4, 5, 8, 9 are **not** S1 criteria (2 is #177's, 8 and 9
are #178's, 4 and 5 are unchanged by an HTTP probe).

**2.5 What does "upload abuse" concretely mean for *this* repo?** It is these six routes and nothing else —
the archive of the contract, read at the audited revision:

| Route | Methods | Abuse it invites |
| --- | --- | --- |
| `/test_cases/{id}/attachments` | POST | oversized part, attacker-named part, wrong part name, multiple parts, no `file` part |
| `/test_cases/{id}/attachments/{filename}` | GET, DELETE | traversal through `{filename}`; re-reading stored attacker content |
| `/test_cases/{id}/steps/{step_index}/attachments` | GET, POST | the same, plus a listing that the case collection does not expose |
| `/test_cases/{id}/steps/{step_index}/attachments/{filename}` | DELETE | traversal through `{filename}` |
| `/test_runs/{id}/import/junit` | POST | raw XML body: parser abuse, entity/DOCTYPE, oversize, non-XML |
| `/test_runs/{id}/import/json` | POST | structured body imported wholesale: schema, depth, oversize |

The size facts, verified at the design revision and to be re-verified at the audited one
(`grep -n "MAX_ATTACHMENT_BYTES\|MAX_BODY_BYTES" -r src/`):

- `src/domain/mod.rs:29` — `pub const MAX_ATTACHMENT_BYTES: usize = 50 * 1024 * 1024;`
- `src/api/mod.rs:48` — `pub const MAX_BODY_BYTES: usize = MAX_ATTACHMENT_BYTES;`, applied as
  `RequestBodyLimitLayer::new(MAX_BODY_BYTES)` at `src/api/mod.rs:202`.
- The per-part checks are `if contents.len() > MAX_ATTACHMENT_BYTES` (strict `>`), at `src/api/cases.rs:97`
  and `:168` and again in `src/domain/service.rs:889` and `:944`. **Exactly 50 MiB passes the check.**
- The stored name is server-chosen with the client's name appended:
  `format!("{}-{}", unique_suffix(), original_name)` at `src/domain/service.rs:893` and `:949`.
- The multipart extractor is at `src/api/cases.rs:81` (`:151` for the step route).
- `mime_type(filename)` is `src/domain/mod.rs:125`; the attachment entry is
  `{"filename": …, "mimeType": mime_type(&filename)}`.

Consequence to measure, not assume: because `MAX_BODY_BYTES == MAX_ATTACHMENT_BYTES` and multipart framing
adds boundary and header bytes, a 50 MiB *part* cannot traverse a request whose whole body is capped at
50 MiB. The effective ceiling is therefore slightly below the documented constant. Measure both refusal
paths — the layer's 413 and the handler's 413 `payload_too_large` — and record the exact status and body of
each. A different body between the two paths is not automatically a defect; a 500, a hang, or an
unbounded allocation is.

**2.6 Which API-side attachment file is an attacker allowed to read back?** Any file the caller uploaded or
can reach through the case's project. There is no requirement that stored attachment content be inert: the
requirement is that it lands inside the case's own directory and that the response headers do not let a
browser treat it as a document of the API's origin. Both are measured (`S1-9`).

**2.7 Does turning authentication off make every probe moot?** No. The shipped default is
`TUCANO_AUTH_REQUIRED` off, and probes run on **both** arms. A probe that finds the guard absent on arm A
and present on arm B is recorded as the *documented* behaviour of an opt-in control — that fact is the
basis of the S3 exposure finding, and belongs to #178. A probe that finds an *authorization* defect on arm B
is #176's.

### 3. Sub-tasks

Each sub-task states the probe, the command, the control that must hold, and the evidence it produces.
`<n>` in `F-176-<n>` is assigned as findings are written, in severity order.

---

**S1-1 — Enumerate the surface and close the contract diff** *(must run first; produces §3 of the report)*

- **Probe.** Run the script in 2.1 at `$REV`. Enumerate the contract from `openapi.json` **and** from the
  served Swagger UI (`GET $A/openapi.json`, `GET $A/api-docs`) so a served document that differs from the
  file in the repository is caught.
- **Command.** The 2.1 script, plus:
  `curl -s "$A/openapi.json" | sha256sum` and `sha256sum "$AUDIT_ROOT/openapi.json"` — equal hashes.
- **Expected (control holds).** 49 paths / 71 operations; the public set is exactly the five operations;
  the router-vs-contract diff contains only the six declared entries and nothing unserved; the served
  document equals the shipped document byte for byte.
- **Evidence.** The enumeration table in report §3 and the pass entries for
  `tests/service.rs::openapi_document_matches_the_registered_routes` and the byte-equality check. **Not a
  finding.** If any count has moved since this design, record the new number; do not carry 49/71 blindly.

---

**S1-2 — Classify every route public or guarded, naming its guard** *(the task's DoD)*

- **Probe.** For all 71 operations, follow the handler into `src/api/*.rs`, record which guard it calls (or
  that it has none), and confirm the set of no-guard operations equals the five public operations.
- **Command.**
  ```bash
  grep -n "guard_\|require_admin\|require_run\|filter_list" src/api/*.rs
  grep -n "security: \[\]" -A2 -B8 openapi.json | grep -n "operationId\|/"
  ```
- **Expected (control holds).** The five `security: []` operations are the only ones without a guard; every
  other operation reaches a guard **after** the `Principal` extractor has already refused an anonymous
  caller.
- **Evidence.** The classification table (route, method, public/guarded, guard name, `access.rs` line) in
  report §3. A route with neither `security: []` nor a guard is a finding (severity per §5).

---

**S1-3 — Anonymous probe of every guarded operation**

- **Probe.** With arm B up, call every documented operation with a syntactically valid body and **no**
  `Authorization` header; then repeat with a random 32-byte bearer token.
- **Command.**
  ```bash
  python3 - "$B" <<'PY'
  import json, sys, urllib.request
  base = sys.argv[1]; doc = json.load(open("openapi.json"))
  for p, item in doc["paths"].items():
      for m, op in item.items():
          if not isinstance(op, dict) or op.get("security") == []: continue
          url = base + p.replace("{id}", "x").replace("{case_id}", "x") \
                    .replace("{step_index}", "0").replace("{filename}", "x") \
                    .replace("{version}", "1")
          path = url.replace("{", "").replace("}", "")
          req = urllib.request.Request(path, method=m.upper(),
                                       data=b"{}" if m in ("post","put","patch") else None)
          try:
              r = urllib.request.urlopen(req); print(m.upper(), p, r.status)
          except urllib.error.HTTPError as e:
              print(m.upper(), p, e.code, e.read()[:160], e.headers.get("WWW-Authenticate"))
  PY
  ```
- **Expected (control holds).** `401` with `missing_token`, then `401` with `invalid_token`; every response
  carries `WWW-Authenticate: Bearer realm="Tucano Test API"`; no response body contains data.
- **Evidence.** Pass entry crediting `tests/auth.rs::every_guarded_operation_refuses_an_anonymous_caller`
  and `::a_token_this_deployment_did_not_mint_is_refused`. Any 200, 500, or data leak is a finding.

---

**S1-4 — Confirm the public surface needs no caller**

- **Probe.** Call the five public operations with no token; then call `/auth/me`, `/auth/logout`, and one
  guarded read to confirm they are *not* public.
- **Expected (control holds).** `/health` → 200 `{"status":"ok","storage":"filesystem"}`; `/openapi.json`
  and `/api-docs` → 200; `/auth/login` and `/auth/refresh` → 400/401 on bad input, never 500; the other
  three → 401.
- **Evidence.** Pass entry crediting `tests/auth.rs::the_public_endpoints_need_no_caller`; the
  `/openapi.json` public-by-design fact is also a §6 calibration re-confirmation (the contract is public by
  design, so its exposure is not a finding).

---

**S1-5 — Token lifecycle: minting, spending, revoking, forging**

- **Probes and expected results.**
  1. **Sign in** as the bootstrap account on arm B → 200 with `accessToken`, `refreshToken`, `tokenType`,
     `expiresIn`. *(pass)*
  2. **Tamper** one base64 character of the access token → 401 `invalid_token`. *(pass)*
  3. **Forge** `{"alg":"none"}` / a token signed with a different secret → 401 `invalid_token`. *(pass;
     baseline `a_token_this_deployment_did_not_mint_is_refused`)*
  4. **Expire**: run a third arm with `TUCANO_ACCESS_TOKEN_TTL=1` (optional, only if time allows) and reuse
     the token after 2 s → 401 `token_expired`. *(pass)*
  5. **Spend a refresh token twice** → the second exchange is 401 `invalid_refresh_token`. *(pass;
     baseline, invariant 7's rotation claim)*
  6. **Two concurrent exchanges of the same refresh token** (`xargs -P2`) → **exactly one** 200. Two 200s
     is a rotation race and a finding (replay of a spent token).
  7. **Logout then reuse the refresh token** → 401. *(pass)*
  8. **Logout then reuse the access token** → expected 200, because `authentication-decision.md` records
     that stateless tokens are not revoked instantly and the revocation window is bounded by
     `TUCANO_ACCESS_TOKEN_TTL`. Record as a pass entry **with the document cited**. It becomes a finding
     only if a document promises immediate access-token revocation, or if the default TTL is materially
     longer than the documented 15 minutes at the audited revision.
- **Command.** `curl` chains against `$B`; keep each request and response in the report.
- **Evidence.** Pass entries per step, or findings for steps 6 and 8.

---

**S1-6 — The role matrix across projects (authorization, not authentication)**

- **Probe.** Sign in as the seeded viewer — `owner` of `checkout.json`, **no grant** on `payments.json` —
  and address every operation that names `payments.json` or a resource inside it, by identifier; then
  repeat as an editor and as an owner of some *other* project. Read *and* write.
- **Command.** `POST $B/auth/login` for the viewer, then for each of
  `GET /projects/payments.json`, `GET /test_suites/*`, `GET /test_cases/*`, `GET /test_runs/*`,
  `GET /milestones/*`, `GET /configurations/*`, `GET /results*`, `GET /reports/*` scoped to a
  `payments.json` resource, and the matching PUT/DELETE, record status and body.
- **Expected (control holds).** `403` or `404` — never 200 and never data. The threat model's *Known
  limitations* records that "Authorization is decided before existence", so a `403` for a nonexistent
  identifier is **documented behaviour, not a finding**; what is a finding is a `200`, a body carrying the
  other project's content, or a `500`.
- **Evidence.** The matrix (route × role × outcome) in report §3; pass entries crediting
  `tests/auth.rs::a_viewer_reads_the_projects_it_reaches_and_cannot_write`,
  `::an_editor_writes_content_but_not_projects_or_milestones`,
  `::an_owner_administers_its_project_but_not_the_installation`,
  `::a_system_administrator_reaches_everything`, `::a_caller_with_no_grant_sees_nothing`.

---

**S1-7 — Cross-project references: the run scope and the dual-home identifier**

- **Probe.**
  1. The seed copies `TC-LOGIN-1` and `TC-ORDERS-1` into `payments.json` and places
     `portable.checkout.json` in both projects, so those three identifiers each have **two homes**.
     Address the global case routes (`GET /test_cases/TC-LOGIN-1`, `GET /test_cases/TC-LOGIN-1/history`,
     `GET /results/TC-LOGIN-1/defects`) and the global suite routes (`GET /test_suites/portable.checkout.json`,
     `GET /test_suites/portable.checkout.json/test_cases`) as the `checkout.json`-only viewer and as the admin.
  2. A run's `projects` array decides which projects it reaches (`guard_update` → editor over the home *and*
     every project in the array). As an editor of `checkout.json`, update `nightly.json`'s `projects` array
     to name `payments.json`, then read the run and any embedded snapshots of `payments.json` cases.
  3. Compose across projects: copy a `checkout.json` case into `payments.json` and move one the other way,
     as a caller with a role in only one of the two. The seed's own step 11 already did both, so use
     `TC-CART-1` (single home in `smoke.checkout.json`) for the copy and any case named here for the move.
- **Expected (control holds).** `reachable_projects` answers `Conflict` when two projects hold one
  identifier and `NotFound` when none does, and every bare-identifier route above answers `409` rather than
  picking a home; a caller may not use a run to read another project's snapshots;
  composition requires a role in both source and target. The known limitation that "Run scope can be
  narrowed by the caller that holds the run" is **accepted and is not a finding** — record it as a
  re-confirmation, and note explicitly that raising a run's privileges is not possible, so it is denial of
  access rather than escalation.
- **Evidence.** Outcomes per probe; pass entries crediting `tests/auth.rs::composing_a_case_checks_the_source_only_when_it_exists`
  and `::the_caller_is_told_which_projects_it_can_reach`.

---

**S1-8 — Traversal through every identifier and filename**

- **Probe.** Send each hostile value through **every** path parameter of every documented operation:
  `{id}` (projects, suites, cases, runs, configurations), `{case_id}`, `{step_index}`, `{version}`,
  `{filename}` (case attachments and step attachments). Hostile values: `../`, `..%2F..%2Fetc%2Fpasswd.json`,
  `..%5C..%5C`, `%2e%2e%2f`, an absolute path (`/etc/passwd`), a Windows absolute path, a NUL
  (`%00`), a newline, a reserved name (`test_runs`, `milestones`, `configurations`, `.tucano.lock`,
  `CON`, `nul`), the empty string, `.`, `..`, `a/b`, `a\\b`, a 4 KiB value, and a unicode value with a
  combining mark and a normalization-confusable identifier.
- **Command.**
  ```bash
  for v in '..' '..%2Fetc%2Fpasswd' '%2e%2e%2f' '/etc/passwd' '%00' 'a/b' 'a%5Cb' \
           'test_runs' 'milestones' 'configurations' '.tucano.lock' '.' '..' '' "$(python3 -c 'print("a"*4096)')"; do
    for p in "/test_cases/$v" "/test_cases/$v/attachments" \
             "/test_cases/$v/steps/0/attachments/x" "/test_suites/$v" "/projects/$v" \
             "/test_runs/$v" "/milestones/$v" "/configurations/$v"; do
      printf '%s -> ' "$p"; curl -s -o /dev/null -w '%{http_code}\n' "$B${p}"
    done
  done
  # and the write side, then prove the volume gained nothing:
  docker exec audit-b find /data -newermt '-2 minutes' -printf '%p\n'
  docker exec audit-b find /data -name '*.json' -newermt '-2 minutes' | wc -l
  ```
- **Expected (control holds).** 400 (`invalid_…`) or 404; **never 500** and never a write outside the
  case's own directory. `find /data -newermt` must show nothing new from the read probes; the write probes
  must produce at most a refusal and no file.
- **Evidence.** Pass entries crediting `tests/security_tests.rs`'s five traversal tests and
  `src/storage/layout.rs::hostile_components_are_rejected`, `::paths_outside_the_root_are_rejected`,
  `::a_document_identifier_is_validated_before_any_path_is_built`. A 500 or a stray file is a finding
  (`F-176-n`, trust boundary *Resource ID or filename to filesystem*).

---

**S1-9 — Upload abuse on the six routes of 2.5**

- **Probes and expected results.**
  1. **Boundary:** a multipart upload whose part is exactly `50 * 1024 * 1024` bytes, and one a single byte
     larger, against `POST /test_cases/TC-LOGIN-1/attachments` on arm B.
     Expected: the over-limit one is refused with 413 `payload_too_large` and **no file is created**; the
     exactly-50-MiB one is accepted *if* the whole request body also fits `MAX_BODY_BYTES` — measure which
     of the two refusals wins and record the effective ceiling (see 2.5). A 500, a wedge, or an
     unbounded-memory container kill is a finding (*Attachment upload to storage*).
  2. **Malformed multipart:** a request with no part, a part named `attachment` instead of `file`, two
     `file` parts, and a body that is not multipart at all.
     Expected: 400 with a stable code — **never 500**. The extractor is `src/api/cases.rs:81`.
  3. **Attacker-named filename:** parts named `../../../../etc/cron.d/x`, `/etc/passwd`, `.`, `..`,
     `a/b`, a 300-character name, a name with a NUL, and a UTF-8 name with a combining mark, uploaded to
     both the case and the step route.
     Expected: stored where the case's own attachments live, under a server-chosen prefix
     (`{unique_suffix}-{original_name}`, `src/domain/service.rs:893`/`:949`); the read-back
     `GET …/attachments/{filename}` succeeds for the returned name and **fails for the traversal form**;
     `find /data -newermt` shows nothing outside that case's directory.
  4. **Re-reading stored content:** upload `payload.html`, `payload.svg`, `payload.js`, and `payload.txt`,
     then `GET /test_cases/{id}/attachments/{stored-name}` and record `Content-Type`,
     `Content-Disposition`, `X-Content-Type-Options`, and any CSP. Expected: the response does not let a
     browser execute the content as a document of the API's origin. If the content is served with a
     sniffable type (`text/html`, `image/svg+xml`), no `nosniff`, and no `attachment` disposition, that is a
     finding — and its severity must argue the origin: the API serves no cookies and Swagger UI is on the
     same origin, so state explicitly what an attacker gains. If it is served as an opaque type or with
     `Content-Disposition: attachment`, it is a pass entry.
  5. **Overwrite:** upload the same client filename twice to the same case; then delete one.
     Expected: two distinct stored names, both listed (on the step route) or both retrievable; deleting one
     leaves the other and removes only its own metadata entry.
  6. **Other bodies:** `POST /test_runs/{id}/import/junit` with a DOCTYPE-bearing XML, an external entity
     reference, a 100 MiB XML, a non-XML body, and a deeply nested one; and `/import/json` with the same
     shapes. Expected: refusal with a stable code, no external fetch (watch the container's network with
     `docker exec audit-b ss -tn`), no CPU/memory blow-up.
- **Evidence.** A table of probe → status → body → volume state, in report §3, with findings for any 500,
  stray file, accepted oversize, or executable-content read-back. Credit
  `tests/attachments.rs`/`tests/validation.rs` where they already cover a probe.

---

**S1-10 — The error envelope, headers, and log disclosure (invariants 3, 6)**

- **Probe.**
  1. Force every `DomainError` variant and record status, `error.code`, `error.message`, and whether
     `requestId` is present and well-formed: `NotFound`, `InvalidRequest`, `Conflict` (409),
     `PayloadTooLarge` (413 `payload_too_large`), `Internal` (500 `storage_error`), `Storage` (500
     `storage_error`), `Unauthenticated` (401), `Forbidden` (403).
  2. **Force a 500 with a path in it:** corrupt a stored document in the throwaway volume (truncate
     `checkout.json`'s project document, or replace a case document with invalid UTF-8), then read it.
     `DomainError::Internal(message)` echoes its message into `error.message`; if that message is raw IO
     text it will carry an internal path — a direct violation of invariant 6 and of *Data disclosure*.
     **This is the highest-probability S1 finding and must be reproduced or refuted.** The matching S2
     probe is S2-6; write the finding **once**, under whichever boundary it belongs to, and cross-reference
     the other rather than duplicating.
  3. `X-Request-Id`: send a hostile value (a newline, a 4 KiB string, `../../x`) and observe whether it is
     echoed verbatim and whether it reaches the container's stdout (`docker logs audit-b`). Reflection is a
     finding only if it is unescaped into a header or a log line in a way that forges an entry; otherwise
     record it as a pass entry that a request ID is generated when absent.
  4. Confirm the 401 challenge shape and that no response carries a stack trace, an internal path, an OS
     error string, or file contents.
- **Expected (control holds).** "Client-visible errors use stable codes and safe messages with request IDs";
  "Security-sensitive events exclude credentials, tokens, raw payloads, attachment contents, and internal
  paths."
- **Evidence.** The variant table, `docker logs` excerpts, and the finding for step 2 if it reproduces.

---

**S1-11 — Bounded denial-of-service probes (must stay bounded)**

- **Probe.** Deeply nested JSON (10 000 levels) to a create; a multipart with 1 000 parts; a list query
  with a huge `limit`/`offset`/filter; a body streamed at 1 KiB/s with `curl --limit-rate`; 64 concurrent
  requests to one create; 64 concurrent duplicate operations on one case.
- **Expected (control holds).** Every probe answers within the container's limits, or is refused; the
  container survives; `docker stats --no-stream audit-b` shows memory inside the 512 MiB Compose limit; no
  partial document is published. The threat model's *Denial of service* requirement is bounded resources,
  not absolute resistance — an unbounded allocation or an unkillable request is a finding, slow-but-bounded
  behaviour is not.
- **Evidence.** Timings, statuses, and a `docker stats` line; pass entry where the bound holds.

---

**S1-12 — Sign-in: enumeration, timing, and throughput**

- **Probe.** Sign in with an existing username and a wrong password, and with a nonexistent username, on
  arm B; repeat each 100 times and measure wall-clock latency of the whole request. Then try 200 sign-ins in
  parallel with one password and record how many succeed before any throttling appears.
- **Expected (control holds).** The bodies are identical
  (`tests/auth.rs::the_two_ways_a_sign_in_fails_are_one_answer`). Timing equality is **not** covered by the
  baseline: measure it. A large, stable difference (a factor, not noise) supports a user-enumeration
  finding; report it with the distributions. No rate limit is documented anywhere, so absent throttling is
  not a violation of a stated control — record it as an Info observation at most, and only raise it if the
  absence combines with something else (for example unbounded Argon2id memory) into a resource finding.
- **Evidence.** Both distributions, and the parallel-throughput result.

---

**S1-13 — Boundary 9: the service offers no filesystem path to a client**

- **Probe.** Address every route prefix and method combination not in the contract (`/data`, `/auth`,
  `/auth/accounts`, `/data/auth/accounts.json`, `/../data/…`, `/_`, a `?file=`-style query) on both arms,
  and confirm there is no directory listing and no arbitrary-file read.
- **Expected (control holds).** 404/405 for everything; `GET /auth/me` and the four session routes are the
  only `/auth*` surface; nothing under `TUCANO_DATA_DIR/auth/` is retrievable.
- **Evidence.** Pass entry naming the enumeration command. Any read is a finding (*GUI to storage* / *HTTP
  client to service*).

---

**S1-14 — Auth-off parity**

- **Probe.** Re-run S1-3, S1-6, S1-8, and S1-9 against arm A (`TUCANO_AUTH_REQUIRED` unset).
- **Expected (control holds).** Every guard returns before it resolves anything, so every route is
  reachable with no caller — this is the **shipped default**, not a defect in #176. Record the observation
  and hand it to #178 (S3-3), which owns the deployment-exposure finding. Do not write an S1 finding whose
  only content is "authentication is optional".
- **Evidence.** A short parity table; a cross-reference to S3-3.

---

### 4. Merge base and report PR for #176

- **Merge base.** Record `git rev-parse origin/main` in full, the `Cargo.lock` SHA-256, and the `openapi.json`
  SHA-256 at the same revision, in report §1. The audit runs against that commit and no other; the report
  names it, and a moved `main` afterwards does not invalidate or extend the report. All commands in §3 ran
  from the worktree at that revision (`/tmp/audit-rev`, added detached at `$REV`).
- **Report.** `docs/security/audit-s1-http-surface.md`, `# Security Audit S1 — HTTP surface (Issue #176)`,
  epic `#166`, the S1 sentence from `audit-scope.md` §2, and "It carries findings only; nothing here is
  fixed." Then §1–§7 exactly as the shared contract's §5 specifies. Findings are `F-176-<n>`.
- **PR.** `gh pr create --base main --title "docs(security): S1 HTTP surface audit report (#176)"` linking
  `#176`, `#166`, and this design; assign `ECiurleo`; add the report row to the README documentation table;
  do not merge.
- **Deliverable-specific extras the DoD requires.** The route classification table (S1-2) is part of the
  report, not an appendix to this design. The DoD's "extend `tests/security_tests.rs` and `tests/auth.rs`
  where a check is missing" is a **recommendation in the report's `Suggested fix`/pass-entry text** — an
  audit PR writes no test, because fixes are out of scope for every audit task. Record each missing check as
  a pass-entry gap or a finding, and name the test that would close it.

### 5. Unresolved for #176, recorded rather than guessed

- **Whether an attacker-influenced `Content-Type` on an attachment is a finding on its own** (S1-9.4). It
  becomes one only when a browser can be made to treat the stored bytes as a document of the API's origin;
  the executor must attempt that argument with the real response headers and record either a finding or a
  pass entry. This design deliberately does not pre-commit a severity.
- **The effective attachment ceiling** (S1-9.1) follows from the interaction between two constants at the
  audited revision; it is measurable in one probe and this design does not predict which refusal wins.
- **The timing-enumeration threshold** (S1-12) — no document states an acceptable difference, so the finding
  is written only if the measured distributions separate cleanly, and the number and the distributions go
  into the finding rather than a threshold.
- **`/auth/logout` does not revoke the access token** (S1-5.8). Whether that is a finding depends on
  whether any document promises otherwise; `authentication-decision.md` currently does not. The executor
  must check every document at the audited revision, not just that one.

---

## #177 — S2: Storage and filesystem

### 1. Scope

The audit attacks the persistence layer across the boundary the HTTP surface does not touch: it plants
files and links inside the configured data root, addresses identifiers that could collide with the tree's
own reserved names, kills the service mid-write, runs two processes against one data directory, reads the
file permissions the service applies, corrupts stored documents, and inspects whether the API's own error
paths disclose internal paths or file contents. In `audit-scope.md` §2's words, S2 is the audit of
**storage and filesystem invariants**. Concretely: `src/storage/layout.rs` (path construction,
confinement, permissions), `src/storage/fs.rs` (atomic writes, the advisory lock, attachment and revision
writes), `src/auth/store.rs` (the auth store's files), and the attachment/revision paths in
`src/domain/service.rs`. The acceptance criteria are the threat model's invariants **1, 2, 6, 7, and 9**
and trust boundaries **3 (Resource ID or filename to filesystem), 4 (Service → stored JSON),
5 (Attachment upload → storage), 6 (Configuration file → service startup), 7 (Configuration key → service
startup), and 8 (Service logs and audit events)**. Boundary **1** belongs to #176; the *deployment* half of
6/7 belongs to #178, and the overlap is resolved in S2-15.

### 2. Ambiguities, resolved

**2.1 Where does the audited filesystem live?** In the throwaway target's private data directory
(`$AUDIT_DATA`, bind-mounted at `/data` in both arms). The audit may **read and write any corner of it** —
that is the declared adversary — and may `docker exec` into its own containers to plant fixtures, inspect
modes, and watch the tree. It may never write to, exec into, or inspect the operator's instance's volume.

**2.2 Which tree is the contract?** The real-home tree of the project philosophy: a project is a folder; a
suite is a folder inside a project; a case is a folder inside a project or a suite holding its JSON
document, steps, and attachments; a project also holds the reserved collections `test_runs`, `milestones`,
`configurations` (`src/storage/layout.rs:23`,
`RESERVED_PROJECT_CHILDREN = ["test_runs", "milestones", "configurations"]`). `Resource::ALL`, `ROOT_DIRS`,
`folder_name`, `folder_wire_id`, `validate_document_id` (`:236`), `validate_component` (`:367`),
`ensure_within` (`:391`), and `resolve_existing_prefix` (`:415`) are the enforcement points — capture their
current lines with `grep -n "fn validate_document_id\|fn validate_component\|fn ensure_within\|fn resolve_existing_prefix\|RESERVED_PROJECT_CHILDREN\|fn set_private_permissions" src/storage/layout.rs`.

**2.3 What is the atomicity claim, exactly?** "Persistence publishes complete documents atomically; failed
writes do not replace valid data." The mechanism, at the design revision:
`src/storage/fs.rs:163` creates `directory.join(format!(".tucano-{}.tmp", unique_suffix()))`,
`:173` calls `file.sync_all()?`, `:174` calls `fs::rename(&temporary, destination)`. The same pattern guards
attachment writes (`:606`) and revision writes (`:711`). **This design treats invariant 2 as satisfied by
inspection and expects a pass entry** — the audit's job is to try to falsify it (S2-5) and to confirm the
temp name is never addressable through any route. Note explicitly that nothing in the invariants claims
**power-loss** durability: the missing parent-directory `fsync` is therefore recorded as an observation,
never a finding, and this design pre-commits to that so the executor does not have to decide it.

**2.4 What are the permission expectations, and which document is normative?** `AGENTS.md` says "Apply
restrictive file permissions and explicit overwrite behaviour under `Storage Security`", and boundary 5's
required control names "safe permissions". The implementation calls
`set_private_permissions` at four sites: `src/storage/fs.rs:168` (atomic document write), `:604`
(attachment), `:709` (revision), and `src/auth/store.rs:405` (the auth store). The function is at
`src/storage/layout.rs:451` and sets mode **`0o666`** at `:455`; the repository's own test asserts
`assert_eq!(mode, 0o666)` (`src/storage/fs.rs:2286`). So the mode is deliberate and tested, and it
contradicts the written rule. **This is the highest-probability S2 finding.** Re-verify with
`grep -n "0o666\|from_mode\|set_private_permissions" src/storage/layout.rs src/storage/fs.rs src/auth/store.rs`
and score it per §5, stating both adjustments in the finding (see S2-2 for the recommended reading).

**2.5 What counts as "concurrent" for invariant 2?** Two cases: (a) many HTTP writers to one document inside
one replica; (b) two replicas against the **same** data directory. The lock is
`self.root.join(".tucano.lock")` opened with an exclusive lock at `src/storage/fs.rs:36` and released at
eight explicit `unlock` sites (`fs.rs:523/541`, `546/552`, `576/578`, `590/613`, `624/640`, `674/679`,
`692/718`, `729/740`). The acquisition is a `File` guard and the release is a call, not RAII — so a failed
operation must not wedge the volume. That is tested empirically (S2-9); the audit does not reason about it
from the source alone. The deployment's reliance on the host's advisory locks is expressly *recorded* by
§3, and **host reliance may not be written as a finding** — at most an Info observation.

**2.6 What is the overwrite contract per operation?** Recorded in S2-11 as an explicit table. Creation with
an existing identifier is a conflict; updates replace; deletes remove the folder; duplication creates a new
entity and leaves the source untouched; imports are the one operation whose conflict behaviour is not
obviously pinned, and it is measured rather than assumed.

**2.7 Is a stray file in the tree a defect?** The layout tests prove nothing outside the root is reachable
and no file type other than `.json` is accepted in the project tree
(`src/storage/layout.rs::only_projects_are_stored_below_the_data_root`,
`::a_symlinked_project_folder_that_escapes_the_root_is_rejected`,
`::a_symlinked_collection_directory_that_escapes_the_root_is_rejected`,
`::the_project_tree_is_not_readable_by_a_non_json_file`-family assertions). A leftover
`.tucano-<suffix>.tmp` is therefore expected to be inert: refuse-worthy if addressable, irrelevant if not.
**Resolved: it is a pass entry, and it becomes a finding only if a route can name it.**

### 3. Sub-tasks

---

**S2-1 — Enumerate the persistence surface before probing** *(produces report §3)*

- **Probe.** List every path that writes or renames under the root, and every place a path is built from
  request data.
- **Command.**
  ```bash
  grep -n "fs::rename\|fs::write\|File::create\|OpenOptions\|create_dir\|remove_dir\|remove_file\|sync_all" \
    src/storage/fs.rs src/storage/layout.rs src/auth/store.rs
  grep -n "attachment_path\|step_attachment_path\|revision_dir\|project_document_path\|project_collection_dir" \
    src/storage/layout.rs
  ```
- **Expected.** A closed table: every mutating operation maps to one call site; no path is built outside
  `layout.rs`.
- **Evidence.** The table in report §3. A path built anywhere else is a finding.

---

**S2-2 — File permissions on every created file (candidate finding)**

- **Probe.** After seeding on arm B, walk the whole data directory and record mode, owner, and type of
  every file the service created — documents, markers, attachments, revisions, the lock file, and the auth
  store.
- **Command.**
  ```bash
  docker exec audit-b find /data -printf '%M %u:%g %s %p\n' | sort | head -100
  docker exec audit-b stat -c '%a %n' /data/auth/* /data/Projects/*/.tucano.json 2>/dev/null
  docker exec audit-b ls -la /data /data/auth
  ```
- **Expected (control holds).** Document mode is restrictive (owner-only or owner+group), per `AGENTS.md`
  "Apply restrictive file permissions"; the auth store in particular is not world-readable.
- **Expected (probable actual, to be reproduced).** `0o666` on documents, attachments, revisions, and the
  auth store — every local user on the host can read and write the installation's data and its auth store,
  matching `src/storage/layout.rs:451`/`:455` and the repository's own assertion at
  `src/storage/fs.rs:2286`.
- **Severity, recommended reading.** Impact axis is Moderate at most (the auth store holds Argon2id PHC
  hashes and refresh-token SHA-256 digests, which invariant 7 permits; nothing readable is a usable
  credential), so `Severe` is wrong. Exploitability needs a local position or a shared host account —
  either way it is a condition the threat model's §3 rules exclude as the adversary's own capability — so
  the reading is *Difficult × Moderate = **Low***, escalated **only** if the executor can show the exposed
  data is a usable secret (it is not) or that the default deployment publishes the volume to another
  principal. State both adjustments in the finding. Do not claim remote reachability.
- **Evidence.** `F-177-1` — `In scope: S2 — Attachment upload to storage` and boundary 3, `Where:` the
  representative site `src/storage/layout.rs:451` plus the enumeration
  `src/storage/fs.rs:168`, `:604`, `:709`, `src/auth/store.rs:405`, `Expected:` quoting `AGENTS.md`'s
  restrictive-permissions rule and boundary 5's "safe permissions", `Duplicates / prerequisites: none`.
  Every one of the four call sites is named so the class is closed rather than sampled.

---

**S2-3 — Symlink and hardlink escape fixtures**

- **Probe.** Plant each fixture in the throwaway data directory and address it through the API:
  1. a symlinked project folder pointing at `/etc` (`ln -s /etc /data/Projects/evil.json`);
  2. a symlinked collection directory (`/data/Projects/checkout.json/test_runs` → `/tmp`);
  3. a symlinked case folder and a symlinked case document;
  4. a symlinked attachment file inside a real case's attachments directory, pointing outside the root;
  5. a symlink *inside* the root (case → another project's case) to test confinement rather than escape;
  6. a **hardlink** from an outside file into the attachments directory and into a project folder (a
     hardlink cannot be detected by path inspection, so this tests whether the control is confinement or
     type-checking).
- **Command.**
  ```bash
  docker exec audit-b sh -lc 'ln -s /etc /data/Projects/evil.json;
    ln -s /tmp /data/Projects/checkout.json/test_runs;
    ln -s /tmp/escape.json /data/Projects/checkout.json/suites/smoke.checkout.json/cases/TC-LOGIN-1/attachments/escape.json'
  curl -s -o /dev/null -w '%{http_code}\n' "$B/test_cases/TC-LOGIN-1/attachments/escape.json"
  curl -s -o /dev/null -w '%{http_code}\n' "$B/projects/evil.json"
  docker exec audit-b sh -lc 'ln /etc/hostname /data/Projects/checkout.json/hard.json; cat /data/Projects/checkout.json/hard.json'
  ```
- **Expected (control holds).** Refusal for every fixture; `ensure_within` / `resolve_existing_prefix`
  canonicalise before use; `find /data` never resolves outside the root; the hardlink cannot be turned into
  a read of an outside file **through the API**. Credit
  `src/storage/layout.rs::a_symlink_that_escapes_the_root_is_rejected`,
  `::a_symlinked_collection_directory_that_escapes_the_root_is_rejected`,
  `::a_symlinked_project_folder_that_escapes_the_root_is_rejected`,
  and `tests/security_tests.rs::symlink_tests::test_rejects_symlink_escape` as pass entries.
- **If the control fails.** Note that the fixture requires write access to the volume, which §3 excludes as
  a precondition — so the finding de-escalates one level and must say so. Record it anyway; a bypass is a
  bypass.
- **Evidence.** A per-fixture table (fixture → route → status → what the filesystem shows), plus the
  finding or the pass entries.

---

**S2-4 — Reserved names, separators, and degenerate identifiers**

- **Probe.** Create projects, suites, and cases whose identifiers are `test_runs`, `milestones`,
  `configurations`, `.tucano.lock`, `.tucano-1700000000000000000.tmp`, `CON`, `nul`, `aux`, `.`, `..`, `a/b`,
  `a\\b`, `a%2Fb`, the empty string, whitespace only, a 4 KiB value, and a unicode value with a combining
  mark; then read the tree to see whether a reserved collection was shadowed or a document was written
  outside its case folder.
- **Command.** `curl` POSTs to the project-scoped create routes, then
  `docker exec audit-b find /data -newermt '-5 minutes' -printf '%y %p\n'`.
- **Expected (control holds).** Refusal (400 `invalid_…` or 409) for every degenerate value; no directory
  named `test_runs`/`milestones`/`configurations` created below a project except by the service itself;
  no file outside `Projects/<project>/…`.
- **Evidence.** Pass entries crediting
  `src/storage/layout.rs::a_document_identifier_is_validated_before_any_path_is_built`,
  `::hostile_components_are_rejected`, `::a_project_reserves_the_names_of_its_collections`,
  `::case_folders_keep_their_identifier_verbatim`, and `tests/security_tests.rs`'s traversal tests.

---

**S2-5 — Atomicity and durability: kill the writer mid-write**

- **Probe.** Run a loop that PUTs a large document (a few MiB of steps) to one case while killing the
  container with `SIGKILL` mid-flight; restart; read the document and validate the JSON; compare against the
  two candidate contents (old complete, new complete). Repeat at least ten times. Then inspect for leftover
  temp files and try to address one.
- **Command.**
  ```bash
  for i in $(seq 1 10); do
    ( curl -s -X PUT "$B/test_cases/TC-LOGIN-1" -H 'content-type: application/json' --data-binary @/tmp/big.json & \
      sleep 0.$((RANDOM % 5)); docker kill --signal=KILL audit-b >/dev/null; wait )
    docker start audit-b >/dev/null; sleep 1
    curl -s "$B/test_cases/TC-LOGIN-1" | python3 -c 'import json,sys; json.load(sys.stdin); print("valid")'
  done
  docker exec audit-b find /data -name '.tucano-*.tmp' -printf '%s %p\n'
  curl -s -o /dev/null -w '%{http_code}\n' "$B/test_cases/%2Etucano-1700000000000000000%2Etmp"
  ```
- **Expected (control holds).** Every read after every kill returns a **complete, parseable** document equal
  to one of the two submitted versions — never a truncated or mixed one. Leftover `.tucano-*.tmp` files are
  inert and unaddressable.
- **Evidence.** **Pass entry for invariant 2**, quoting the invariant, with the log of ten kills and the
  validity results. **Pre-committed non-finding:** the absence of a parent-directory `fsync` is recorded as
  an observation only.

---

**S2-6 — Corrupted and hostile stored documents**

- **Probe.** Corrupt stored documents in four ways in the auditor's own volume, then read them back:
  truncate a document mid-object; flip bytes to invalid UTF-8; replace a document with valid JSON of the
  wrong shape; replace a document with a 100 MiB blob. Before and after each read, hash the file.
- **Command.**
  ```bash
  docker exec audit-b sh -lc 'F=/data/Projects/checkout.json/test_cases/TC-LOGIN-1/test_case.json;
    cp "$F" /tmp/orig; head -c 200 "$F" > /tmp/t; mv /tmp/t "$F"; sha256sum "$F"'
  curl -s -w '\n%{http_code}\n' "$B/test_cases/TC-LOGIN-1"
  docker exec audit-b sha256sum /data/Projects/checkout.json/test_cases/TC-LOGIN-1/test_case.json
  ```
- **Expected (control holds).** A **safe storage error** (500 `storage_error` with a stable, non-disclosing
  message) and **the original file is preserved** — the corrupted bytes stay exactly as the auditor left
  them, unmodified and not silently replaced or deleted. The response body carries **no internal path, no
  stack trace, no raw OS error, and no file contents** (invariant 6, boundary 8).
- **Evidence.** Pass entry crediting `tests/security_tests.rs::malformed_json_tests` and the repository's
  corruption handling, plus the disclosure finding — written **once**, cross-referenced with S1-10.2 so the
  same defect is not counted twice.

---

**S2-7 — Concurrent writers inside one replica**

- **Probe.** 32 parallel PUTs of 32 distinct documents to one case, then read it back.
- **Command.**
  ```bash
  for i in $(seq 1 32); do curl -s -X PUT "$B/test_cases/TC-LOGIN-1" \
      -H 'content-type: application/json' --data-binary "{\"title\":\"v$i\"}" -o /dev/null & done; wait
  curl -s "$B/test_cases/TC-LOGIN-1" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("valid", d["title"])'
  ```
- **Expected (control holds).** The final document is valid and equals exactly one of the 32 submissions —
  no interleaving, no partial document. Loss of all but one write is correct last-writer-wins behaviour,
  not a finding.
- **Evidence.** Pass entry crediting `tests/security_tests.rs::data_integrity_tests::test_concurrent_writes_do_not_corrupt`.

---

**S2-8 — Two replicas against one data directory**

- **Probe.** Start a third container from the same image on the same `$AUDIT_DATA` (a different host port),
  then drive interleaved writes of the same document from both replicas and check the outcome, the lock
  file's behaviour, and whether either replica reports a conflict.
- **Command.** `docker run -d --name audit-c … -v "$AUDIT_DATA":/data …`, then a loop alternating PUTs to
  `$A` and `$C`.
- **Expected (control holds).** The lock serialises the writers; no torn document; a divergence window is
  permitted. §3 says the audit may "record where the deployment relies on the host holding (for example
  shared-storage advisory locks)" — so the reliance on `flock` **is recorded, not scored**. A torn or
  interleaved document is a finding; the fact that `flock` is host-local is an Info observation at most.
- **Evidence.** Pass entry or finding, plus the recording of the host-dependency, plus
  `docker exec audit-c stat -f -c %T /data` (the filesystem type the throwaway volume sits on) so the
  result is interpretable — an overlay/tmpfs result does not transfer to a real volume.

---

**S2-9 — Lock release on the failure path**

- **Probe.** Trigger a write that fails *after* the lock is acquired (for example a placement or rename
  whose destination is made unwritable, or a document whose ID passes HTTP validation but fails a later
  layout check), then immediately perform a normal successful write. Do it for a document write, an
  attachment write, and a revision write.
- **Command.** `docker exec audit-b sh -lc 'chmod 0555 /data/Projects/checkout.json'` → attempted write →
  restore mode → successful write with `curl --max-time 10`.
- **Expected (control holds).** The follow-up write succeeds: the failed path released the lock. The lock
  is acquired as a `File` (`src/storage/fs.rs:36`) and released by an explicit `unlock` at eight sites
  (`:523/541`, `:546/552`, `:576/578`, `:590/613`, `:624/640`, `:674/679`, `:692/718`, `:729/740`); an early
  `?` return drops the guard, closing the descriptor. So the expected outcome is release, and the probe
  confirms it rather than assuming it. A wedge (lock held for the process lifetime, or held across a
  `docker restart`) is a finding — *Moderate × Moderate* at most, and the finding must state whether the
  release is guaranteed by drop or merely accidental at the sites observed.
- **Evidence.** Pass entry, or `F-177-<n>` naming the exact call site whose early return leaked.

---

**S2-10 — Attachments and revisions: publication, orphans, and immutability**

- **Probe.**
  1. Attachments are written **in place** (`src/storage/fs.rs:604` — `sync_all()` without a temp+rename),
     unlike documents. Read one back in a tight loop while re-uploading a larger file over the same stored
     name, and look for a truncated body.
  2. Delete an attachment, then look for an orphan metadata entry and for an orphan file; and vice versa:
     remove a file under the attachments directory by hand and read the listing.
  3. `save_revision`/`revisions/v1.json`: attempt to write a revision through every route; hash the
     revision, perform several updates, and re-hash.
- **Expected (control holds).** A read of an attachment is either the complete old content or the complete
  new content; a delete leaves neither a dangling entry nor a served orphan; a revision is immutable and
  unreachable as a write target. Also note the asymmetry the contract exposes — the step collection has a
  `GET` listing while the case collection does not — and record whether it lets a caller observe an orphan.
- **Evidence.** A finding for a torn read (Low candidate, boundary 5) or a pass entry; pass entries for
  orphan handling and revision immutability.

---

**S2-11 — Overwrite behaviour for every mutating operation**

- **Probe.** For each of create, update, delete, duplicate, import (`/import/junit`, `/import/json`),
  compose copy, compose move, and removal, record the expected and actual outcome against an existing
  identifier and against a missing one; and check after each failure that no half-created folder, marker,
  or entry remains.
- **Command.** `curl` per operation, then
  `docker exec audit-b find /data -newermt '-1 minute' -printf '%y %p\n'` after each failure.
- **Expected (control holds).** Create over an existing identifier → 409 conflict, no overwrite; update →
  replaces completely; delete → removes the folder and its contents; duplicate → a new entity, source
  byte-identical (`sha256sum` before/after); import → conflict behaviour recorded (this is the open decision
  "Locking implementation and overwrite/conflict semantics" made concrete); compose copy leaves the source
  in place, compose move leaves exactly one home.
- **Evidence.** The table in report §3 plus findings for any partial write, silent overwrite, or
  source mutation. Credit `tests/cases.rs`, `tests/suites.rs`, `tests/runs.rs` where they cover the case.

---

**S2-12 — Error leakage on every failure path (invariant 6, the DoD item)**

- **Probe.** Enumerate the failure paths — every `DomainError` variant × each layer (identifier validation,
  not-found, conflict, authorization, storage IO, corrupted document, oversize, unsupported type) — and for
  each record the status, `error.code`, `error.message`, whether it names a setting or a path, and whether
  `docker logs` carries anything the response does not.
- **Expected (control holds).** A stable code and a safe message; no path, no OS error, no stack trace, no
  file contents, in the response **or the logs**.
- **Evidence.** The table, plus the disclosure finding if it reproduces (shared with S1-10.2 and S2-6).

---

**S2-13 — The root is the only area read or written (invariant 1)**

- **Probe.** Start with a full hash of the container's filesystem outside `/data` and `/tmp`, run the whole
  seeded workload, and diff.
- **Command.**
  ```bash
  docker exec audit-b sh -lc 'find / -xdev -not -path "/proc/*" -not -path "/sys/*" \
    -not -path "/data/*" -not -path "/tmp/*" -printf "%p %s\n" | sort' > /tmp/fs-before.txt
  # … run the S1 and S2 workloads …
  docker exec audit-b sh -lc '… same …' > /tmp/fs-after.txt
  diff /tmp/fs-before.txt /tmp/fs-after.txt
  docker diff audit-b | grep -v '^C /data\|^C /tmp'
  ```
- **Expected (control holds).** No change outside a tmpfs or a purely ephemeral path; nothing written to
  the image layer. `docker diff` shows changes only under `/data` (the mount) and `/tmp` (the tmpfs).
- **Evidence.** Pass entry quoting invariant 1, sharing its evidence with S3-10.

---

**S2-14 — The service exposes no filesystem path, and `auth/` is unreachable**

- **Probe.** Confirm that no route lists, reads, or writes anything under `TUCANO_DATA_DIR/auth/`, and that
  no listing route lets a caller enumerate the tree.
- **Expected (control holds).** Only `GET /auth/me` and the session routes touch `/auth*`; the auth store
  files are never served.
- **Evidence.** Pass entry, cross-referenced with S1-13 (one probe, two scopes) and with S2-2's permission
  facts.

---

**S2-15 — The configuration-file boundary, storage side** *(bounded; the rest belongs to #178)*

- **Probe.** With `TUCANO_CONFIG_FILE` naming a file mounted **read-only** into a scratch container: a
  well-formed file resolves; an unknown key, a bad `version`, a malformed document, and a missing file each
  **refuse startup** with an error that names the setting and never the value; the running service never
  writes the file (invariant 9).
- **Command.** `docker run --rm -v /tmp/audit-178/config.json:/etc/tucano/config.json:ro -e TUCANO_CONFIG_FILE=/etc/tucano/config.json …`
  then `docker diff` and an in-container `touch`.
- **Expected (control holds).** Refusal on every bad input (invariant 8), a write-free runtime (invariant 9),
  and no value in any error text.
- **Evidence.** Pass entries. **Pre-committed boundary limit:** the *Configuration key* boundary is not yet
  exercised — `threat-model.md` records that "**The file has no encryption yet**" and that #189's AEAD
  envelope and externally supplied key are still pending. So a secret held in the file is in the clear **by
  documented state**, and that is not a finding; the audit records the limitation and tests the loader's
  refusals only. If the executor finds #189 has landed at the audited revision, encrypt the file's secret
  and test the key boundary too.

---

### 4. Merge base and report PR for #177

- **Merge base.** Report §1 records the full SHA of `origin/main` at audit time (the merge base), the
  `Cargo.lock` SHA-256, and the filesystem type of the throwaway data directory (`stat -f -c %T /data`) so
  that flock, symlink, and permission results are interpretable. Nothing ran against a moving `main`.
- **Report.** `docs/security/audit-s2-storage-and-filesystem.md`, `# Security Audit S2 — Storage and
  filesystem invariants (Issue #177)`, epic `#166`, the S2 sentence from `audit-scope.md` §2, and "It
  carries findings only; nothing here is fixed." Then §1–§7 per the shared contract. Findings are
  `F-177-<n>`.
- **PR.** `gh pr create --base main --title "docs(security): S2 storage and filesystem audit report (#177)"`
  linking `#177`, `#166`, and this design; assign `ECiurleo`; add the README documentation-table row; do not
  merge.
- **DoD specifics.** The permissions survey (S2-2) must name every call site found, not one; the error-leak
  check (S2-12) must cover **every** error path, presented as a table; overwrite behaviour (S2-11) must be
  explicit per operation; hardlinks (S2-3.6) must be included, not only symlinks. Any test the audit
  recommends adding is written into the report text, **not** into the code (fixes are out of scope).

### 5. Unresolved for #177, recorded rather than guessed

- **The severity of the `0o666` finding** depends on facts this design cannot read: whether the host is
  multi-user, whether the data directory is a bind mount readable by other local principals, and whether the
  deployment's default publishes it. The recommended reading is Low (Difficult × Moderate, with the
  precondition exclusion stated); Medium is defensible if the executor demonstrates another local principal
  on a default deployment. Both arguments must be written down; the finding carries one severity and its
  reason.
- **Whether attachment publication in place can be observed as a torn read** depends on the write and read
  sizes and the page cache; it is measurable (S2-10.1) and this design does not predict it.
- **Whether `#189` has landed at the audited revision** changes S2-15's boundary from "documented pending"
  to "must be exercised". The executor checks `threat-model.md`'s *Decided* section at the audited
  revision and reports which state applied.

---

## #178 — S3: Container and deployment posture

### 1. Scope

The audit reviews the container and the deployment as shipped: the `Dockerfile`'s runtime posture, the
`docker-compose.yml` service definition, the secret-delivery path, the published network surface, the
healthcheck and restart behaviour, the rollback path the runbook documents, the CI security jobs measured
against `scanning-policy.md`, and the configuration-file boundary at the deployment edge. In
`audit-scope.md` §2's words, S3 is the audit of **container and deployment posture**. The subject is
therefore the **shipped files**, not a re-authored stack: the container the auditor runs is a faithful copy
of the shipped service with only the *host port number* and the *data path* changed so it can run beside the
operator's instance. The acceptance criteria are the threat model's invariants **7 (credentials never in the
clear), 8 (configuration resolved once, refused rather than defaulted), and 9 (the running service never
writes its configuration)**, plus trust boundaries **6 (Configuration file → service startup), 7
(Configuration key → service startup), and 8 (service logs and audit events)**, and the abuse cases *Secret
disclosure through the file*, *Unusable configuration*, and *Vulnerable dependency or image*.

### 2. Ambiguities, resolved

**2.1 How is the shipped stack audited without touching the operator's instance?** A **scratch copy** of the
shipped Compose file, edited in exactly two ways that cannot affect a finding's premise:

```bash
mkdir -p /tmp/audit-178 && cp "$AUDIT_ROOT/docker-compose.yml" /tmp/audit-178/compose.yml
# 1. the host port NUMBER only: 3100 -> 3310, keeping the shipped all-interfaces publish syntax
#    (`- "3310:3000"`, not `127.0.0.1:3310:3000`) so the finding's premise is preserved.
# 2. the build context path -> $AUDIT_ROOT, and the data path -> /tmp/audit-178/data.
cd /tmp/audit-178 && docker compose -p audit-178 up -d --build
```

Both edits are recorded in report §2 as deviations, together with the shipped file's own lines. The
exposure consequence (S3-3) is then measured on the copy **and** asserted against the shipped file's text:
the shipped `ports:` entry is `- "3100:3000"` with **no host-address prefix**, which Docker publishes on
`0.0.0.0` by default. Capture the exact line number at audit time:
`grep -n "ports:\|3100:3000\|TUCANO_AUTH_REQUIRED\|read_only\|tmpfs\|no-new-privileges\|cap_drop\|healthcheck" docker-compose.yml`.

**2.2 What is the adversary for S3?** "An adversary who can reach the HTTP listener" — which for S3 means
**any host that can route to the published port**, not merely a process on the same machine. The audit must
therefore test reachability from the host's own non-loopback address and, where the operator's network
permits, from another machine on the same LAN. It never needs the router to be exposed to the internet; the
default configuration already publishes the listener on every interface, and that fact plus one
unauthenticated request is the whole reproduction.

**2.3 What counts as "default configuration"?** The shipped `docker-compose.yml` run with
`docker compose up -d --build`, and a bare `cargo run`. Both are named by §5's escalation rule. The shipped
file does **not** set `TUCANO_AUTH_REQUIRED`, so the default is the authentication-off arm — the API's own
`authentication-decision.md` says "The API must **not** be exposed beyond a trusted network until this is
implemented", and requirement 5 repeats it. That the project's own documents state the constraint is what
makes publishing the port a defect rather than a preference.

**2.4 What must the secret path keep secret, and what is normative?** The `JWT secret` (or
`TUCANO_JWT_SECRET_FILE`'s content), the bootstrap password, and any configuration-file secret. Normative:
invariant 7 (never in the clear), invariant 8 (resolved once at startup; an unresolvable configuration is a
refusal, never a warning and never a plaintext fallback), the *Secret disclosure through the file* abuse
case ("Keep a secret supplied by file out of the image, out of `docker inspect`, out of logs, errors, and
responses; never fall back to plaintext when a secret-bearing file has no usable key"), and the
*Configuration key* boundary ("Key supplied from outside the artifact by a read-only mount, authenticated
encryption with a key identifier, refusal to boot rather than warn, no value ever logged").
**Documented gap, not a finding:** `threat-model.md` records that "#189's AEAD envelope and externally
supplied key are still pending, so a secret held in the file is in the clear and the *Configuration key*
boundary above is not yet exercised". The audit tests the loader's refusals and the container's hygiene; it
does not score the pending encryption.

**2.5 What is the rollback contract?** `docs/deployment/canary-validation-and-rollback.md`: "Record
`PREVIOUS` before touching anything; it is the rollback target."; "Rollback re-deploys `PREVIOUS` and re-runs
the checks; it never moves a tag."; "With Compose, restore the previously pinned image tag and recreate only
the `api` service"; and "`PREVIOUS` and `CANDIDATE` — the exact immutable tags." The shipped Compose file
instead builds locally: `image: tucano-test-api:local` with `build: .`. Whether the documented procedure has
a local equivalent is measured (S3-4), not assumed.

**2.6 Which CI claims are in scope, and what is already covered?** `scanning-policy.md` claims four
scans — dependency audit, secret scan, container scan, SBOM — and #178's DoD requires reviewing the CI
security jobs against it. But S4 already audited the workflow files and produced five findings, one Low and
four Info (**F-179-1 … F-179-5**). Re-raising them is prohibited by the audit's own baseline rule. The
resolution: #178 reads the policy claim by claim, states which claim each `security.yml` job discharges,
cites the S4 findings as `Duplicates / prerequisites` where they already cover a gap, and raises a finding
only for a gap S4 did **not** cover. The two S4 left: the published GHCR artifact is never scanned, and the
SBOM is never attached to the image. Re-read
`docs/security/audit-s4-dependencies-and-supply-chain.md` §4 before writing anything in S3-8.

### 3. Sub-tasks

---

**S3-1 — Verify every hardening claim against the running container**

- **Probe.** Start the scratch copy (2.1) and read the posture Docker actually applied, not the file's
  intent.
- **Command.**
  ```bash
  docker inspect --format '{{json .Config}}' audit-178-api-1 | python3 -m json.tool
  docker inspect --format '{{json .HostConfig}}' audit-178-api-1 | python3 -m json.tool
  docker inspect --format 'User={{.Config.User}} Readonly={{.HostConfig.ReadonlyRootfs}}
    CapAdd={{.HostConfig.CapAdd}} CapDrop={{.HostConfig.CapDrop}}
    SecOpt={{.HostConfig.SecurityOpt}} Tmpfs={{.HostConfig.Tmpfs}} Privileged={{.HostConfig.Privileged}}' audit-178-api-1
  ```
- **Expected (control holds).** `User` is `tucano`/`10001` (the Dockerfile's `useradd --system --uid 10001`)
  and not `root`; `ReadonlyRootfs=true` (`read_only: true`); `SecurityOpt` contains
  `no-new-privileges:true`; `/tmp` is a tmpfs; the data mount is the only writable mount.
- **Expected (probable actual).** `CapDrop` is **empty** while the stack applies no `cap_drop`. The
  Dockerfile drops nothing and Compose sets nothing, so the process runs with Docker's default capability
  set although it needs none of it — a hardening gap. Recommended severity **Low** (Moderate × Limited: it
  "defeat[s] one control that by itself grants nothing further", and it is not remotely reachable on its
  own). `Suggested fix` names `cap_drop: ["ALL"]` (plus the read-only rootfs the stack already has). CWE-250.
- **Evidence.** The verification table (claim → file → observed value) in report §3; `F-178-<n>` if the
  capability gap reproduces. Everything that holds is a pass entry naming the file and the field.

---

**S3-2 — Secret hygiene: the image, `docker inspect`, the logs, and the responses**

- **Probe.**
  1. Search the built image's `Config.Env`, `Config.Labels`, `Config.Cmd`, and history for the JWT secret and
     the bootstrap password used in the scratch stack.
  2. Run an arm with the secret supplied by file rather than by value:
     `-e TUCANO_JWT_SECRET_FILE=/run/secrets/jwt` with a read-only mount, and confirm the value never appears
     in `docker inspect`, in the image layer, in `docker logs`, or in any response.
  3. Supply a **too-short** secret (31 bytes) and confirm the service **refuses to start** rather than
     warning or defaulting (invariant 8), and that the error names the setting and never the value.
  4. Supply both `TUCANO_JWT_SECRET` and `TUCANO_JWT_SECRET_FILE` and a configuration-file secret and
     confirm the per-key precedence and the conflict refusal.
- **Command.**
  ```bash
  docker history --no-trunc "$AUDIT_IMAGE" | grep -i 'secret\|password\|jwt' || echo "no secret in history"
  docker inspect audit-178-api-1 --format '{{json .Config.Env}}' | tr ',' '\n' | grep -i 'jwt\|password' || echo "no secret in Env"
  docker logs audit-178-api-1 | grep -i "$AUDIT_JWT_SECRET" || echo "no secret in logs"
  docker run --rm -e TUCANO_AUTH_REQUIRED=true -e TUCANO_JWT_SECRET=tooshort… "$AUDIT_IMAGE" ; echo "exit=$?"
  ```
- **Expected (control holds).** No secret anywhere outside the process environment; the file path works from
  a read-only mount; a short secret is a startup refusal naming `TUCANO_JWT_SECRET`; no value in any error
  or log.
- **Evidence.** A per-channel table (Env / labels / history / logs / responses / errors) with each result,
  pass entries for the refusals, and `F-178-<n>` for any leaked value (severity: a leaked secret is Severe
  impact; score it per §5 with the reachability stated).

---

**S3-3 — Network exposure of the shipped default (highest-probability S3 finding)**

- **Probe.** With the scratch stack (2.1) running the **shipped** posture — `TUCANO_AUTH_REQUIRED` unset,
  the shipped all-interfaces publish — determine the bound address, then make one unauthenticated request
  from a host that is not the machine's loopback.
- **Command.**
  ```bash
  docker port audit-178-api-1                     # expect 0.0.0.0:3310 -> 3000
  ss -ltn 'sport = :3310'                         # expect LISTEN on 0.0.0.0:3310
  LAN=$(ip -4 addr show scope global | awk '/inet /{print $2}' | cut -d/ -f1 | head -1)
  curl -s -o /dev/null -w '%{http_code}\n' "http://$LAN:3310/projects"       # the money shot
  curl -s "http://$LAN:3310/projects" | head -c 400
  curl -s -o /dev/null -w '%{http_code}\n' -X POST "http://$LAN:3310/projects" \
       -H 'content-type: application/json' -d '{"id":"audit-probe.json","name":"audit probe"}'
  ```
  Then assert the same against the **shipped file** rather than the copy:
  `grep -n 'ports:' -A2 docker-compose.yml` and the `TUCANO_AUTH_REQUIRED` absence:
  `grep -n 'TUCANO_AUTH_REQUIRED' docker-compose.yml || echo "auth is not enabled by the shipped stack"`.
- **Expected (control holds).** The published port is reachable only from the operator's trust boundary, and
  no unauthenticated request returns data or writes it.
- **Expected (probable actual).** `0.0.0.0` publish plus an unauthenticated `200` carrying project data and
  an accepted unauthenticated write, from an address that is not loopback.
- **Severity, per §5.** Exploitability **Trivial** — "no account and no special position… a single request
  with no prerequisite state": the whole adversary model of `audit-scope.md` §1 is "an adversary who can
  reach the HTTP listener". Impact **Severe** — cross-project data ("Loss or corruption of data the
  attacker does not own", and the rubric's High definition "cross-project data access, write outside the
  caller's scope"). Trivial × Severe = **Critical**, and the one-level escalation for remote reachability in
  a default configuration is **already satisfied by the shipped file**, so it is neither applied twice nor
  available as a de-escalation. The finding must address, explicitly, the counter-argument that the project
  documents a "trusted local network" expectation: the defect is not that auth is optional, it is that the
  shipped default publishes the listener on every interface while leaving the control that makes it safe
  turned off, so the safe configuration is the one an operator has to discover. `Suggested fix`: publish on
  `127.0.0.1` by default and/or set `TUCANO_AUTH_REQUIRED=true` in the Compose file, and state the
  expectation in the README. CWE-306 (missing authentication for a critical function) or CWE-1327 (binding
  to an unrestricted IP address) — pick the one the observed mechanism matches.
- **Evidence.** `F-178-1` with `docker port`, `ss`, the raw curl response, the two `grep` outputs from the
  shipped file, and the reproduction from a clean scratch stack. This is the finding the S4 report's
  calibration examples anticipated; keep the reproduction pasted, not described.

---

**S3-4 — The rollback path against the shipped Compose file**

- **Probe.** Follow the runbook's rollback literally against the scratch stack: build, record `PREVIOUS`,
  change something, `docker compose up -d --build` again, and try to re-deploy `PREVIOUS` by tag.
- **Command.**
  ```bash
  docker compose -p audit-178 up -d --build
  PREVIOUS=$(docker inspect --format '{{.Image}}' audit-178-api-1); echo "$PREVIOUS"
  # change a byte in the source, rebuild, then inspect what happened to the tag:
  docker compose -p audit-178 up -d --build
  docker image ls --filter reference='tucano-test-api' --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.CreatedSince}}'
  docker image ls --filter dangling=true
  docker compose -p audit-178 up -d api      # the runbook's "restore the previously pinned image tag"
  ```
- **Expected (control holds).** The runbook's Compose instruction — "restore the previously pinned image
  tag and recreate only the `api` service" — is executable with an immutable tag, and the restored container
  runs the previous image.
- **Expected (probable actual).** `image: tucano-test-api:local` + `build: .` means each `--build` retags
  the single local name; the previous image id becomes `<none>`, so `PREVIOUS` is not addressable by tag and
  the documented rollback has no local equivalent. Score it: the defect is a process/documentation control
  that does not hold. Recommended **Low** (Difficult × Limited: it needs a failed release *and* an operator
  who follows the runbook, and its consequence is a slower rollback, not disclosed or corrupted data), and
  say so in the finding rather than reaching for Medium. `Suggested fix`: publish immutable
  `build-<run number>`/`vMAJOR.MINOR.PATCH` tags from CI and reference them in Compose, or amend the runbook
  to record the image id and `docker tag` it back. CWE-1059 or CWE-693 — pick the closer one.
- **Evidence.** `F-178-2` with the runbook quotes (including the `PREVIOUS="$IMAGE:build-4700"` line and
  "`PREVIOUS` and `CANDIDATE` — the exact immutable tags."), the two `docker image ls` outputs, and the
  failed restore.

---

**S3-5 — Healthcheck and restart policy**

- **Probe.** Wedge the process without exiting it, then ask whether the stack notices.
- **Command.**
  ```bash
  docker inspect --format '{{json .Config.Healthcheck}}' audit-178-api-1      # expect null
  grep -n 'HEALTHCHECK' "$AUDIT_ROOT/Dockerfile" || echo "no HEALTHCHECK in the Dockerfile"
  grep -n 'healthcheck' docker-compose.yml || echo "no healthcheck in Compose"
  docker exec audit-178-api-1 sh -lc 'kill -STOP 1'
  docker ps --filter name=audit-178-api-1 --format '{{.Status}}'              # still "Up"
  curl -s -o /dev/null -w '%{http_code}\n' --max-time 5 http://127.0.0.1:3310/health   # hangs
  docker exec audit-178-api-1 sh -lc 'kill -CONT 1'
  ```
- **Expected (control holds).** A liveness signal exists and `restart: unless-stopped` acts on a wedged
  process.
- **Expected (probable actual).** No `HEALTHCHECK` in the Dockerfile, no `healthcheck:` in Compose, while
  `/health` is a public endpoint and the service is declared with `restart: unless-stopped`; a `STOP`ped
  process stays `Up` and accepts connections it never answers. Recommended **Low** or **Info**: it is a
  missing availability control, in the auditor's own container, with no data or credential impact, and
  `Suggested fix` is a `HEALTHCHECK` plus a Compose `healthcheck:`. CWE-1059 (insufficient examination) is
  the closest.
- **Evidence.** `F-178-<n>` or an Info observation, with the four command outputs pasted. Restore the
  process and confirm `/health` answers again before tear-down.

---

**S3-6 — Build context and `.dockerignore`**

- **Probe.** Confirm the repository has no `.dockerignore`, then measure what a `context: .` build actually
  sends and whether anything sensitive can enter the image.
- **Command.**
  ```bash
  ls -a | grep -c dockerignore || echo "no .dockerignore"
  DOCKER_BUILDKIT=0 docker build --no-cache --target builder -t /dev/null "$AUDIT_ROOT" 2>&1 | grep -i 'sending build context\|transferring context'
  tar --exclude-vcs-ignores -cf - -C "$AUDIT_ROOT" . | wc -c
  docker history --no-trunc "$AUDIT_IMAGE" | grep -i 'COPY\|ADD'
  grep -n 'COPY\|ADD' "$AUDIT_ROOT/Dockerfile"
  ```
- **Expected (control holds).** The context is bounded and carries no data or secret; the image's layers are
  exactly the intended files.
- **Expected (probable actual).** No `.dockerignore` while both `container-scan` and `release.yml` use
  `context: .`; the whole worktree (including `.git` and any operator data directory that exists at build
  time) is sent to the daemon. The Dockerfile's `COPY` instructions name only `Cargo.toml`, `Cargo.lock`,
  `src`, `openapi.json`, and `swagger.html`, so the files do **not** enter the image — the consequence is
  context size, build-cache exposure on a shared builder, and a latent hazard if a `COPY .` is ever added.
  **Record as an Info observation**, not a finding, unless the audit can show a concrete file entering the
  image or a shared cache receiving a secret.
- **Evidence.** The measured context size, the `COPY` inventory, and either the Info observation or the
  finding with the concrete file named.

---

**S3-7 — Tooling and documentation consistency for the shipped ports**

- **Probe.** Run the test script with its default and against the shipped host port.
- **Command.**
  ```bash
  bash "$AUDIT_ROOT/scripts/smoke.sh"                    # defaults to http://localhost:3000
  bash "$AUDIT_ROOT/scripts/smoke.sh" http://127.0.0.1:3310
  grep -n '3100' README.md docs/wiki/getting-started.md | head
  ```
- **Expected (control holds).** The default invocation works against the shipped stack.
- **Expected (probable actual).** `scripts/smoke.sh` defaults to `http://localhost:3000` while the shipped
  Compose publishes `3100` (documented as such in `README.md` and `docs/wiki/getting-started.md`), so the
  default invocation fails against the shipped stack. Recommended **Info** — a tooling/documentation
  mismatch with no security effect.
- **Evidence.** The two invocations' output and the grep, recorded as an Info observation.

---

**S3-8 — CI security jobs against `scanning-policy.md` (the DoD item)**

- **Probe.** Read `docs/security/scanning-policy.md` and `.github/workflows/security.yml` together, claim by
  claim, and record for each claim the job that discharges it, the evidence in the job definition, and what
  the job does *not* do. Then read
  `docs/security/audit-s4-dependencies-and-supply-chain.md` §4 and mark each gap as already found or new.
- **Claim table to fill in (the columns are the deliverable).**

| Policy claim | Job | Evidence in the job | Gap | Already in S4? |
| --- | --- | --- | --- | --- |
| dependency advisory scan runs on every PR and push to `main` | `audit` | `cargo audit` on a pinned `RUSTUP_TOOLCHAIN` | verify the job can actually fail (no `continue-on-error`, no `|| true`) | — |
| secret scan "scans repository history" | `secret-scan` | `gitleaks detect --source /repo --no-git --redact` | filesystem-only: it cannot read history | **yes — F-179-5** |
| container scan fails on CRITICAL/HIGH | `container-scan` | `trivy-action`, `exit-code: 1`, `ignore-unfixed: true` | fixed-criteria only; scans only its own local build | partially (F-179-1/F-179-2 are the pinning) |
| SBOM is produced and validated | `sbom` | cyclonedx + inline validation + `upload-artifact` | never attached to the published image | **no** |
| published artifact is scanned | — | `release.yml` pushes without a scan step | no scan, no `provenance:`/`sbom:`/`attestations:`, no `id-token` | **no** |
| scans run weekly as well as per-PR | all four | `cron: "17 3 * * 1"` | — | — |
| actions are pinned | all four | major-tag references | not SHA-pinned | **yes — F-179-2** |

- **Expected (control holds).** Each policy claim has a job that enforces it, and the published artifact is
  the same artifact that was scanned.
- **Expected (probable actual).** Two gaps S4 did not cover: the published GHCR image is never scanned, and
  the SBOM never accompanies the image. Recommended **Low** or **Info** each — they are supply-chain
  assurance gaps with no demonstrated exploitation, and S4's calibration placed the analogous pinning
  findings at Low/Info. Cite `F-179-1` and `F-179-2` as prerequisites where the fix overlaps.
- **Evidence.** The completed claim table in report §3, `F-178-<n>` for the two new gaps with the workflow
  lines quoted, and pass entries for the claims that hold. **Do not re-raise F-179-3, F-179-4, or F-179-5.**

---

**S3-9 — Configuration boundary at the deployment edge**

- **Probe.** With the config file read-only mounted and `TUCANO_CONFIG_FILE` set: a good file resolves; an
  unknown key, a bad `version`, a malformed document, and a missing file each refuse startup naming the
  setting and never the value; the running service never writes the file; and a config-supplied secret is
  returned in no response and no log.
- **Command.**
  ```bash
  docker run --rm -v /tmp/audit-178/conf:/conf:ro -e TUCANO_CONFIG_FILE=/conf/config.json "$AUDIT_IMAGE"; echo "exit=$?"
  # repeat with a bad key, a bad version, a truncated file, and TUCANO_CONFIG_FILE pointing at nothing
  docker run -d --name audit-conf -v /tmp/audit-178/conf:/conf:ro -e TUCANO_CONFIG_FILE=/conf/config.json …
  docker diff audit-conf ; docker exec audit-conf sh -lc 'touch /conf/x' ; echo "exit=$?"
  docker logs audit-conf | grep -i "$AUDIT_JWT_SECRET" || echo "no value in logs"
  ```
- **Expected (control holds).** Refusal on every unresolvable input (invariant 8); a write-free runtime
  (invariant 9); no value named in any error; the read-only mount keeps working.
- **Expected (probable actual)** — see 2.4. The AEAD envelope is pending (#189), so a file-supplied secret is
  in the clear **by documented state**: record the limitation, do not score it. The *Configuration key*
  boundary stays "not yet exercised", which the report states explicitly.
- **Evidence.** Pass entries for the refusals and the write-free runtime; a recorded limitation for the
  unencrypted-file state; `F-178-<n>` only if a refusal *names a value*, which invariant 8 forbids outright.

---

**S3-10 — Root filesystem, writable mounts, and the data-root invariant**

- **Probe.** Confirm the running container writes nothing to its image layer, that the only writable
  locations are the data mount and the tmpfs, and that the data root is the only area the service touches.
- **Command.**
  ```bash
  docker inspect --format '{{json .Mounts}}' audit-178-api-1 | python3 -m json.tool
  docker exec audit-178-api-1 sh -lc 'touch /nope' ; echo "exit=$?"     # read-only rootfs: must fail
  docker diff audit-178-api-1
  ```
- **Expected (control holds).** Writes outside `/data` and `/tmp` fail; `docker diff` shows nothing on the
  image layer. Shares its evidence with S2-13 and supports invariant 9.
- **Evidence.** Pass entries quoting invariants 1 and 9.

---

**S3-11 — The asset lines of `audit-scope.md` §2 for S3**

- **Probe.** Address the S3 sentences not covered above: the image as a *deployable artifact* (label, tag,
  and version discipline — `LABEL org.opencontainers.image.version="${BUILD_NUMBER}"` with the default
  `local`), and "node/`docker inspect`-visible" deployment metadata.
- **Expected (control holds).** The image identifies its build; nothing in its metadata discloses a secret
  or an internal path; the version label is set by the build argument and is `local` for a local build.
- **Expected (probable actual).** The facts hold; record them as pass entries. Only a metadata disclosure
  becomes a finding.
- **Evidence.** The `docker inspect` metadata excerpt and the pass entries; any disclosure is
  `F-178-<n>`.

---

### 4. Merge base and report PR for #178

- **Merge base.** Report §1 records the full SHA of `origin/main` at audit time (the merge base), the
  `Cargo.lock` SHA-256, **and the built image id** (`docker inspect --format '{{.Id}}'`), because the
  artifact is half this audit's subject. The audit ran from the detached worktree at that revision; a moved
  `main` afterwards is a different audit.
- **Report.** `docs/security/audit-s3-container-and-deployment.md`, `# Security Audit S3 — Container and
  deployment posture (Issue #178)`, epic `#166`, the S3 sentence from `audit-scope.md` §2, and "It carries
  findings only; nothing here is fixed." Then §1–§7 per the shared contract, with §2 naming the two
  deviations of the scratch Compose copy (host port number, paths) and §3 carrying the hardening-claim
  table, the CI claim table, and the Compose/Dockerfile key inventory. Findings are `F-178-<n>`.
- **PR.** `gh pr create --base main --title "docs(security): S3 container and deployment audit report (#178)"`
  linking `#178`, `#166`, and this design; assign `ECiurleo`; add the README documentation-table row; do not
  merge.
- **DoD specifics.** The CI review (S3-8) must be presented as the claim table, not prose, and must cite
  the S4 findings it does not re-raise. Dropped capabilities (S3-1), the healthcheck and restart policy
  (S3-5), and the rollback path (S3-4) must each appear either as a finding or as a pass entry — the DoD
  names all three. `#178` is labelled `complexity:S`; the audit stays inside S3 and does not wander into S1
  or S2 (the exposure test in S3-3 is a *deployment* fact, not an HTTP-surface audit, and its `In scope:`
  line says so).

### 5. Unresolved for #178, recorded rather than guessed

- **The severity of the published-port finding.** This design's reading is Critical (Trivial × Severe, in
  the shipped default configuration), and it is the one call in this document that a reasonable auditor
  could argue differently: the counter-argument is that the project documents a "trusted local network"
  expectation, so the exposure is a configuration choice rather than a defect. Both readings are written
  into S3-3 so the executor can choose with the evidence in hand; the finding must state which reading it
  took and why. Re-read `audit-scope.md` §5's escalation rule before writing the severity line — the
  escalation is not available as an *extra* because the shipped file already is the default.
- **Whether the missing `cap_drop` is Low or Info.** It is a hardening gap with no demonstrated
  exploitation; the executor decides from §5's definitions and states the impact axis.
- **Whether the two new CI gaps are Low or Info**, and whether S4 already raised them under a different
  wording. S3-8 requires reading S4 §4 first; if S4 covered one, it becomes a prerequisite citation rather
  than a finding.
- **Whether the operator's network permits a genuine off-host probe** for S3-3. If it does not, the
  loopback-address probe plus the shipped `0.0.0.0` publish evidence is the reproduction, and the report
  says plainly that the request was made from the host's own non-loopback address rather than from a second
  machine.
- **Whether `#189` has landed**, which changes 2.4 from a recorded limitation to a boundary that must be
  exercised.

---

## Execution order and dependencies

**The order.**

1. **#178 first, or in parallel with nothing else running.** It needs only the built image and a scratch
   Compose copy; it does not need the seed, the two arms, or the bind-mounted data directory. Its S3-3
   exposure test is the finding most sensitive to drift in the shipped files, so run it while the shipped
   files are exactly the audited revision's.
2. **#177 second**, on the two-arm seeded target. It needs the fixtures the seed creates and the writable
   data directory, and it is the audit that destroys its own target most aggressively (kills, corruptions,
   chmods) — so it runs after any audit that needs a clean tree.
3. **#176 last, on a fresh seed**, because #177 leaves the volume deliberately damaged. Re-create the data
   directory (shared contract A.4–A.7) and run #176 against a clean seeded stack. If the executor prefers
   one session, run #176 first on a fresh seed and re-seed before #177 — either way, **no audit may run
   against a tree another audit has already mutated**.

**Shared infrastructure.** #176 and #177 share the recipe: one image at `$REV`, one private data directory,
arm A on 3210 and arm B on 3211, one seeding through arm B, one teardown. #178 shares the image and needs a
separate scratch directory (`/tmp/audit-178/`) and its own copy of the Compose file. Nothing shares the
operator's ports, volume, or containers.

**What each audit must not re-do.** #176 and #177 both touch error disclosure: the rule is **one finding,
one place**, with the other report cross-referencing it under `Duplicates / prerequisites`. #178 must not
re-raise the S4 findings (F-179-1 … F-179-5); it cites them. No audit writes a fix, a test, or a code
change; the audit PRs contain documentation only.

**Related tickets.**

- **[#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179) — S4 dependencies and supply
  chain: CLOSED.** Its report, `docs/security/audit-s4-dependencies-and-supply-chain.md`, is the accepted
  report-PR precedent this design copies, and its five findings are the supply-chain floor for #178's CI
  review. Its report's counts (46 paths / 67 operations at revision `61b02b92…`) are **stale** — the
  contract at this design's revision has 49 paths / 71 operations, and every audit re-derives its own
  numbers from `openapi.json` at its own revision rather than copying S4's.
- **[#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180) — triage of the audit's findings:
  BLOCKED.** It is labelled `model:mid` and cannot start until the S1, S2, and S3 reports exist. It consumes
  all three reports, assigning each finding an owner and a decision (fix, accept, or defer), and it needs
  the same two-arm seeded target plus whatever fixture a specific finding's reproduction requires — each
  finding must carry its reproduction with it, which is why §4's evidence discipline is not optional.
- **[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — the epic — closes only when all
  of #176, #177, #178, and #180 are closed.** Its own definition of done adds two things no single S1/S2/S3
  report satisfies: a consolidated summary under `docs/security/`, linked from the README documentation
  table, and an update to `threat-model.md` if the trust model changed. If an audit finds a boundary that
  was not modelled, or a control the threat model declares present that does not hold, that change is noted
  in the closing section of the epic's summary — not fixed in an audit PR.

### Ambiguities that remain open, recorded rather than guessed

These are the questions a design pass could not close by reading, with the probe or decision that settles
each. They are listed here so no executor has to invent an answer and no reader mistakes a decision for a
measurement.

1. **The severity of the shipped all-interfaces publish with authentication off (#178, S3-3).** Reading:
   Critical. The competing reading — a documented "trusted local network" expectation makes it a
   configuration choice — is written into S3-3 with the argument the finding must answer either way.
2. **The severity of the `0o666` file mode (#177, S2-2).** Reading: Low, with the precondition exclusion
   stated. Medium is defensible only with demonstrated exposure to another principal on a default
   deployment.
3. **Whether an attacker-influenced attachment content type is a finding (#176, S1-9.4).** It is a finding
   only with the browser-origin argument made against the real response headers; otherwise a pass entry.
4. **The effective maximum attachment size (#176, S1-9.1).** Measurable in one probe; the interaction
   between `RequestBodyLimitLayer` and the per-part check decides it, and this design does not predict which
   refusal wins.
5. **The timing-enumeration threshold (#176, S1-12).** No document names one; the finding is written only
   if the distributions separate cleanly, and the numbers go in the finding.
6. **`logout` does not revoke the access token (#176, S1-5.8).** A finding only if a document promises
   otherwise; the executor must check every security document at the audited revision, not only
   `authentication-decision.md`.
7. **Whether the two uncovered CI gaps are Low or Info, and whether S4 already covered them (#178, S3-8).**
   S4 §4 must be read first; a covered gap becomes a prerequisite citation.
8. **Whether the missing parent-directory `fsync` should ever be a finding (#177, S2-5).** Pre-committed
   answer: **no**. No invariant claims power-loss durability, and this design fixes that reading so the
   executor does not have to re-litigate it.
9. **Whether the configuration-key boundary must be exercised (#177 S2-15, #178 S3-9).** It depends on
   whether #189 has landed at the audited revision; `threat-model.md`'s *Decided* section as of this design
   records that "**The file has no encryption yet**", so the boundary stays unexercised and that state is
   recorded, not scored.
10. **What filesystem the throwaway volume sits on.** `overlay`, `ext4`, and `tmpfs` differ in flock,
    symlink, and permission behaviour. The executor records `stat -f -c %T /data` in report §1 so every S2
    result is interpretable; a result obtained on `tmpfs` is not a result about a real deployment volume.
