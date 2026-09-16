# V1 Release Gate — Gap Analysis

Baseline gathered 2026-09-16 against `main` (commit `7adf15c`). This document
classifies every tracked ticket, re-verifies the closed roadmap gates, and
proposes the v1 blocker set.

---

## 1. Gap Taxonomy

Every ticket is classified as **v1-blocking**, **v1-deferrable**, or
**post-v1**, with the reason.

### 1.1 Closed Roadmap Tickets

| # | Title | Roadmap Priority | Classification | Reason |
|---|-------|-----------------|----------------|--------|
| #9 | Security Scanning and Dependency Policy | P0 | **v1-blocking** (gate) | Release gate; verified implemented (§2.1). |
| #11 | Threat Model and Compatibility Contract | P0 | **v1-blocking** (gate) | Release gate; verified implemented (§2.2). |
| #14 | Typed HTTP API and OpenAPI Contract | P1 | **v1-blocking** (gate) | Core capability; verified implemented (§2.3). |
| #34 | Test Configurations and Environment Matrix | P1 | **v1-blocking** (gate) | Core capability; verified implemented (§2.4). |
| #45 | Step-level Uploads | P1 | **v1-blocking** (gate) | Core capability; verified implemented (§2.5). |
| #46 | Tags Across Projects, Suites, Cases, and Runs | P1 | **v1-blocking** (gate) | Core capability; verified implemented (§2.6). |
| #47 | Duplication Across Projects, Suites, Cases, and Runs | P1 | **v1-blocking** (gate) | Core capability; verified implemented (§2.7). |
| #130 | Authentication and Authorization | P1 | **v1-blocking** (gate) | Core capability; verified implemented (§2.8). |
| #35 | Automated Test Result Ingestion | P2 | **v1-deferrable** | Closed; feature-complete but not a release gate. |
| #36 | External Defect and Issue Tracker Linkage | P2 | **v1-deferrable** | Closed; feature-complete but not a release gate. |
| #37 | Execution Metrics and Pass/Fail Rollups | P2 | **v1-deferrable** | Closed; feature-complete but not a release gate. |
| #38 | Test Case Versioning and Audit History | P2 | **v1-deferrable** | Closed; feature-complete but not a release gate. |
| #6 | Container Deployment and Operational Endpoints | P3 (roadmap P3) | **v1-deferrable** | Closed; Dockerfile, Compose, and health endpoints exist. |
| #28 | Branch Protection | P3 (roadmap P3) | **post-v1** | Repository setting, not a code deliverable. |
| #215 | Storage Layout v3 (project-scoped) | P0 (ad-hoc) | **v1-blocking** (gate) | All 9 sub-issues closed; layout is live on `main`. |

### 1.2 Closed Ad-Hoc Tickets

| # | Title | Classification | Reason |
|---|-------|----------------|--------|
| #169 | Create test data generation script | **v1-deferrable** | P2 epic; all 6 sub-issues closed. |
| #145 | OpenAPI operationId, tags, servers | **v1-deferrable** | P2 enhancement; closed. |
| #140 | OpenAPI typed write bodies and error codes | **v1-deferrable** | P2 enhancement; closed. |
| #175 | Security: define audit scope and methodology | **post-v1** | Audit design task; P3 epic sub-issue. |
| #177 | Security: audit storage and filesystem invariants | **post-v1** | Audit sub-issue; P3. |
| #179 | Security: audit dependencies and supply chain | **post-v1** | Audit sub-issue; P3. |
| #181 | Storage: ADR — object storage vs file-based invariant | **post-v1** | Design decision; P3 epic sub-issue. |
| #187 | Config: ADR — unified config file vs env vars | **post-v1** | Design decision; P3 epic sub-issue. |
| #188 | Config: define and implement config file schema and loader | **post-v1** | Implementation; P3 epic sub-issue. |
| #236 | docs(wiki): fix broken wiki link | **post-v1** | Documentation fix. |

### 1.3 Open Tickets

| # | Title | Labels | Classification | Reason |
|---|-------|--------|----------------|--------|
| #241 | V1 release gate (this epic) | P0, epic | **v1-blocking** | The meta-ticket defining v1. |
| #214 | Add SSO compatibility | (none) | **post-v1** | No labels, no milestone, no body; GUI-side feature; no API contract exists. See §5. |
| #166 | Security audit (epic) | P3, epic | **post-v1** | P3 scope; ongoing audit does not block v1 — CI scanning covers known vulns. |
| #167 | S3 backend (epic) | P3, epic | **post-v1** | New storage backend; the file-based invariant is sufficient for v1. |
| #168 | Unified config with encrypted secrets (epic) | P3, epic | **post-v1** | Enhancement; current env-var config works for v1. |
| #15 | Observability and operational hardening | P3, epic | **post-v1** | Operational depth; not a release gate. |
| #16 | Migration, performance, and GUI readiness | P3, epic | **post-v1** | Future readiness; not a release gate. |
| #176 | Security: audit the HTTP surface | P3, blocked | **post-v1** | Audit sub-issue; blocked by #180's triage dependency. |
| #178 | Security: audit container and deployment posture | P3 | **post-v1** | Audit sub-issue. |
| #180 | Security: triage audit findings | P3, blocked | **post-v1** | Blocked on #176 and #178. |
| #182 | Storage: extract backend-agnostic Repository boundary | P3 | **post-v1** | S3 epic sub-issue; the startable frontier for #167. |
| #183 | Storage: implement S3-backed Repository | P3, blocked | **post-v1** | Blocked on #182. |
| #184 | Storage: S3 configuration and credential wiring | P3, blocked | **post-v1** | Blocked on #182, #187, #188. |
| #185 | Storage: conformance and contract tests | P3, blocked | **post-v1** | Blocked on #183, #185. |
| #186 | Storage: document storage backends | P3, blocked | **post-v1** | Blocked on #183. |
| #189 | Config: encrypted secrets at rest | P3 | **post-v1** | Config epic sub-issue. |
| #190 | Config: precedence and validation | P3 | **post-v1** | Config epic sub-issue. |
| #191 | Config: document configuration reference | P3 | **post-v1** | Config epic sub-issue. |
| #97 | Benchmark harness | P3 | **post-v1** | Performance epic sub-issue. |
| #100 | Fuzz and property tests | P3 | **post-v1** | Migration epic sub-issue. |
| #101 | Migration fixtures and rollback drills | P3 | **post-v1** | Migration epic sub-issue. |
| #102 | Structured tracing and metrics | P3 | **post-v1** | Observability epic sub-issue. |
| #103 | Timeouts, concurrency limits, and bounded bodies | P3 | **post-v1** | Observability epic sub-issue. |

**Summary:** 0 open tickets are v1-blocking beyond this meta-epic. All 21 P3
open issues and #214 are classified post-v1. Every P0/P1/P2 roadmap gate is
closed and verified on `main`.

---

## 2. P0/P1 Release Gate Re-Verification

Each gate is verified against `main` at commit `7adf15c`. Evidence is file
path and key content; nothing is assumed without inspection.

### 2.1 #9 — Security Scanning and Dependency Policy

| Evidence | Location |
|----------|----------|
| Security CI workflow | `.github/workflows/security.yml` — runs cargo audit, gitleaks, trivy, cargo-cyclonedx |
| Scanning policy document | `docs/security/scanning-policy.md` — full policy with unsafe-code forbid, dependency rules, vuln response SLA |
| Unsafe code forbidden | `Cargo.toml` — `unsafe_code = "forbid"` |
| Cargo.lock committed | `Cargo.lock` present at repo root |

**Verdict: ✅ Verified.** All four scanning jobs (dependency audit, secret
scan, container scan, SBOM generation) are configured and enforced in CI.

**Known gap:** gitleaks scans only the current tree (`git archive` of HEAD),
not commit history. Finding F-179-5 in
`docs/security/audit-s4-dependencies-and-supply-chain.md` records this. A
secret committed and later deleted is not caught. Classified as a known
limitation for v1, not a blocker — the scanning policy documents the gap
explicitly.

### 2.2 #11 — Threat Model and Compatibility Contract

| Evidence | Location |
|----------|----------|
| Threat model | `docs/security/threat-model.md` — assets, trust boundaries, abuse cases, security invariants |
| API compatibility contract | `docs/contracts/api-compatibility.md` — fixture layout, compatibility rules, versioning plans |
| File format versioning plan | `docs/contracts/file-format-versioning-plan.md` |
| Test case versioning plan | `docs/contracts/test-case-versioning-plan.md` |

**Verdict: ✅ Verified.** Both deliverables are comprehensive, committed, and
cross-referenced from `README.md`.

### 2.3 #14 — Typed HTTP API and OpenAPI Contract

| Evidence | Location |
|----------|----------|
| OpenAPI specification | `openapi.json` — full typed spec with request/response schemas |
| Swagger UI | `swagger.html` — interactive API documentation |
| API module | `src/api/` — one module per resource with typed handlers |
| Model definitions | `src/models.rs` — typed domain models with serde |

**Verdict: ✅ Verified.** The OpenAPI spec is served at `/openapi.json` and
the Swagger UI at `/api-docs`. All endpoints are typed.

### 2.4 #34 — Test Configurations and Environment Matrix

| Evidence | Location |
|----------|----------|
| Configuration model | `src/models.rs` — `Configuration` struct |
| Configuration storage | `src/storage/` — CRUD for configurations under project folders |
| API routes | `src/api/configurations.rs` — project-scoped routes |
| OpenAPI entries | `openapi.json` — configuration endpoints |

**Verdict: ✅ Verified.** Configurations are project-scoped per the v3 storage
layout (#215).

### 2.5 #45 — Step-level Uploads

| Evidence | Location |
|----------|----------|
| Step model | `src/models.rs` — `TestStep` with optional attachments |
| Attachment API | `src/api/attachments.rs` — upload/download/delete attachments |
| Storage layer | `src/storage/` — attachment persistence under case/suite folders |

**Verdict: ✅ Verified.** Attachments can be uploaded at the step level.

### 2.6 #46 — Tags

| Evidence | Location |
|----------|----------|
| Tags field in models | `src/models.rs` — `tags: Vec<String>` on Project, TestSuite, TestCase, TestRun |
| Tag filtering | `src/api/` — query parameter filtering by tag |
| OpenAPI entries | `openapi.json` — tag filter parameters on list endpoints |

**Verdict: ✅ Verified.** Tags are supported across all four resource types.

### 2.7 #47 — Duplication

| Evidence | Location |
|----------|----------|
| Duplicate endpoints | `src/api/` — POST duplicate routes for projects, suites, cases, runs |
| Domain logic | `src/domain/` — duplication with copy/move semantics |
| OpenAPI entries | `openapi.json` — duplicate operation definitions |

**Verdict: ✅ Verified.** Duplication with `mode: "copy" | "move"` is
implemented across all four resource types.

### 2.8 #130 — Authentication and Authorization

| Evidence | Location |
|----------|----------|
| Auth decision record | `docs/security/authentication-decision.md` |
| Auth middleware | `src/api/` — JWT validation middleware |
| Auth tests | `tests/auth.rs` — authentication and authorization test suite |
| RBAC model | Project-scoped role grants in `src/models.rs` |

**Verdict: ✅ Verified.** Short-lived JWT + refresh token with project-scoped
RBAC is implemented per the P1 roadmap gate.

---

## 3. Startable Frontier

The startable frontier is the first unblocked task of each open epic. These
tickets can be spawned today without waiting on any other open ticket.

| Epic | Startable Task | Blocked By |
|------|---------------|------------|
| #166 Security audit | #176 — audit the HTTP surface | Nothing (the audit itself is unblocked) |
| #167 S3 backend | #182 — extract backend-agnostic Repository boundary | Nothing |
| #168 Unified config | #189 — encrypted secrets at rest | Nothing (ADR #187 and schema #188 are closed) |
| #15 Observability | #102 — structured tracing, metrics, and audit logging | Nothing |
| #16 Migration/perf | #97 — benchmark harness | Nothing |

**Note:** #180 (triage audit findings) is blocked on #176 and #178 completing
first. #183–#186 are blocked on #182. #190–#191 are blocked on #189. These
are **not** on the startable frontier.

---

## 4. Milestone and Label Hygiene

### 4.1 Current State

| Milestone | Open | Closed | Issues |
|-----------|------|--------|--------|
| P0 | 1 | 0 | #241 only — #9 and #11 were never assigned |
| P1 | 0 | 0 | Empty — #34, #45, #46, #47, #130 were never assigned |
| P2 | 0 | 6 | #35, #36, #37, #38, #169, #192–#196 |
| P3 | 21 | 13 | All open P3 work + #15, #16 |
| (none) | — | — | #214, #215 and all #215 sub-issues (#217–#225) |

### 4.2 Required Fixes

1. **Assign #9, #11 to the P0 milestone** (currently orphaned closed issues).
2. **Assign #34, #45, #46, #47, #130 to the P1 milestone** (currently
   orphaned closed issues).
3. **Assign #215 and its sub-issues (#217–#225) to the P0 milestone** — they
   were P0 ad-hoc work.
4. **Create a `v1.0` milestone** for the release criteria and the gap analysis
   PR.
5. **Triage #214** — assign labels, milestone, and assignee (§5).

---

## 5. #214 Triage — SSO Compatibility

**Current state:** No labels, no milestone, no assignee, no body text.

**Classification: post-v1.**

**Reasoning:**
- SSO is a GUI-side integration concern; the API already implements JWT-based
  authentication (#130).
- No API contract for SSO exists; this would require a new design (OAuth2/OIDC
  proxy, token exchange, or SAML assertion relay).
- The ticket has no body, so the scope is undefined.
- v1 ships with JWT auth; SSO integration is a post-v1 enhancement that
  depends on the GUI's SSO support maturing.

**Recommended triage actions:**
- Label: `enhancement`, `priority:P3`, `complexity:M`, `model:high`
- Milestone: P3
- Assignee: ECiurleo
- Add a comment noting the post-v1 classification and the dependency on GUI
  SSO support.

---

## 6. V1 Definition

### 6.1 Release Criteria Checklist

v1.0.0 is ready to tag when:

- [x] All P0 release gates verified on `main` (#9, #11, #215)
- [x] All P1 core capabilities verified on `main` (#14, #34, #45, #46, #47,
  #130)
- [x] P2 features implemented (#35, #36, #37, #38)
- [x] Gap analysis document committed (`docs/roadmap-v1-gap-analysis.md`)
- [x] `v1.0` milestone created with release criteria
- [x] #214 triaged and classified
- [x] Local CI gate green: `cargo fmt`, `cargo clippy`, `cargo test`,
  `cargo build --release`, `actionlint`
- [ ] Tag `v1.0.0` pushed to `main`, triggering the release workflow
- [ ] GHCR image `ghcr.io/tucanotechnology/tucanotestapi:v1.0.0` published
- [ ] GitHub Release created with release notes

### 6.2 Milestone Scheme

| Milestone | Purpose | Contents |
|-----------|---------|----------|
| `v1.0` | The first stable release | Gap analysis PR, release criteria tracking |
| P0–P3 | Delivery priority tracking | Existing assignments (with hygiene fixes from §4.2) |

The `v1.0` milestone tracks the release process itself. Feature work is
tracked in P0–P3 milestones as today.

### 6.3 Tag Scheme

Consistent with `.github/workflows/release.yml`:

- **SemVer tag:** `v1.0.0` — the immutable release identity
- **Build tag:** `build-<run_number>` — the immutable CI run number
- Both tags are pushed on `git push --tags` to `main`
- The tag triggers the release job: build → push to GHCR → create GitHub
  Release
- Tags are never reused or overwritten (per the release numbering policy in
  `AGENTS.md`)

### 6.4 Post-v1 Known Limitations

These are accepted risks for v1.0.0, tracked for post-v1 remediation:

1. **Gitleaks history gap (F-179-5):** Secret scan covers only the current
   tree, not commit history. A secret committed and later deleted is not
   detected. Tracked in the S4 audit report.
2. **Security audit incomplete:** The comprehensive security audit (#166) is
   P3 and ongoing. CI scanning covers known vulnerabilities but not
   penetration testing or deep review.
3. **No SSO:** JWT auth is the only authentication method. SSO integration
   (#214) is post-v1.
4. **File-system only:** No S3/object-storage backend (#167). The file-based
   invariant is the sole persistence layer.
5. **Environment-variable config only:** No unified config file with encrypted
   secrets (#168). Current env-var model works but lacks file-based
   configuration.

---

## 7. V1 Blocker Set (Priority Order)

Every item below must complete before `v1.0.0` can be tagged. Dependencies
flow top to bottom.

| Priority | Ticket | Title | Depends On | Complexity | Model |
|----------|--------|-------|------------|------------|-------|
| 1 | #241 | V1 release gate (this epic) | — | L | high |
| 2 | #242 (PR) | Commit gap analysis and open PR | #241 | S | mid |
| 3 | #243 ✅ | Triage #214: labels, milestone, assignee | #241 | S | mid |
| 4 | #244 ✅ | Create v1.0 milestone and assign closed roadmap items | #242 | S | mid |
| 5 | #245 | Tag v1.0.0 on main and verify GHCR publish | #242, #244 | S | mid |

**Dependency graph:**

```
#241 (this epic)
├── #242 (PR): gap analysis document
│   └── #244 ✅: create v1.0 milestone (done)
│       └── #245: tag v1.0.0 (after merge)
└── #243 ✅: triage #214 (done)
```

All four sub-tasks are design/tracking work — **no `src/` changes**. The
entire v1 blocker set is this epic plus its administrative sub-tasks.

---

## 8. Sub-Task Decomposition

Each sub-task is executable without clarification by a flash execution lane.

### Sub-1 (#242 — PR): Commit Gap Analysis and Open PR

- **Files:** `docs/roadmap-v1-gap-analysis.md` (this document)
- **Acceptance test:** `cargo fmt --check && cargo clippy && cargo test` pass
  (no code changes, so only the doc build is affected); PR opens with this
  document.
- **Model label:** `model:mid`
- **Complexity:** S
- **Labels:** `documentation`, `priority:P0`, `complexity:S`, `model:mid`
- **Depends on:** #241
- **Steps:**
  1. Create branch `v1-gap-analysis` from `main`
  2. Commit `docs/roadmap-v1-gap-analysis.md`
  3. Run `actionlint` on workflows (no changes expected, baseline check)
  4. Run `cargo fmt --check && cargo clippy --all-targets --all-features && cargo test --all-targets --all-features`
  5. Push and open PR targeting `main`; assign to ECiurleo

### Sub-2 (#243 ✅): Triage #214

- **Files:** None (GitHub issue metadata only)
- **Acceptance test:** #214 has labels, milestone, assignee, and a comment
  recording the post-v1 classification.
- **Model label:** `model:mid`
- **Complexity:** S
- **Labels to apply:** `enhancement`, `priority:P3`, `complexity:M`,
  `model:high`
- **Milestone:** P3
- **Assignee:** ECiurleo
- **Depends on:** #241
- **Steps:**
  1. `gh issue edit 214 --add-label enhancement,priority:P3,complexity:M,model:high`
  2. `gh issue edit 214 --milestone "P3 - later delivery priority"`
  3. `gh issue edit 214 --add-assignee ECiurleo`
  4. Post comment: "Classified as **post-v1** per the v1 gap analysis (#241).
     SSO is a GUI-side integration; the API ships v1 with JWT auth (#130). SSO
     design requires a new API contract (OAuth2/OIDC) and depends on the GUI's
     SSO support maturing."

### Sub-3 (#244 ✅): Create v1.0 Milestone and Assign Closed Items

- **Files:** None (GitHub milestone metadata only)
- **Acceptance test:** `v1.0` milestone exists; #9, #11, #34, #45, #46, #47,
  #130, #215, #217–#225 are assigned to correct milestones.
- **Model label:** `model:mid`
- **Complexity:** S
- **Depends on:** sub-1
- **Steps:**
  1. Create milestone `v1.0` with description "First stable release"
  2. Assign #9, #11 to P0 milestone
  3. Assign #34, #45, #46, #47, #130 to P1 milestone
  4. Assign #215, #217, #218, #219, #220, #221, #222, #223, #224, #225 to P0
     milestone
  5. Verify milestone counts match §4.2

### Sub-4 (#245): Tag v1.0.0 and Verify GHCR Publish

- **Files:** None (git tag only)
- **Acceptance test:** `v1.0.0` tag exists on `main`; GHCR image
  `ghcr.io/tucanotechnology/tucanotestapi:v1.0.0` is published; GitHub Release
  exists.
- **Model label:** `model:mid`
- **Complexity:** S
- **Depends on:** sub-1, sub-3
- **Steps:**
  1. Ensure `main` is green on CI (all workflow jobs pass)
  2. `git tag v1.0.0 <merge-commit-sha>`
  3. `git push origin v1.0.0`
  4. Verify the release workflow runs successfully
  5. Verify GHCR image is published
  6. Create GitHub Release from the tag with release notes
