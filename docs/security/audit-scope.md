# Security Audit Scope, Methodology, and Severity Rubric (Issue #175)

Issue: [#175](https://github.com/TucanoTechnology/TucanoTestAPI/issues/175). Parent epic:
[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — *Carry out security audit*.

This document is the agreement that has to exist **before** the audit runs: what it covers, how a
finding is scored, what evidence a finding must carry, and what is explicitly not in scope. The
audit tasks [#176](https://github.com/TucanoTechnology/TucanoTestAPI/issues/176)–[#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179)
work to this document, and the triage task [#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180)
consumes its output. It adds no route, no field, and no stored-document change, and it ships no
application code.

It does not restate the trust model. The boundaries, assets, abuse cases, invariants, and known
limitations live in [threat-model.md](threat-model.md) and are cited here by name.

## 1. Objective and rules of engagement

The audit is an **external penetration test of this repository as deployed**: an adversary who can
reach the HTTP listener, supply any request body, upload any file, and read any corner of the data
volume is assumed. The audit produces tracked findings; it does not fix them. Fixes ship from the
remediation tickets #180 raises.

| Rule | Detail |
| --- | --- |
| Target | This repository at the merge-base commit the audit task records in its report. The audit never runs against a moving `main`. |
| Authority to test | Only the auditors' own throwaway deployment: a Compose project created for the audit, its own volume, and a free host port. See *Rules of engagement* below. |
| Evidence | Every finding carries the evidence named in [section 4](#4-required-finding-shape). A claim without a reproduction is not a finding. |
| Output | Findings recorded in the audit task's report PR under `docs/security/`, then triaged by #180 into fix tickets or accepted-risk records. |
| Non-goal | A narrative "security posture" document. The epic's goal is concrete, tracked issues; prose that raises no finding is not a deliverable. |
| Fixing | Out of scope for every audit task. An auditor who finds a fixable defect records it and continues; the fix is a separate PR from #180's tickets. |

### Rules of engagement

- **A throwaway deployment is mandatory.** Never point an audit at the instance the operator uses.
  On this machine the long-lived instance is the Compose project `tucano-test` (container
  `tucano-test-api-1`); it is **never** a test target, and it must not be stopped or rebuilt by an
  audit. Audits use their own Compose project name and a free port.
- **Data volumes are disposable.** An audit volume holds seeded, non-real data. If an audit must
  read a production-like volume, it reads a copy.
- **Denial-of-service tests are bounded and local.** Resource-exhaustion probes run only against the
  auditor's own throwaway container, never against a shared service, and never long enough to
  affect the host beyond that container's own limits.
- **Third-party systems are read-only.** The GitHub API, the container registry, and the RustSec
  advisory database are consulted, never written to.

## 2. In-scope surface

Four attack surfaces, mapping one-to-one onto the child tasks and onto the assets and trust
boundaries already named in [threat-model.md](threat-model.md).

| # | Surface | Audit task | What is examined |
| --- | --- | --- | --- |
| S1 | HTTP surface | [#176](https://github.com/TucanoTechnology/TucanoTestAPI/issues/176) | Every route in the served contract: authentication and authorization (authn/authz), object-level authorization and IDOR, identifier validation and path traversal, request-body and upload abuse, error-envelope and header disclosure, session and token lifecycle. |
| S2 | Storage and filesystem | [#177](https://github.com/TucanoTechnology/TucanoTestAPI/issues/177) | Path confinement and symlink handling, atomicity and durability of writes, the advisory lock, attachment and revision storage, file permissions on documents and on the authentication store, and the on-disk tree as an integrity boundary. |
| S3 | Container and deployment | [#178](https://github.com/TucanoTechnology/TucanoTestAPI/issues/178) | The image and its build, the Compose configuration and container hardening, secret delivery from outside the image, the volume as the only state, network exposure, and rollback to an immutable tag. |
| S4 | Dependencies and supply chain | [#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179) | The dependency inventory and its advisories, the CI supply-chain controls, action and image pinning, the SBOM, and the workflow permissions model. |

### Trust boundaries in scope

The audit tests the boundaries declared in
[threat-model.md § Trust boundaries](threat-model.md#trust-boundaries), and no others. They are,
verbatim in name:

1. HTTP client → service
2. JSON payload → domain model
3. Resource ID or filename → filesystem
4. Service → stored JSON
5. Attachment upload → storage
6. Configuration file → service startup
7. Configuration key → service startup
8. Service logs and audit events
9. GUI → storage

Boundary 9 is in scope **only** as the claim that the service provides no filesystem access path to
a client that is supposed to use the HTTP API. The GUI's own implementation is out of scope
([section 3](#3-out-of-scope)).

The [security invariants](threat-model.md#security-invariants) are the audit's acceptance criteria:
an invariant that can be violated in a deployed configuration is a finding, whatever the code
intends. The [known limitations](threat-model.md#known-limitations) are *already accepted* — an
auditor may re-argue one only by showing the stated impact is wrong (for example that a limitation
documented as denial of access is in fact an escalation). Re-deriving an accepted limitation without
new evidence is not a finding.

### Controls that already exist, and what "new" means

The controls CI enforces on every pull request and every push to `main` are **baselines, not
findings**. The audit's job on these is to test the control, not to rediscover it. The baseline is
the four jobs in [`.github/workflows/security.yml`](../../.github/workflows/security.yml) described
in [scanning-policy.md](scanning-policy.md):

| Baseline control | Job | An audit finding would be |
| --- | --- | --- |
| Dependency advisories (`cargo audit`) | `audit` | a vulnerable dependency the scan cannot see, or a scan that cannot fail the build |
| Secret scanning (`gitleaks`) | `secret-scan` | a secret the pattern set misses, or a scan that cannot fail the build |
| Container image scanning (`trivy`, CRITICAL/HIGH) | `container-scan` | an exploitable image vulnerability the scan misses, or a scan that cannot fail the build |
| SBOM generation (`cargo-cyclonedx`) | `sbom` | a document that is published without listing the crate's components |

The existing security test coverage ([`tests/security_tests.rs`](../../tests/security_tests.rs) for
traversal, symlink escape, and malformed input; [`tests/auth.rs`](../../tests/auth.rs) for the auth
matrix) is likewise a baseline. A finding is a *gap or bypass* — a path the tests do not exercise, a
control that is present but not reached, or a documented control that does not hold.

## 3. Out of scope

| Excluded | Why |
| --- | --- |
| The paired GUI repository, [TucanoTestGUI](https://github.com/TucanoTechnology/TucanoTestGUI) | A separate repository with its own review, CI, and issue tracker. Its own audit is its own ticket. Only boundary 9 is audited here, and only as the service's guarantee. |
| The human operator's long-lived deployment, and any real or production data | Not the auditor's system, and not a dataset an audit may touch. See *Rules of engagement*. |
| Third-party infrastructure: GitHub Actions runners, GHCR, the Docker registry, crates.io, the RustSec database, the deployment platform's ingress, TLS terminators, and DNS | Operated by third parties under their own security programmes. The audit examines only *how this repository uses them* — pinning, permissions, what is published — never the third party itself. |
| The container engine, the host kernel, the host filesystem, and the container runtime | Outside the trust boundary of the application. A host compromise is a precondition, not a finding, although the audit may record where the deployment relies on the host holding (for example shared-storage advisory locks). |
| Physical access, and a volume an attacker can both read and write | Explicitly out of the threat model and recorded as a known limitation: file encryption protects against accidents, not against an actor who can read the volume. |
| Social engineering, phishing, and the security of individual developer workstations | Not a property of the artifact under audit. |
| Compliance certification (SOC 2, ISO 27001, GDPR) | Not an audit deliverable; the epic raises technical findings. |
| Fixing anything | The audit reports; #180 triages; remediation tickets fix. |
| Denial of service that requires an unbounded target, or that measures the outcome rather than the control | Bounded probes of a declared limit are in scope (a body-size limit either refuses an oversize body or it does not). Saturating a shared host is not. |

## 4. Required finding shape

Every finding uses the template below **verbatim**, as one `###` section per finding inside the
audit task's report. A finding missing any required field is returned to the auditor rather than
triaged; that is deliberate, because a finding without a reproduction cannot be verified by the
person who fixes it.

```markdown
### F-<task>-<n>: <one-line title, imperative and specific>

- **Severity:** Critical | High | Medium | Low | Info
- **In scope:** S1 | S2 | S3 | S4 — and the trust boundary, by the name used in threat-model.md
- **Where:** `METHOD /route` for HTTP findings, or `path/to/file.rs:LINE` — file and line, so the
  fix lands on the code that is wrong; a class of defects names one representative site plus the
  enumeration that proves the class
- **Affected revision:** the full commit SHA the audit ran against
- **Reproduction:** the exact commands, request, or fixture, in fenced blocks, from a clean seeded
  deployment: the smallest sequence that shows the defect. Batched requests are fine; "see the
  audit" is not.
- **Observed:** what actually happened, quoting the real response, status code, error code, or
  on-disk result
- **Expected:** what should have happened, and the invariant, control, or threat-model row it
  violates (quote the invariant text)
- **Impact:** what an attacker gains, who is affected, and under which deployment configuration —
  with authentication off or on stated explicitly
- **Suggested fix:** the change that would close it, at the level of "confine X through Y" or
  "authorize Z against the project named in the body"; not a patch
- **CWE:** the identifier that fits, when one does
- **Duplicates / prerequisites:** the finding this one repeats or depends on, or "none"
```

Two evidence rules apply to every finding:

- **A finding is reproducible or it is not a finding.** The reproduction must run from a clean
  deployment of the recorded revision — seeded by the ticket's own steps or by a documented seed
  script — with no state left by a previous experiment.
- **Credit what already holds.** A control the audit tested *and could not break* is recorded as a
  one-line pass entry under the task's scope, with the test that proves it. The audit's value
  includes the evidence that a control works, and a fail-only report loses it.

## 5. Severity rubric

Severity is scored on two axes, **exploitability** and **impact**, and the score is the pair, not a
single intuition. Score the two axes first, then read severity off the matrix.

### Exploitability

| Level | Test |
| --- | --- |
| **Trivial** | The attacker needs no account and no special position, and the attack is a single request or file with no prerequisite state. |
| **Moderate** | The attacker needs a valid account, one granted role, or one prerequisite step; the attack is repeatable without special conditions. |
| **Difficult** | The attacker needs a privileged position (for example project ownership or system-admin authority), a race, a particular deployment configuration, or a chain of several conditions that must all hold. |

### Impact

| Level | Test |
| --- | --- |
| **Severe** | Loss or corruption of data the attacker does not own; execution of code or a command; extraction of a secret that grants standing access (the JWT signing secret, an account credential, the volume's contents). |
| **Moderate** | Disclosed data, tampered data, or lost availability **inside** the attacker's own authorization scope, or the defeat of one control that by itself grants nothing further. |
| **Limited** | A disclosure or an effect that is already public, requires conditions the deployment never meets, or is confined to the attacker's own session. |

### Severity matrix

| Exploitability \ Impact | Severe | Moderate | Limited |
| --- | --- | --- | --- |
| **Trivial** | **Critical** | **High** | **Medium** |
| **Moderate** | **High** | **Medium** | **Low** |
| **Difficult** | **Medium** | **Low** | **Info** |

Escalation and de-escalation, which must be stated in the finding when used:

- **Escalate one level** when the defect is remotely reachable in a *default* configuration (the
  shipped Compose stack, or `cargo run` with no environment set) and needs no unusual
  configuration. A defect on a route that is public by design is reachable by definition.
- **De-escalate one level** when the defect depends on a configuration the project does not
  recommend, or on a precondition the threat model already excludes.
- **Never de-escalate below the impact axis.** A severe impact stays at least Medium however
  difficult it is to reach.
- **Reserved identifiers.** `Critical` is reserved for the top row of the matrix with a `Severe`
  impact. Every downgrade or upgrade states its reason in the finding, so a later reader can
  disagree with the reason rather than with the number.

### Severity definitions and response targets

The definitions must agree with the response times the repository already commits to in
[scanning-policy.md § Vulnerability Response](scanning-policy.md#vulnerability-response).

| Severity | Definition | Response target |
| --- | --- | --- |
| **Critical** | Unauthenticated remote compromise, or extraction of a signing secret or credentials, with a severe impact on the whole installation. | Fix before any further deployment of the affected build; a release-level action. |
| **High** | A control the audit could break, with moderate-to-severe impact: cross-project data access, write outside the caller's scope, or credential disclosure. | Patch within 24 hours. |
| **Medium** | A real weakness whose impact is confined, needs privileges the attacker must already hold, or defeats a defence-in-depth control. | Patch within 7 days. |
| **Low** | A hardening gap, an information disclosure of little value, or a defect only reachable under an excluded precondition. | Patch within 30 days. |
| **Info** | An observation, a documentation or process gap, or a control the audit recommends adding; no exploitable defect today. | No response deadline; recorded so the decision is deliberate. |

### Worked examples

Both are **examples that calibrate the rubric, not findings**. They describe the *kind* of defect
each severity names, so that two audit tasks score the same defect the same way.

- **Critical.** With the shipped Compose configuration, `GET /openapi.json` is public by design, and
  suppose some route derived a filesystem path from a request field without confinement, so a single
  unauthenticated request read a file outside `TUCANO_DATA_DIR`. Exploitability *Trivial*, impact
  *Severe*, and reachable in the default configuration → **Critical**.
- **Info.** `scripts/clear-data.mjs` probes a fixed list of base URLs and sends no credentials, so
  against an installation with `TUCANO_AUTH_REQUIRED` on it cannot complete its deletions and must
  be pointed at a deployment without authentication or paired with a hand-supplied token. Nothing
  confidential is exposed — the script is limited to the caller's own authority — so this is a
  tooling observation → **Info**. (Recorded here as calibration; it is for #179 or #178 to confirm
  against the code, not a finding this document raises.)

## 6. How the audit tasks run

Every audit task [#176](https://github.com/TucanoTechnology/TucanoTestAPI/issues/176)–[#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179)
follows the same shape, so their reports can be compared and triaged together.

1. **Pin the revision.** Record the commit SHA the audit ran against.
2. **Stand up a throwaway target.** Its own Compose project and volume, on a free port, seeded to a
   known state, with both `TUCANO_AUTH_REQUIRED` off and on, because several threats only exist in
   one of the two modes.
3. **Enumerate before probing.** List the surface from the served contract and from the code, so
   coverage can be shown as a count rather than asserted.
4. **Test each threat-model abuse case** for the task's surfaces, plus the boundaries assigned to
   it, and note the ones that hold as pass entries.
5. **Record findings** in the shape of [section 4](#4-required-finding-shape), scored with
   [section 5](#5-severity-rubric).
6. **Report in a pull request** that adds the task's report under `docs/security/`, links the task
   and epic, states the revision and the pass entries, and lists the findings in severity order.
7. **Leave nothing running.** Tear the throwaway deployment and its volume down when the task ends.

The audit reads the contract from `openapi.json` and the served Swagger UI; it never restates a
route or schema from memory. Where the audit finds the implementation and the contract disagree,
that disagreement is itself a finding.

## 7. Maintaining this document

- This document is the agreement the audit tasks are held to. A change to it is a change to the
  rules mid-audit, so it is edited in the pull request that needs it, with the reason in the
  description and a comment on #175.
- If an audit shows the trust model itself is wrong — a boundary missing, an asset unlisted, an
  invariant untrue — the fix is an update to [threat-model.md](threat-model.md) in the same pull
  request, per the epic's Definition of Done.
- The severities in [section 5](#5-severity-rubric) and the response targets in
  [scanning-policy.md](scanning-policy.md) must stay consistent; a change to one is a change to both.
- The scope here was checked against the implementation at the merge-base of #175: the four
  surfaces are the four a reader reaches from the contract, both child tasks and surfaces cover the
  trust boundaries declared in the threat model, and no in-scope surface is left without an owner.
