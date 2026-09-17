# Security Audit S4 — Dependencies and Supply Chain (Issue #179)

Issue: [#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179). Parent epic:
[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — *Carry out security audit*.

This report runs surface **S4** of [audit-scope.md](audit-scope.md): *"The dependency inventory and
its advisories, the CI supply-chain controls, action and image pinning, the SBOM, and the workflow
permissions model."* It carries findings only; nothing here is fixed. Remediation belongs to the
tickets [#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180) raises.

- **Affected revision (the pinned target):** `61b02b92f9227190ded969a66f071f1ce4a8c3e0`
- **Method:** [audit-scope.md § 6](audit-scope.md#6-how-the-audit-tasks-run), steps 1–7.
- **Findings in severity order:** none at Medium or above; **one** Low and **four** Info.
- **Pass entries:** five, in the [Pass entries](#pass-entries) section.

## 1. Revision pinned

| Item | Value |
| --- | --- |
| Repository revision audited | `61b02b92f9227190ded969a66f071f1ce4a8c3e0` |
| `origin/main` at audit time | `61b02b92f9227190ded969a66f071f1ce4a8c3e0` (merge-base, so the report's tree is the audited tree) |
| `Cargo.lock` SHA-256 | `4cddc67a699e87847a5a1f9d5cba12c3d32f6c80c83e1c11459fd970064f78ab` |
| RustSec advisory database revision | `e2e640471715167f73e22eaf761f2e547adafeec` (2026-09-14), 1246 advisories |
| Crate dependencies scanned | 100 (`Cargo.lock`) |

## 2. Throwaway target (step 2)

Two of its own Compose projects, each with its own volume and its own free host port, both built
from the pinned revision. The operator's long-lived instance (Compose project `tucano-test`,
container `tucano-test-api-1`) was never a target and was not stopped or rebuilt.

| Arm | Compose project | Host port | `TUCANO_AUTH_REQUIRED` | Result |
| --- | --- | --- | --- | --- |
| Auth off | `audit-s4` | 3210 | `false` | `/health` → `{"status":"ok","storage":"filesystem"}`; `scripts/smoke.sh http://localhost:3210` → **PASS** (health, scratch project/case CRUD, both deletions observed) |
| Auth on | `audit-s4-kauth` | 3211 | `true` | `GET /health` → `200`; `GET /projects` → `401 {"code":"missing_token"}`; `POST /auth/login` → `200` with an access token, so the bootstrap account exists and the enforcing arm is reachable |

Both images are the local build of the pinned revision (`Config.User=tucano`,
`org.opencontainers.image.version=local`, image id
`sha256:247a6b40e98499454a6473da908f3ee8a76ac4ae367ec603401f04b7e1fe50ef`). Both containers ran
with `ReadonlyRootfs=true`, `no-new-privileges:true`, and `/tmp` as a tmpfs.

## 3. Surface enumerated before probing (step 3)

S4 is not a route surface, so the enumeration is the supply-chain inventory rather than a route
list. Everything below was read from the pinned revision, not from memory.

**Contract check.** The served contract (`openapi.json`, OpenAPI 3.0.3) declares **67 operations
across 46 paths**. Five operations are public by design and carry `security: []` — `GET /health`,
`GET /openapi.json`, `GET /api-docs`, `POST /auth/login`, `POST /auth/refresh` — which matches
[authentication-decision.md](authentication-decision.md). The remaining 62 carry `bearerAuth`. S4
found no disagreement between the contract and the implementation.

**Dependency inventory.** 100 packages in `Cargo.lock`; 13 direct dependencies and 3 dev-dependencies
in `Cargo.toml`.

**CI supply-chain inventory.**

| Item | Count | Pinning |
| --- | --- | --- |
| Distinct Actions used | 8 | all by mutable major/version tag |
| `uses:` call sites | 14 | **0** pinned by commit SHA |
| Docker images used by workflows | 5 | 2 by exact version tag; 1 by tag; 2 by digest (see F-179-1) |
| Container base images (Dockerfile) | 2 | 1 by tag, 1 by digest |
| Provenance / attestation / signing steps | 0 | — |

The 14 call sites, in full, so the class is closed rather than sampled:

```
.github/workflows/lint.yml:15         actions/checkout@v5
.github/workflows/lint.yml:30         actions/checkout@v5
.github/workflows/security.yml:34     actions/checkout@v5
.github/workflows/security.yml:43     actions/checkout@v5
.github/workflows/security.yml:52     actions/checkout@v5
.github/workflows/security.yml:55     aquasecurity/trivy-action@v0.36.0
.github/workflows/security.yml:81     actions/checkout@v5
.github/workflows/security.yml:114    actions/upload-artifact@v5
.github/workflows/release.yml:21      actions/checkout@v5
.github/workflows/release.yml:22      docker/setup-buildx-action@v3
.github/workflows/release.yml:23      docker/login-action@v3
.github/workflows/release.yml:28      docker/metadata-action@v5
.github/workflows/release.yml:35      docker/build-push-action@v6
.github/workflows/auto-merge.yml:26   actions/github-script@v7
```

**Workflow permissions model.**

| Workflow | `permissions` | Trigger |
| --- | --- | --- |
| `lint.yml` | `contents: read` | `pull_request`, `push` to `main`/`feature/**` |
| `docs.yml` | `contents: read` | same |
| `build-test.yml` | `contents: read` | same |
| `security.yml` | `contents: read`, `actions: write` | same, plus weekly `cron: "17 3 * * 1"` |
| `release.yml` | `contents: read`, `packages: write` | `push` to `main` and `v*.*.*` tags |
| `auto-merge.yml` | `contents: write`, `pull-requests: write`, `checks: read` | `workflow_run` on Lint/Docs/Build and Test/Security completion |

`release.yml` publishes to `ghcr.io/${{ github.repository }}` (`ghcr.io/tucanotechnology/tucanotestapi`)
with `docker/build-push-action@v6`, `push: true`, `tags` from `docker/metadata-action@v5`
(`type=ref,event=tag` and `type=raw,value=build-${{ github.run_number }}`), and passes
`build-args: BUILD_NUMBER=${{ github.run_number }}`. It passes no `provenance:`, `sbom:`,
`attestations:` or `id-token` permission. No workflow references `sigstore`, `cosign`,
`attest`, or `id-token` anywhere in `.github/`.

**SBOM inventory.** `.github/workflows/security.yml` job `sbom` generates `sbom.json` with
`cargo cyclonedx --format json --override-filename sbom`, validates it with an inline `python3`
step, and uploads it with `actions/upload-artifact@v5` (`retention-days: 90`). Nothing outside
`.github/` refers to that artifact.

## 4. Findings

One finding was raised. Four observations are recorded as Info because they are decisions worth
making deliberately rather than defects: in each case the impact axis is *Limited* — the effect is a
documentation or process gap, already-public information, or a control whose defeat grants nothing
further on its own.

### F-179-1: Digest-pin both Dockerfile stages so the audited image is the built image

- **Severity:** Low
- **In scope:** S4 — *HTTP client → service* and *Service logs and audit events* are not touched; the
  boundary this finding reaches is the artifact itself, examined under S4's "action and image
  pinning". The image is the thing every other boundary's controls are carried by, and
  [deployment-guide.md](../deployment/deployment-guide.md) makes an image tag the audit identity for
  rollback ("Rollback means re-deploying the previously recorded **immutable** image tag").
- **Where:** `Dockerfile:1` (builder stage, tag-pinned) versus `Dockerfile:9` (runtime stage,
  digest-pinned). The class has exactly two members, so the enumeration is the file's two `FROM`
  lines.
- **Affected revision:** `61b02b92f9227190ded969a66f071f1ce4a8c3e0`
- **Reproduction:** build the image twice, weeks apart, from the same revision, and compare the
  builder stage's resolved base image.

  ```console
  $ sed -n '1p;9p' Dockerfile
  FROM rust:1.98.0-slim-trixie AS builder
  FROM debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f

  $ docker build --file Dockerfile --tag tucano-test:PIN-CHECK .
  $ docker image inspect rust:1.98.0-slim-trixie --format '{{.Id}}'
  ```

  Re-run the last two commands after the `rust` maintainers republish `1.98.0-slim-trixie`; the
  `FROM` line is unchanged in the repository, but the resolved image id has moved.
- **Observed:** the builder stage names a mutable tag and resolves it at build time, so
  `rust:1.98.0-slim-trixie` can be republished under the same tag with different content. The
  runtime stage is pinned correctly:

  ```
  FROM rust:1.98.0-slim-trixie AS builder
  FROM debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f
  ```

  The comment immediately above the runtime `FROM` states the intent for the file as a whole —
  *"The base image is pulled by digest rather than by tag so the cache key is the published image
  instead of whatever happens to be cached locally"* — but only the second stage does it. The
  builder is additionally the stage that runs `cargo build --release` over the entire dependency
  graph, so it is also the stage with the most to gain from a digest pin.
- **Expected:** every base image in the build names an immutable identity, so that "build the
  recorded revision" and "audit the built image" are the same statement. The control this is
  measured against is the abuse-case row *"Vulnerable dependency or image | Run advisory, secret,
  and container scans in CI | Security workflow and clean-baseline checks"*
  ([threat-model.md](../security/threat-model.md) — cited from this report's own directory as
  `threat-model.md`). That row is satisfied by the scans; this finding is the pinning half of S4's
  scope, which the same scope sentence names separately ("action and image pinning").
- **Impact:** an attacker who can republish or compromise the `rust:1.98.0-slim-trixie` tag controls
  the toolchain that compiles every dependency — the strongest position in a supply-chain attack,
  since it can alter the artifact without altering any pinned crate. The image the operator deploys
  is then not the image the recorded revision produced, which defeats the rollback identity the
  deployment guide promises. Who is affected: anyone deploying this image. Deployment configuration:
  independent of `TUCANO_AUTH_REQUIRED`; auth on or off makes no difference, because the compromise
  happens before the service exists. The precondition is that the attacker already controls the
  `rust` tag on Docker Hub, which is the difficulty that keeps this out of higher severity.
- **Suggested fix:** pin `rust:1.98.0-slim-trixie` by digest, the same way the runtime stage is
  pinned, and keep the digest current with the same deliberate-bump discipline the file already
  applies to `CACHE_BUSTER`. The workflow image references deserve the same treatment: see F-179-2.
- **CWE:** CWE-494 (Download of Code Without Integrity Check); CWE-1357 (Reliance on Insufficiently
  Trustworthy Control Sphere) for the tag-swap case.
- **Duplicates / prerequisites:** none. Distinct from F-179-2, which is the same class in the
  workflow files; the two are listed separately because they are fixed in different files by
  different changes, and #180 may raise one ticket with both.

### F-179-2: Pin every CI action and image by commit SHA or digest

- **Severity:** Info
- **In scope:** S4 — the CI supply-chain controls, taken at the point where the repository uses
  third-party code.
- **Where:** all 14 `uses:` call sites listed in [section 3](#3-surface-enumerated-before-probing-step-3),
  and the five Docker images the workflows run. Representative site:
  `.github/workflows/security.yml:43`; the enumeration above proves the class is 14 of 14.
- **Affected revision:** `61b02b92f9227190ded969a66f071f1ce4a8c3e0`
- **Reproduction:**

  ```console
  $ grep -rho 'uses: [^ ]*' .github/workflows/ | sort | uniq -c
        7 uses: actions/checkout@v5
        1 uses: actions/github-script@v7
        1 uses: actions/upload-artifact@v5
        1 uses: aquasecurity/trivy-action@v0.36.0
        1 uses: docker/build-push-action@v6
        1 uses: docker/login-action@v3
        1 uses: docker/metadata-action@v5
        1 uses: docker/setup-buildx-action@v3

  $ grep -rEn 'uses: *[^ ]+@[0-9a-f]{40}' .github/workflows/ || echo "no SHA pins"
  no SHA pins
  ```

- **Observed:** every action is consumed through a mutable tag. `@v5`, `@v7`, `@v0.36.0`, `@v6`,
  `@v3` are all mutable; `@v0.36.0` is a version rather than a moving major, but it is still a tag
  GitHub will let the owner repoint. Zero of the 14 call sites name a commit SHA. Two of the five
  workflow images are pinned well — `rhysd/actionlint:1.7.12` and `zricethezav/gitleaks:v8.9.0` — and
  `rust:1.98.0-bookworm` is version-exact; nothing is digest-pinned.
- **Expected:** supply-chain tooling consumes actions and images by immutable identity, so that the
  reviewed workflow is the workflow that runs. Source: S4's "action and image pinning"
  ([audit-scope.md](audit-scope.md) § 2).
- **Impact:** an attacker who can move a tag, or who compromises an action's upstream repository,
  executes code inside the runner on every pull request and every push to `main`, with whatever
  permissions that job holds. The severity axis here is *Limited*, not *Moderate*, because the
  attacker must first control a tag in a well-known action repository, and because the jobs that
  carry the most authority (`release.yml`: `packages: write`; `auto-merge.yml`: `contents: write`)
  are separately constrained — see F-179-3 — so defeating an action pin alone grants nothing further.
  Deployment configuration: independent of `TUCANO_AUTH_REQUIRED`.
- **Suggested fix:** pin each `uses:` to a full commit SHA with the tag retained as a trailing
  comment, and pin the workflow images by digest; adopt a scheduled bump so pins do not rot.
  `docs/architecture/rust-service-core.md` already lists `cargo-deny` policy checks as outstanding,
  which is a natural place to enforce this.
- **CWE:** CWE-494; CWE-1357.
- **Duplicates / prerequisites:** same class as F-179-1, different files; either may be fixed alone.

### F-179-3: State the auto-merge PAT's authority in the threat model, or drop it

- **Severity:** Info
- **In scope:** Information / no trust boundary — this is an operational control outside the nine
  boundaries, examined under S4's "workflow permissions model".
- **Where:** `.github/workflows/auto-merge.yml:26` (`github-token: ${{ secrets.AUTO_MERGE_TOKEN || github.token }}`),
  and `.github/workflows/auto-merge.yml:4-7` (the job's own `permissions:` block).
- **Affected revision:** `61b02b92f9227190ded969a66f071f1ce4a8c3e0`
- **Reproduction:**

  ```console
  $ sed -n '4,7p;18,28p' .github/workflows/auto-merge.yml
  permissions:
    contents: write
    pull-requests: write
    checks: read
  ...
    name: Merge owner pull requests
    if: github.event.workflow_run.event == 'pull_request'
    ...
      - name: Merge when every required check has passed
        uses: actions/github-script@v7
        with:
          # A personal access token belonging to the owner can merge past the
          # required review; the built-in token cannot, but it still beats an
          # unset secret, which fails the step before it runs a single call.
          github-token: ${{ secrets.AUTO_MERGE_TOKEN || github.token }}
  ```

- **Observed:** the workflow's own comment states the intent plainly: the PAT *"can merge past the
  required review"*. The `github.token` fallback cannot. So the behaviour of the merge gate depends
  on a repository secret whose value and scopes are invisible in the repository, and the stronger
  branch — the one that actually merges owner pull requests — is the one that bypasses the review
  requirement.
- **Expected:** a control this consequential is described where the project describes its controls.
  [threat-model.md](threat-model.md) names no boundary, abuse case, or known limitation covering the
  merge path, so a reader of the threat model does not learn that a PAT with review-bypassing
  authority stands in the merge path. Recorded as Info rather than a defect: the compensating
  controls are real and were tested — the workflow merges only when the PR head is in the same
  repository, the base is the default branch, the author's collaborator permission is exactly
  `admin`, the PR is not a draft, it is mergeable and not `behind`, and every check run reported
  `success`, `skipped`, or `neutral`. The approval requirement is a process control the project may
  have decided deliberately not to apply to its own owner.
- **Impact:** a reader or auditor cannot tell from the repository what the auto-merge path is able
  to do; if the PAT is over-scoped, the repository's required review is decorative for PRs opened by
  the owner account. Nothing is exposed to an unauthenticated attacker, and no data is reachable
  through it, which is why this does not rise above Info.
- **Suggested fix:** either replace the PAT with the built-in token and accept that owner PRs need a
  manual approval, or add a row to [threat-model.md](threat-model.md) recording that the merge path
  runs with owner-scoped authority and is a deliberate trade for an owner-operated repository.
- **CWE:** CWE-1357; CWE-284 (Improper Access Control) if the PAT is broader than the workflow needs.
- **Duplicates / prerequisites:** none.

### F-179-4: Reconcile the dependency-pinning claim with `Cargo.toml`

- **Severity:** Info
- **In scope:** S4 — the dependency inventory and its advisories; the divergence is between the
  documented control and the manifest.
- **Where:** `docs/security/scanning-policy.md:38` versus `Cargo.toml:9-21`.
- **Affected revision:** `61b02b92f9227190ded969a66f071f1ce4a8c3e0`
- **Reproduction:**

  ```console
  $ sed -n '38p' docs/security/scanning-policy.md
  - All dependencies are pinned to specific versions in `Cargo.toml`

  $ sed -n '9,21p' Cargo.toml
  tracing = { version = "0.1", default-features = false, features = ["std"] }
  ...
  argon2 = "0.5"
  base64 = "0.22"
  hmac = "0.12"
  rand = "0.8"
  sha2 = "0.10"
  ```

- **Observed:** six direct dependencies are specified as caret ranges (`tracing = "0.1"`,
  `argon2 = "0.5"`, `base64 = "0.22"`, `hmac = "0.12"`, `rand = "0.8"`, `sha2 = "0.10"`), which
  resolve to whatever the newest compatible release is at lock time: `tracing` 0.1.44, `argon2`
  0.5.3, `base64` 0.22.1, `hmac` 0.12.1, `rand` 0.8.8, `sha2` 0.10.9. The policy sentence above them
  says the opposite.
- **Expected:** the documented control and the manifest agree. The two statements are not equally
  strong claims: `Cargo.lock` **is** committed, which is what makes builds reproducible, so the
  policy sentence is over-broad rather than the manifest being wrong.
- **Impact:** an auditor or a maintainer reading the policy would conclude that a reviewed PR is the
  only way a dependency version changes. In fact `cargo update` inside a caret range changes a
  version without any `Cargo.toml` line changing, so the review-diff signal the policy promises is
  absent for those six crates. No dependency is currently unpatched — see the
  [pass entry](#pass-entries) for the clean `cargo audit` run — so nothing exploitable follows from
  this today.
- **Suggested fix:** narrow the policy sentence to what is true (versions are pinned exactly where
  the manifest says so, and the committed `Cargo.lock` is the enforcement for the rest), or make it
  true by converting the caret ranges to exact versions. The `cargo-deny` check that
  `docs/architecture/rust-service-core.md` already lists as outstanding would let the policy be
  enforced rather than asserted.
- **CWE:** CWE-1104 (Use of Unmaintained Third-Party Components) is adjacent; no single CWE fits a
  documentation/manifest divergence, and none is claimed.
- **Duplicates / prerequisites:** none.

### F-179-5: Run the secret scan over history, or correct the claim that it does

- **Severity:** Info
- **In scope:** S4 — the CI supply-chain controls; specifically the `secret-scan` baseline.
- **Where:** `.github/workflows/security.yml:47` versus `docs/security/scanning-policy.md:17`.
- **Affected revision:** `61b02b92f9227190ded969a66f071f1ce4a8c3e0`
- **Reproduction:**

  ```console
  $ sed -n '43,47p' .github/workflows/security.yml
        - uses: actions/checkout@v5
          with:
            fetch-depth: 0
        - name: Scan repository for secrets
          run: docker run --rm -v "$PWD:/repo:ro" zricethezav/gitleaks:v8.9.0 detect --source /repo --no-git --redact

  $ sed -n '17p' docs/security/scanning-policy.md
  - Scans repository history for accidentally committed secrets
  ```

- **Observed:** the job checks out the **full** history (`fetch-depth: 0`), which is only meaningful
  if history is going to be scanned, and the next step passes `--no-git`, which tells gitleaks to
  walk the working tree rather than the commits. Verified in the throwaway target of [section
  2](#2-throwaway-target-step-2): the same invocation against a directory whose only secret lives in
  a past commit reports `INF no leaks found` and exits `0`, while the same invocation against a
  directory whose working tree holds the secret reports `WRN leaks found` and exits `1`. The
  `fetch-depth: 0` line is therefore doing work nothing consumes.
- **Expected:** the scan covers what the policy says it covers, or the policy says what the scan
  covers. Source: S4's "CI supply-chain controls", and the abuse-case row *"Vulnerable dependency or
  image | Run advisory, secret, and container scans in CI"* ([threat-model.md](threat-model.md)).
- **Impact:** a secret committed once and then deleted in a later commit stays in the repository's
  history — where `git clone` still delivers it — and no scheduled or PR job looks there. The
  severity axis is *Limited*: the exposure is already public to anyone who can clone the repository,
  the scan is a detection control rather than a confinement control, and nothing an attacker gains
  from finding a historical secret is granted by the workflow itself.
- **What was *not* found, and must not be claimed:** the pinned detector is not stale and not blind.
  The probe described above found a canonical AWS credential pair and a `ghp_…` GitHub token in a
  working tree with default rules and no configuration file. An earlier mid-audit hypothesis that
  v8.9.0's default rule set had gone stale was tested against a pristine fixture directory and
  **disproved** — and the one anomalous "no leaks found" result that prompted it turned out to be an
  artifact of the fixture's file mode, not of the detector: gitleaks v8.9.0's walker skips a file it
  cannot open, and that fixture's secret files were `0600` under a different uid. With the mode
  corrected, the same fixture reported `WRN leaks found: 3`. The repository itself scans clean (see
  the [pass entry](#pass-entries)).
- **Suggested fix:** drop `--no-git` so the job scans history, and keep `--redact`; or, if scanning
  only the tree is the intended cost/benefit, correct the policy sentence and drop the now-pointless
  `fetch-depth: 0`. Note that history mode needs a `.git` the container can traverse, which the
  workflow's `"$PWD:/repo:ro"` mount provides on the runner but which the manual command in
  `scanning-policy.md` § "Testing Security Controls" does not evidence.
- **CWE:** CWE-615 (Inclusion of Sensitive Information in Source Code Comments) is not right; the
  fitting one is CWE-538 (Insertion of Sensitive Information into Externally-Accessible File or
  Directory) for the underlying exposure, with the finding itself being a coverage gap.
- **Duplicates / prerequisites:** relates to the `secret-scan` baseline in
  [audit-scope.md](audit-scope.md) § "Controls that already exist" — the baseline is cited there as
  existing, and this finding is the *gap* that baseline table asks for, not a rediscovery of the
  control.

## 5. Pass entries

Controls the audit tested and could not break. Each entry names the test that proves it.

- **Dependency advisories are current and the scan can fail the build.** `cargo audit` at the pinned
  revision, against advisory database `e2e64047…` (2026-09-14, 1246 advisories), over all 100
  `Cargo.lock` dependencies: no vulnerabilities, exit `0`. The `audit` job runs
  `cargo install cargo-audit --locked && cargo audit`, and `cargo audit` exits non-zero on a finding,
  so the job fails with it.
- **The repository's working tree carries no detected secret.** The exact CI invocation —
  `docker run --rm -v "$PWD:/repo:ro" zricethezav/gitleaks:v8.9.0 detect --source /repo --no-git --redact`
  — against the pinned revision: `INF no leaks found`, exit `0`. The same image and invocation
  against a fixture holding a canonical AWS key pair and a `ghp_…` token: `WRN leaks found`, exit
  `1`, so the control's detection path is live and the job's non-zero exit is reachable.
- **The SBOM job cannot publish an empty or malformed document.** `cargo cyclonedx --format json
  --override-filename sbom` writes `sbom.json`; the inline `python3` step then exits non-zero unless
  the file exists, is non-empty, parses as JSON, reports `bomFormat == "CycloneDX"`, and lists at
  least one component, printing the spec version and component count otherwise. This is a genuine
  control that holds — it was written in response to a real 0-byte artifact (#139), and it addresses
  exactly the failure mode [audit-scope.md](audit-scope.md) § "Controls that already exist" calls a
  finding for this job ("a document that is published without listing the crate's components").
- **No workflow grants a write scope broader than its job needs, and none requests an OIDC token.**
  `lint.yml`, `docs.yml`, `build-test.yml` and `security.yml` hold `contents: read` (`security.yml` adds only `actions: write`, for
  the artifact upload); `release.yml` adds only `packages: write`, for the GHCR push; no workflow
  declares `id-token: write` or any other elevated scope. `auto-merge.yml`'s broader
  `contents: write` / `pull-requests: write` is examined on its own in F-179-3, and its trigger is
  constrained to same-repository, default-branch, admin-authored, non-draft, mergeable PRs whose
  every check run has passed.
- **The release path publishes to the expected registry with the two documented tag forms.** A grep
  of `.github/` for `sigstore|cosign|attest|id-token` returns nothing beyond the `sbom:` job key, and
  `release.yml` passes none of `provenance:`, `sbom:`, or `attestations:` to
  `docker/build-push-action@v6`; `docker/metadata-action@v5` produces `type=ref,event=tag` and
  `type=raw,value=build-${{ github.run_number }}`, and `build-args: BUILD_NUMBER=${{
  github.run_number }}` is what lands in `org.opencontainers.image.version`. Anything asserted about
  *published* artifacts was read from the workflow definition and from the local build of the pinned
  revision, never from the live registry (per [audit-scope.md](audit-scope.md) § 1, third-party
  systems are read-only, and the pinned revision's tags exist only if that revision was released).

## 6. Calibration confirmed

- **`scripts/clear-data.mjs`** — the Info-level calibration recorded in
  [audit-scope.md](audit-scope.md) § 5 and left to "**#179** or #178 to confirm against the code".
  Confirmed against the code at the pinned revision: the script's `resolveBaseUrl()` probes a fixed
  candidate list in order — `argv[2]`, `API_URL`, `TUCANO_API_URL`, `http://localhost:3100`,
  `http://localhost:8080/api`, `http://localhost:3000` — and falls back to the first candidate when
  none answers `/health`; `api()` sends no `Authorization` header; it never touches configurations,
  accounts, or grants. The calibration's stated consequence therefore holds: against an installation
  with `TUCANO_AUTH_REQUIRED` on it cannot complete its deletions, and on auth off it can delete
  every milestone, run, suite, project, and case the fallback URL answers for. Recorded as
  calibration confirmed, **not** raised as a new finding.

## 7. Tear-down (step 7)

Both throwaway Compose projects and both volumes were removed at the end of the audit
(`docker compose -p audit-s4 down -v` and `docker compose -p audit-s4-kauth down -v`), together with
the two local images built for it (`sha256:247a6b40…` and `sha256:b1d957a2…`) and the scratch
directory that held their volumes. The operator's long-lived Compose project `tucano-test` and its
container `tucano-test-api-1` were never a target, were never stopped, and were never rebuilt; it
reported `Up` before, during, and after the audit.

One detail worth recording for whoever runs the next audit, because it cost this one a debugging
cycle and is not a property of the application: the container runs as uid 10001, so files it creates
in a bind-mounted volume are owned by uid 10001 and cannot be removed by the host user that owns the
directory. `rm -rf` on the scratch path fails on exactly those files. Removing them needs either
`sudo` or a throwaway container with the volume mounted, which is what this audit used.
