# Consolidated Security Audit Summary — S1–S4 (Epic #166)

This is the consolidated summary of the security audit carried out under epic
[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166). It lists every finding from
the four surface reports with its severity, the remediation ticket that now owns it, and that
ticket's status, so a reader can see the audit's outcome and what remains open without opening the
four full reports.

- **Method and severity rubric:** [audit-scope.md](audit-scope.md) (#175); the shared design brief
  for S1–S3 is [audit-design-176-178.md](audit-design-176-178.md).
- **Triage record:** the finding → ticket mapping was fixed by
  [#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180) (triage comment of
  2026-09-24), which raised the 17 remediation tickets
  [#320](https://github.com/TucanoTechnology/TucanoTestAPI/issues/320)–[#336](https://github.com/TucanoTechnology/TucanoTestAPI/issues/336).
- **Status snapshot:** 2026-09-24. Every ticket state below was read from the GitHub API on that
  date; re-check before relying on it.

**Status legend:** *merged* — the remediation landed on `main` and the ticket is closed;
*open* — the ticket is open and work remains; *decided* — the disposition is an explicit recorded
decision or accepted risk rather than a code fix. No finding is in the *decided* state at this
snapshot: every finding is either remediated or tracked by an open fix ticket.

## Totals

| | Critical | High | Medium | Low | Info | Total |
| --- | --- | --- | --- | --- | --- | --- |
| Findings | 1 | 2 | 3 | 8 | 6 | **20** |
| Remediated (ticket merged, or fixed pre-triage) | 1 | 2 | 3 | 8 | 2 | **16** |
| Remaining (open ticket) | 0 | 0 | 0 | 0 | 4 | **4** |

The one Critical (F-178-1) and both Highs (F-176-1, F-177-6) are remediated; so is every Medium and
every Low. The four remaining findings are all Info. Open remediations at this snapshot:
[#331](https://github.com/TucanoTechnology/TucanoTestAPI/issues/331) (F-178-5),
[#333](https://github.com/TucanoTechnology/TucanoTestAPI/issues/333) (F-179-2),
[#334](https://github.com/TucanoTechnology/TucanoTestAPI/issues/334) (F-179-5),
[#336](https://github.com/TucanoTechnology/TucanoTestAPI/issues/336) (F-179-3). Separately, the
PR-time container-scan control that F-178-4's remediation relies on is temporarily out of service
(PR [#349](https://github.com/TucanoTechnology/TucanoTestAPI/pull/349)) and its restoration is
tracked by [#362](https://github.com/TucanoTechnology/TucanoTestAPI/issues/362) — see
[Defects noticed during remediation](#defects-noticed-during-remediation).

## S1 — HTTP surface (#176)

Report: [audit-s1-http-surface.md](audit-s1-http-surface.md), pinned at revision
`096835bc108fae2428786c7900b5cf02e92a5dcf`. Ticket:
[#176](https://github.com/TucanoTechnology/TucanoTestAPI/issues/176). 3 findings.

| Finding | Severity | Summary | Remediation | Status |
| --- | --- | --- | --- | --- |
| **F-176-1** | High | Over-deep element nesting in the JUnit import overflows the worker stack and aborts the process — remote denial of service on the whole installation (CWE-674) | [#320](https://github.com/TucanoTechnology/TucanoTestAPI/issues/320) | merged |
| **F-176-2** | Medium | Concurrent identical creates each answer `201` while N−1 documents are silently discarded — the create path checks existence before taking the lock | [#322](https://github.com/TucanoTechnology/TucanoTestAPI/issues/322) | merged |
| **F-176-3** | Low | Over-long identifiers and names fail as `500 storage_error` instead of a `400` client error | [#324](https://github.com/TucanoTechnology/TucanoTestAPI/issues/324) (merged with F-177-4: one root cause) | merged |

## S2 — Storage and filesystem (#177)

Report: [audit-s2-storage-and-filesystem.md](audit-s2-storage-and-filesystem.md), pinned at revision
`c5e99431389854368ab3a8e07003622f34dfdd21`. Ticket:
[#177](https://github.com/TucanoTechnology/TucanoTestAPI/issues/177). 7 findings.

| Finding | Severity | Summary | Remediation | Status |
| --- | --- | --- | --- | --- |
| **F-177-1** | Low | Every stored document, attachment, revision and auth-store file is created world-writable `0o666` instead of owner-only | [#325](https://github.com/TucanoTechnology/TucanoTestAPI/issues/325) | merged |
| **F-177-2** | Low | A stored document is served without shape validation, so a tampered or corrupted file reaches clients unchecked | [#326](https://github.com/TucanoTechnology/TucanoTestAPI/issues/326) | merged |
| **F-177-3** | Medium | A concurrent write acknowledged with `200` can be silently discarded — the unlocked `exists_at`/write race (re-graded from Low during the audit) | [#323](https://github.com/TucanoTechnology/TucanoTestAPI/issues/323) | merged |
| **F-177-4** | Low | An identifier longer than the filesystem's name limit is accepted and then fails as `500 storage_error` | [#324](https://github.com/TucanoTechnology/TucanoTestAPI/issues/324) (merged with F-176-3) | merged |
| **F-177-5** | Medium | A refused configuration file prints the error's `Debug` rendering, so the refusal names neither the setting nor the file | [#321](https://github.com/TucanoTechnology/TucanoTestAPI/issues/321) (merged with F-177-6: same file pair, one fix) | merged |
| **F-177-6** | High | A configuration value whose JSON type contradicts its field is echoed verbatim in the startup error | [#321](https://github.com/TucanoTechnology/TucanoTestAPI/issues/321) (merged with F-177-5) | merged |
| **F-177-7** | Low | A hardlink is served as an attachment because confinement is a path check, which cannot see a second name for the same inode | [#327](https://github.com/TucanoTechnology/TucanoTestAPI/issues/327) | merged |

## S3 — Container and deployment (#178)

Report: [audit-s3-container-and-deployment.md](audit-s3-container-and-deployment.md), pinned at
revision `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`. Ticket:
[#178](https://github.com/TucanoTechnology/TucanoTestAPI/issues/178). 5 findings.

| Finding | Severity | Summary | Remediation | Status |
| --- | --- | --- | --- | --- |
| **F-178-1** | Critical | The shipped Compose file publishes the API on every interface while leaving authentication off, so any host that can route to the port holds full anonymous authority | Fixed by PR [#278](https://github.com/TucanoTechnology/TucanoTestAPI/pull/278) (merged 2026-09-17, before triage — no remediation ticket was raised); the shipped stack now sets `TUCANO_AUTH_REQUIRED=true` by default | merged |
| **F-178-2** | Low | The documented Compose rollback cannot restore a real previous image — no image id is recorded | [#329](https://github.com/TucanoTechnology/TucanoTestAPI/issues/329) | merged |
| **F-178-3** | Low | The container keeps the default capability set; nothing is dropped | [#328](https://github.com/TucanoTechnology/TucanoTestAPI/issues/328) | merged |
| **F-178-4** | Info | CI scans the locally built image, not the artifact that is actually published | [#330](https://github.com/TucanoTechnology/TucanoTestAPI/issues/330); note the PR-time `container-scan` job is temporarily skipped by PR [#349](https://github.com/TucanoTechnology/TucanoTestAPI/pull/349) and [#362](https://github.com/TucanoTechnology/TucanoTestAPI/issues/362) tracks re-enabling it | merged |
| **F-178-5** | Info | The SBOM is generated in CI but not published with the artifact, and no rationale is recorded | [#331](https://github.com/TucanoTechnology/TucanoTestAPI/issues/331) | open |

## S4 — Dependencies and supply chain (#179)

Report: [audit-s4-dependencies-and-supply-chain.md](audit-s4-dependencies-and-supply-chain.md),
pinned at revision `61b02b92f9227190ded969a66f071f1ce4a8c3e0`. Ticket:
[#179](https://github.com/TucanoTechnology/TucanoTestAPI/issues/179). 5 findings.

| Finding | Severity | Summary | Remediation | Status |
| --- | --- | --- | --- | --- |
| **F-179-1** | Low | The Dockerfile's builder stage is not digest-pinned, so the audited image is not provably the built image | [#332](https://github.com/TucanoTechnology/TucanoTestAPI/issues/332) | merged |
| **F-179-2** | Info | CI actions and images are referenced by mutable tag, not pinned by commit SHA or digest | [#333](https://github.com/TucanoTechnology/TucanoTestAPI/issues/333) | open |
| **F-179-3** | Info | The auto-merge PAT's authority is not stated in the threat model | [#336](https://github.com/TucanoTechnology/TucanoTestAPI/issues/336) (needs an owner decision: state the authority, or drop the PAT) | open |
| **F-179-4** | Info | The scanning policy's dependency-pinning claim contradicts `Cargo.toml` | [#335](https://github.com/TucanoTechnology/TucanoTestAPI/issues/335) | merged |
| **F-179-5** | Info | The secret scan claims to cover history but runs with `--no-git`, so it scans the working tree only | [#334](https://github.com/TucanoTechnology/TucanoTestAPI/issues/334) | open |

## Defects noticed during remediation

These are not audit findings: they were discovered while the remediation tickets above were being
implemented, and are tracked so the board is complete.

| Ticket | Severity | Summary | Noticed during | Status |
| --- | --- | --- | --- | --- |
| [#362](https://github.com/TucanoTechnology/TucanoTestAPI/issues/362) | P3 | The `container-scan` and `sbom` PR jobs were temporarily skipped (PR [#349](https://github.com/TucanoTechnology/TucanoTestAPI/pull/349), runner saturation) and their required status checks removed from `main`; nothing blocks a PR with an unscanned image until both are restored | The #349 disable, which took out the control F-178-4's remediation (#330) relies on at PR time | open |
| [#363](https://github.com/TucanoTechnology/TucanoTestAPI/issues/363) | P3 | A missing build digest degenerates the release-scan image reference to `…@` and fails trivy with an opaque parse error instead of naming the cause | Implementing #330 (PR [#354](https://github.com/TucanoTechnology/TucanoTestAPI/pull/354)) | open |

## Epic disposition

- Every child task (#175–#180) is closed, and all four surface reports plus the scope, design and
  triage records are on `main`.
- Every finding is tracked: 16 are remediated (13 closed tickets covering 15 findings — #321 and
  #324 each merged two findings — plus F-178-1 fixed pre-triage by #278), and the remaining 4
  (F-178-5, F-179-2, F-179-3, F-179-5, all Info) are owned by open tickets #331, #333, #336 and
  #334. None was accepted as an untracked risk.
- [threat-model.md](threat-model.md) reflects the audit where a landed remediation changed a status:
  the auth-on-by-default fix for F-178-1 (#278) is recorded there, and the hardlink abuse case
  carries the #327 open-handle confinement requirement. F-179-3's question — stating the
  auto-merge PAT's authority in the threat model — is deliberately left to #336.
