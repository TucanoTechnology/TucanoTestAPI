# Security Audit S3 — Container and deployment posture (Issue #178)

Issue: [#178](https://github.com/TucanoTechnology/TucanoTestAPI/issues/178). Parent epic:
[#166](https://github.com/TucanoTechnology/TucanoTestAPI/issues/166) — *Carry out security audit*.

This report runs surface **S3** of [audit-scope.md](audit-scope.md): *"The image and its build, the
Compose configuration and container hardening, secret delivery from outside the image, the volume as
the only state, network exposure, and rollback to an immutable tag."* It carries findings only;
nothing here is fixed. Remediation belongs to the tickets [#180](https://github.com/TucanoTechnology/TucanoTestAPI/issues/180)
raises.

- **Affected revision (the pinned target):** `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`
- **Method:** [audit-scope.md § 6](audit-scope.md#6-how-the-audit-tasks-run), steps 1–7.
- **Findings in severity order:** **one** Critical, **two** Low, and **two** Info.
- **Pass entries:** ten, in the [Pass entries](#5-pass-entries) section.

## 1. Revision pinned

| Item | Value |
| --- | --- |
| Repository revision audited | `bb0ed2353cf9c50978b533cd5adc44563d60c4a3` |
| `origin/main` at audit time | `bb0ed2353cf9c50978b533cd5adc44563d60c4a3` (merge base, so the report's tree is the audited tree) |
| `Cargo.lock` SHA-256 | `4cddc67a699e87847a5a1f9d5cba12c3d32f6c80c83e1c11459fd970064f78ab` |
| Built image id (the audited artifact) | `sha256:2bad9aa8ea02ada4b5a2e2c08c54b0ae4ae3c6ce7f36f9f2ace7a006aeeff43a` |
| Audit worktree | the shared worktree at the pinned revision, branch `eco/178-deployment-audit`, `git status --porcelain` empty |

The audit did not follow a moving `main`: everything measured below was measured at
`bb0ed2353cf9c50978b533cd5adc44563d60c4a3`, and the artifact this report describes is the image that
revision's `Dockerfile` produced. A later `main` is a different audit.

One artifact id is not the audited one, and is the only reason §7 has work to do: the S3-4 rebuild
(which deliberately reproduces the retag) replaced the local tag with
`sha256:3400b0b874cba4babac822f3827b4559b9b2e782b0b0a3e30e88517a485c6c5f`. That candidate image is a
tear-down item, not a second subject.

## 2. Throwaway target (step 2)

One scratch Compose project, `audit-178`, plus the `docker run` arms the sub-tasks needed. Every arm
was built from the pinned revision in the worktree named in §1. The operator's long-lived instance
(Compose project `tucano-test`, container `tucano-test-api-1`) was never a target and was not stopped
or rebuilt: it reported `Up 33 hours` with `0.0.0.0:3100->3000/tcp, [::]:3100->3000/tcp` before,
during, and after the audit.

### 2.1 Deviations from the shipped Compose file

The scratch copy is `/tmp/audit-178/compose.yml`, a copy of the shipped `docker-compose.yml` (44
lines) with five documented deviations. None of them touches the premise of a finding; the fourth and
fifth are recorded because a reader of the F-178-2 reproduction needs them to judge whether the
mechanism is the shipped one.

1. **Host port number only:** `3100` → `3310`, keeping the shipped all-interfaces publish syntax
   (`- "3310:3000"`, not `127.0.0.1:3310:3000`). The shipped line is unchanged in the repository:
   `docker-compose.yml:11` — `- "3100:3000"`, with no host-address prefix, so Docker publishes on
   `0.0.0.0`. F-178-1's premise is therefore preserved, and the report asserts the shipped text
   separately below.
2. **Paths:** `build.context: .` → the worktree path, and the data bind `./data:/data` →
   `/tmp/audit-178/data:/data`. The container contract (`/data`) is untouched.
3. **Image tag:** `tucano-test-api:local` → `tucano-test-audit:bb0ed23`. This is deliberate: it lets
   the audit rebuild without ever retagging the name the operator's live image holds. The mechanism
   F-178-2 describes is a property of `image:` + `build:` composing into a single local name, and is
   unchanged by renaming the name; the shipped `image: tucano-test-api:local` with `build: .` is the
   file that has it.
4. **Only the `api` service is ever started.** `services.gui` builds from
   `context: ../Tucano-Test-GUI`, which from `/tmp/audit-178` would resolve to `/tmp/Tucano-Test-GUI`.
   The `gui` service is not part of the S3 threat surface under test.
5. **The audit data directory is mode `0777`, owner `1000:1000`, on a `tmpfs`.** Rootless Docker does
   not create a missing bind-mount source, so the directory has to exist and be writable by the
   container's uid before the first start; the widening is an artifact of the scratch host path, not
   of the shipped `./data` bind.

The shipped hardening keys were all left in place, and the exposure arm ran with
`TUCANO_AUTH_REQUIRED` **unset**, which is what the shipped file does: `grep -c
'TUCANO_AUTH_REQUIRED' docker-compose.yml` → `0`.

### 2.2 Arms

| Arm | Compose project / container | Host port | `TUCANO_AUTH_REQUIRED` | Result |
| --- | --- | --- | --- | --- |
| Shipped posture | `audit-178` / `audit-178-api-1` | 3310 | unset (the shipped default) | `docker port` → `0.0.0.0:3310` and `[::]:3310`; unauthenticated `GET /projects` → `200 [];` unauthenticated `POST /projects` → `201` (F-178-1) |
| Secret by file | `audit-178-filesecret` (`docker run -d`) | none published | `true` | `Status=running ExitCode=0`, no log output; unauthenticated `GET /projects` → `401 unauthorized`; login → `200` with a token (pass entry) |
| Refusal arms (secret) | `docker run --rm` | — | `true` | 31-byte secret, both sources, and a missing file each exit `1` naming the setting and never the value (pass entry) |
| Config file, good | `docker run -d` | none published | inherited from the file | `Running=true ExitCode=0`; `docker diff` → `A /conf` only; `touch /conf/x` → `Read-only file system` (pass entry) |
| Config file, refusals | `docker run --rm`, four arms | — | — | missing, unknown key, `version: 99`, truncated: each exit `1` with the setting named and no value or raw path (pass entry) |
| Build context | `docker build`, no run | — | — | 11,311 context files, context tar 3,506,821,120 bytes (Info observation) |

Container facts common to the Compose arm, read back with `docker inspect` rather than assumed:
`Config.User=tucano`, `ReadonlyRootfs=true`, `SecurityOpt=[no-new-privileges:true]`, `/tmp` present
as a tmpfs, and `/data` the only bind-mounted writable path. The image id of the audited build is
`sha256:2bad9aa8ea02…`; after the S3-4 rebuild the tag resolves to `sha256:3400b0b874cb…`.

## 3. Surface enumerated before probing (step 3)

Counts, not assertions. The enumerations were re-run at the audited revision; the numbers below are
that revision's, not the design document's.

### 3.1 The shipped Compose service definition

`docker-compose.yml` is **44 lines** with two services. Everything the file asserts about the `api`
service, at the line that asserts it:

| Line | Content | What it means |
| --- | --- | --- |
| `11` | `- "3100:3000"` | published on every interface: no host-address prefix, so `0.0.0.0` and `[::]` |
| `14` | `read_only: true` | the root filesystem is read-only |
| `15` | `tmpfs:` (`- /tmp`) | the only writable path besides the data mount |
| `18` | `no-new-privileges:true` | setuid escalation is refused |
| `19` | `restart: unless-stopped` | the `api` service's restart policy |
| `38` | `restart: unless-stopped` | the `gui` service's restart policy |

Keys **absent** from the file, which is as important as the keys present: no `cap_drop`, no
`healthcheck`, no `TUCANO_AUTH_REQUIRED`, no `user` override, no `security_opt` beyond
`no-new-privileges`. `services.api` carries `build: {context: ., dockerfile: Dockerfile}`,
`image: tucano-test-api:local`, `environment: {TUCANO_DATA_DIR: /data, PORT: 3000}`,
`volumes: - ./data:/data`, and `deploy: {replicas: 1, resources.limits: {cpus: "1.0", memory: 512M}}`.

### 3.2 The shipped image's build

`Dockerfile` is **42 lines**. The instructions that carry the runtime posture:

| Line | Instruction | Note |
| --- | --- | --- |
| `1` | `FROM rust:1.98.0-slim-trixie AS builder` | tag-pinned; the class is F-179-1's, already raised in S4 and not re-raised here |
| `6`–`8` | `COPY Cargo.toml Cargo.lock ./`, `COPY src ./src`, `COPY openapi.json swagger.html ./` | the complete set of source inputs; no `COPY .` |
| `13` | `FROM debian:trixie-slim@sha256:abc9cb88a5587630d7f915f47b23b0668fe250fbfc6457aa4d52b534c1bbf73f` | digest-pinned |
| `33` | `COPY --from=builder /build/target/release/tucano-test /usr/local/bin/tucano-test` | the only artifact copied forward |
| `35` | `LABEL org.opencontainers.image.version="${BUILD_NUMBER}"` | `local` by default |
| `41` | `USER tucano` | uid 10001 |
| `42` | `ENTRYPOINT ["/usr/local/bin/tucano-test"]` | — |

No `HEALTHCHECK` instruction. No `cap_drop`, no `--chown` on a writable path beyond the `/data` the
`RUN` step creates, and **no `.dockerignore`** in the repository.

`org.opencontainers.image.version` is set from `ARG BUILD_NUMBER=local`, so a local build labels
itself `local` and a release build labels itself `build-<run number>`; the metadata excerpt is in the
[pass entries](#5-pass-entries) (S3-11).

### 3.3 The CI security jobs against `scanning-policy.md` (S3-8)

The DoD requires the CI review claim by claim, so the deliverable is the table. "Already in S4?"
refers to [audit-s4-dependencies-and-supply-chain.md](audit-s4-dependencies-and-supply-chain.md) §4;
the S4 findings it names are cited, never re-raised.

| Policy claim | Job | Evidence in the job | Gap | Already in S4? |
| --- | --- | --- | --- | --- |
| Dependency advisory scan runs on every PR and every push to `main` | `audit` (`.github/workflows/security.yml:17`) | `:36` `cargo install cargo-audit --locked`, `:38` `cargo audit` | none found — the job can genuinely fail: no `continue-on-error` and no `\|\| true` anywhere in the file | — |
| Secret scan "scans repository history" | `secret-scan` (`:40`) | `:45` `fetch-depth: 0`, `:47` `gitleaks detect --source /repo --no-git --redact` | filesystem-only: `--no-git` means the checked-out history is never walked | **yes — F-179-5** |
| Container scan fails on CRITICAL/HIGH | `container-scan` (`:49`) | `:54` `docker build --file Dockerfile --tag tucano-test .`, `:55` `trivy-action@v0.36.0`, `:59` `exit-code: "1"`, `:60` `ignore-unfixed: true`, `:61` `severity: CRITICAL,HIGH` | fixed-criteria only, and it scans only its own local build — the published artifact is a separate build (F-178-4) | partially — F-179-1/F-179-2 are the pinning half |
| SBOM is produced and validated | `sbom` (`:63`) | `:89` `cargo cyclonedx --format json --override-filename sbom`, `:93` the inline `python3` validation, `:114` `upload-artifact@v5` with `:117 path: sbom.json`, `:118 retention-days: 90` | it is never attached to, or published with, the image a consumer deploys | **no — F-178-5** |
| Published artifact is scanned | — | `release.yml` pushes with `docker/build-push-action@v6`, `push: true` | **no scan step at all**, and no `provenance:`, `sbom:`, `attestations:`, or `id-token:` | **no — F-178-4** |
| Scans run weekly as well as per-PR | all four | `security.yml:10` `cron: "17 3 * * 1"` | none | — |
| Actions are pinned | all four | major-tag references (`actions/checkout@v5`, `trivy-action@v0.36.0`, …) | not SHA-pinned | **yes — F-179-2** |

Two gaps are new: the published artifact is never scanned (F-178-4), and the SBOM never accompanies
the image (F-178-5). Everything else in the table is either satisfied or already S4's finding.
F-179-3, F-179-4 and F-179-5 are **not** re-raised.

### 3.4 The configuration boundary's enumerated inputs

The loader's surface, read from the code at the pinned revision rather than from documentation:
`TUCANO_CONFIG_FILE` (`src/config.rs:48`) is the only way the file is named; the file's schema has
exactly eight keys (`version`, `auth_required`, `jwt_secret`, `jwt_secret_file`, `access_token_ttl`,
`refresh_token_ttl`, `bootstrap_username`, `bootstrap_password` —
`docs/deployment/config.example.json`); the secret-bearing environment keys are the four in
`src/auth/config.rs:113-118`; and the minimum secret length is 32 bytes
(`src/auth/config.rs:26`, `MIN_SECRET_BYTES`). These counts are what the S3-9 arms exercise.

## 4. Findings

Five findings were raised: one Critical, two Low, two Info. The two Info entries are supply-chain
assurance gaps rather than defects with a reachable manifestation, and are recorded so the decision
is deliberate.

### F-178-1: Do not publish the API on every interface while the shipped default leaves authentication off

- **Severity:** Critical
- **In scope:** S3 — the deployment edge. This is a *deployment* fact, not an HTTP-surface finding:
  the request is ordinary and the route is not the subject; what is audited is which interface the
  shipped configuration binds and whether the control that makes that binding safe is on.
- **Where:** `docker-compose.yml:11` (`- "3100:3000"`, no host-address prefix) and the file's absent
  `TUCANO_AUTH_REQUIRED` (0 matches), against
  `docs/security/authentication-decision.md`'s "The API must **not** be exposed beyond a trusted
  network until this is implemented" and [threat-model.md](threat-model.md) line 148's status.
- **Affected revision:** `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`
- **Reproduction:** start the shipped posture and make one unauthenticated request from an address
  that is not loopback.

  ```console
  $ docker port audit-178-api-1
  3000/tcp -> 0.0.0.0:3310
  3000/tcp -> [::]:3310

  $ ss -ltn 'sport = :3310'
  LISTEN 0  4096  0.0.0.0:3310  0.0.0.0:*
  LISTEN 0  4096     [::]:3310     [::]:*

  $ curl -s -o /dev/null -w '%{http_code}\n' http://192.168.8.123:3310/projects
  200
  $ curl -s http://192.168.8.123:3310/projects | head -c 400
  []

  $ curl -s -o /dev/null -w '%{http_code}\n' -X POST http://192.168.8.123:3310/projects \
       -H 'content-type: application/json' -d '{"name":"audit probe","description":"unauth write"}'
  201
  $ curl -s http://192.168.8.123:3310/projects | head -c 400
  [{"id":"audit probe.json","name":"audit probe", …}]

  $ curl -s -o /dev/null -w '%{http_code}\n' http://192.168.8.123:3310/health
  200
  $ curl -s -o /dev/null -w '%{http_code}\n' http://192.168.8.123:3310/auth/me
  401
  ```

  The request was made from the host's own non-loopback address (`192.168.8.123`) rather than from a
  second machine: the operator's network did not permit an off-host probe, so the reproduction is the
  loopback-excluded local address plus the `0.0.0.0` publish evidence, which is the fallback
  [the design](audit-design-176-178.md) §5 authorises. The two assertions against the **shipped**
  file, not the scratch copy:

  ```console
  $ grep -n 'ports:' -A2 docker-compose.yml
  10:    ports:
  11:      - "3100:3000"

  $ grep -n 'TUCANO_AUTH_REQUIRED' docker-compose.yml || echo "auth is not enabled by the shipped stack"
  auth is not enabled by the shipped stack
  ```

- **Observed:** the shipped Compose file publishes the listener on `0.0.0.0` and `[::]`, and sets no
  `TUCANO_AUTH_REQUIRED`, so the container that ships starts in the authentication-off arm. An
  unauthenticated `GET /projects` from a non-loopback address returns `200` with the project list, an
  unauthenticated `POST /projects` returns `201` and the resource is readable back, while
  `/auth/me` correctly answers `401` (the route exists and enforces; the stack simply does not turn
  authentication on). This is the shipped default, produced by `docker compose up -d --build` with no
  environment edit.
- **Expected:** publishing the listener and leaving the control that makes publication safe turned
  off is not a state the shipped configuration should produce. The project's own
  `authentication-decision.md` states the constraint in as many words — "The API must **not** be
  exposed beyond a trusted network until this is implemented" — and [threat-model.md](threat-model.md)
  line 148 carries the corresponding ⏳ status. The control is measured against the abuse-case row
  *"Secret disclosure through the file | Keep a secret supplied by file out of the image, out of
  `docker inspect`, out of logs, errors, and responses…"* only in the sense that it is the same
  document that declares the exposure unacceptable; the direct evidence is the decision document's
  sentence.
- **Impact:** authentication off means every client that can route to the port has the full authority
  of an anonymous caller: it reads and writes other people's projects, suites, cases, runs, and
  results. That is cross-project data access and writes outside the caller's scope, the rubric's
  *Severe* impact axis. What makes it Critical rather than High is the exploitability axis and the
  escalation rule, both stated below. Deployment configuration: this **is** the default
  configuration; the finding does not exist without it.
- **Severity, per [audit-scope.md](audit-scope.md) § 5.** Exploitability **Trivial** — "no account and
  no special position… a single request with no prerequisite state": the whole adversary model of
  §1 is "an adversary who can reach the HTTP listener". Impact **Severe** — cross-project data, write
  outside the caller's scope. Trivial × Severe = **Critical**, which is the top row of the matrix with
  a Severe impact, so the reserved identifier is used correctly. The escalation rule for remote
  reachability in a *default* configuration is **already satisfied by the shipped file** and is
  therefore neither applied a second time nor available as a de-escalation: the port is published on
  every interface, and the shipped file is the configuration. This reading is the one
  [the design](audit-design-176-178.md) §5 calls out as arguable, so the counter-argument is answered
  here rather than left implicit.
- **The "trusted local network" counter-argument, answered.** The argument is that the project
  documents a trusted-local-network expectation, so exposing the port is an operator choice rather
  than a defect. It does not hold, for three reasons. First, the document that states the expectation
  states it as a *constraint on the shipped artifact* — authentication-decision.md's sentence is a
  must-not, and the same sentence appears in requirement 5 of the threat model. Second, the defect is
  not that authentication is optional; it is that the shipped default publishes the listener on every
  interface **and** leaves the control that makes publication safe off, so the safe configuration is
  the one an operator has to discover rather than the one the file produces. A default that is safe
  only after an undocumented edit fails the rubric's own test for a default. Third, `0.0.0.0` is not
  a trusted local network: it is every interface the host has, including any that route off the
  machine, and on this host the published address is reachable from the LAN. Reading taken:
  **Critical**.
- **Suggested fix:** publish on `127.0.0.1` by default (`- "127.0.0.1:3100:3000"`) and/or set
  `TUCANO_AUTH_REQUIRED=true` in the shipped Compose file, so that the safe posture is the default
  and the unsafe one is the deliberate choice; state the expectation in `README.md` next to the port
  table, which today documents the port without the constraint. A defence-in-depth companion is a
  reverse proxy or firewall rule, but neither is a substitute for not shipping the unsafe default.
- **CWE:** CWE-306 (Missing Authentication for Critical Function), which is the mechanism actually
  observed: a critical function reachable with no authentication at all. CWE-1327 (Binding to an
  Unrestricted IP Address) describes the other half — the all-interfaces publish — and is cited as
  the reason the exposure is reachable off-host rather than as the primary classification.
- **Duplicates / prerequisites:** none. The S4 report's Critical calibration example
  ([audit-scope.md](audit-scope.md) § 5) describes this band; F-178-1 is the S3 instance of it, not a
  rediscovery of an S4 finding. Fixing F-178-1 does not close F-178-2, F-178-3, F-178-4, or F-178-5.

### F-178-2: Record an image id the documented Compose rollback can actually restore

> **Resolved after the audited revision (note added post-audit; the finding text below describes
> `bb0ed235` and nothing has been edited away).** The remediation is [#329](https://github.com/TucanoTechnology/TucanoTestAPI/issues/329),
> landed by PR [#350](https://github.com/TucanoTechnology/TucanoTestAPI/pull/350): Step 0 of
> `canary-validation-and-rollback.md` now records the running container's image id and pins it
> under a rollback-only tag before any rebuild, and the Rollback section restores by re-tagging
> that recorded id onto the mutable `tucano-test-api:local` name, recreating with
> `--no-build --force-recreate`, and asserting the inspected id still matches. The sentence
> quoted below (`:140` — *“Rollback re-deploys `PREVIOUS` … it never moves a tag.”*) is absent
> from the current tree precisely because that correction rewrote it: the claim did not hold for
> the Compose local build, where `docker compose up` cannot select an image by id. The `Where:`
> line addresses the audited revision — the quoted compose entries now sit at
> `docker-compose.yml:4`–`:5` and `:11`. See the F-178-2 row of
> [audit-summary-s1-s4.md](audit-summary-s1-s4.md) for the tracking status.

- **Severity:** Low
- **In scope:** S3 — "rollback to an immutable tag" ([audit-scope.md](audit-scope.md) § 2). The trust
  boundary is operational rather than one of the nine: the rollback procedure is a control the
  deployment guide and the canary runbook promise the operator.
- **Where:** `docker-compose.yml:8` (`image: tucano-test-api:local`) together with `docker-compose.yml:7`
  (`build: {context: ., dockerfile: Dockerfile}`), against
  `docs/deployment/canary-validation-and-rollback.md:49` ("Record `PREVIOUS` before touching anything;
  it is the rollback target."), `:140` ("Rollback re-deploys `PREVIOUS` and re-runs the checks; it
  never moves a tag."), `:209` ("`PREVIOUS` and `CANDIDATE` — the exact immutable tags."), and the
  Rollback section's "With Compose, restore the previously pinned image tag and recreate only the
  `api` service".
- **Affected revision:** `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`
- **Reproduction:** follow the runbook literally against the scratch stack.

  ```console
  $ PREVIOUS=$(docker inspect --format '{{.Image}}' audit-178-api-1); echo "$PREVIOUS"
  sha256:2bad9aa8ea02ada4b5a2e2c08c54b0ae4ae3c6ce7f36f9f2ace7a006aeeff43a

  $ docker image ls tucano-test-audit --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.CreatedSince}}'
  tucano-test-audit:bb0ed23 2bad9aa8ea02 12 minutes ago

  $ docker compose -f /tmp/audit-178/compose.yml -p audit-178 build --build-arg BUILD_NUMBER=audit-4701 api
  $ docker image ls tucano-test-audit --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.CreatedSince}}'
  tucano-test-audit:bb0ed23 3400b0b874cb About a minute ago

  # the runbook's Compose rollback, verbatim:
  $ docker compose -f /tmp/audit-178/compose.yml -p audit-178 up --detach --no-deps --force-recreate api
  Container audit-178-api-1  Recreated
  Container audit-178-api-1  Starting
  Container audit-178-api-1  Started

  $ docker inspect --format '{{.Image}}' audit-178-api-1
  sha256:3400b0b874cba4babac822f3827b4559b9b2e782b0b0a3e30e88517a485c6c5f
  $ docker image ls --filter dangling=true
  REPOSITORY   TAG   IMAGE ID   CREATED   SIZE
  $ docker image ls | grep 2bad9aa8ea02 || echo "NO tag or name references PREVIOUS"
  NO tag or name references PREVIOUS
  $ docker inspect --format '{{.Id}}' 2bad9aa8ea02
  Error: no such object: 2bad9aa8ea02
  ```

- **Observed:** `image: tucano-test-api:local` with `build: .` means the single local name is
  retagged on every `--build`, so `PREVIOUS` loses its only tag. In this environment the previous
  image is then pruned outright — `docker image ls --filter dangling=true` is empty and the image id
  named by `PREVIOUS` is not addressable by tag **or** by id (`No such object`). The runbook's
  documented Compose rollback (`up --detach --no-deps --force-recreate api`, whose text promises to
  "restore the previously pinned image tag") therefore redeployed the **candidate**: the recreated
  container runs `sha256:3400b0b874cb…`, the same id the tag now names. The rollback silently did
  nothing. Independent corroboration from the operator's own instance, without touching it: the tag
  `tucano-test-api:local` resolves to `sha256:8f075af3c2e7` while the live container's `.Image` is
  `sha256:13f10c0e9208` — the tag and the running image had already diverged there too.
- **Expected:** the runbook's Compose instruction is executable with an immutable identity, and the
  restored container runs the previous image. The mechanism the runbook describes
  (`PREVIOUS="$IMAGE:build-4700"`, `CANDIDATE="$IMAGE:build-4711"`) is a registry-based one where a
  tag is a release artifact; in the shipped Compose configuration the tag is a build output, and the
  runbook does not say what `PREVIOUS` is in that case. The defect is that a documented operational
  control does not hold — the runbook is not wrong about registries, it is silent about the shipped
  local-build path.
- **Impact:** a failed release is the moment the operator reaches for this runbook, and the step they
  reach for re-deploys the build they are trying to escape. Who is affected: anyone following the
  runbook against the shipped Compose file. Deployment configuration: independent of
  `TUCANO_AUTH_REQUIRED`; the retag happens at build time and neither arm avoids it. The severity
  axis is *Limited* — the consequence is a **slower rollback**, not disclosed or corrupted data, and
  the operator still has the image id in their shell history — and the exploitability axis is
  *Difficult*, because it needs a failed release **and** an operator who follows the documented
  procedure. The design recommends **Low** and states that pair as "Difficult × Limited", which reads
  **Info** off the matrix; this report keeps the **Low** the design recommends and states which axis
  moved rather than leaving the discrepancy implicit. The impact is scored *Moderate* under the
  rubric's own wording for that level — "the defeat of one control that by itself grants nothing
  further": the runbook's rollback is one control, and losing it grants nothing beyond the slower
  recovery, so it is a defence-in-depth control the audit could break rather than a disclosure.
  Difficult × Moderate = **Low** by the matrix. A reader who prefers Info can move the impact back to
  *Limited*; the mechanism and the fix do not change.
- **Suggested fix:** publish immutable `build-<run number>` / `vMAJOR.MINOR.PATCH` tags from CI and
  reference them in the Compose file, so `PREVIOUS` is a registry tag as the runbook assumes; or, if
  the local-build path is to stay, amend the runbook to record the image **id** and re-tag it
  (`docker tag "$PREVIOUS" tucano-test-api:local`) before recreating the service — and keep the
  previous id from being pruned. The `release.yml` path already produces both tag forms
  (`type=ref,event=tag`, `type=raw,value=build-${{ github.run_number }}`), so the Compose file is the
  only piece that has to change for the first option.
- **CWE:** CWE-693 (Protection Mechanism Failure) — the rollback mechanism itself fails, because the
  identity it depends on is destroyed by the next build. CWE-1059 (Insufficient Technical
  Documentation) is adjacent, and describes the runbook's silence about the local-build path; CWE-693
  is primary because the mechanism, not only the prose, is broken.
- **Duplicates / prerequisites:** none. Fixing F-178-4 (scanning the published artifact) touches the
  same release path but does not fix this; F-178-4's suggested fix could carry the immutable tags as
  part of the same change.

### F-178-3: Drop the capabilities the container never needs

- **Severity:** Low
- **In scope:** S3 — "the Compose configuration and container hardening".
- **Where:** `docker-compose.yml` (no `cap_drop` key anywhere in the 44-line file) and `Dockerfile`
  (no capability change; the runtime stage ends `USER tucano`).
- **Affected revision:** `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`
- **Reproduction:**

  ```console
  $ docker inspect --format 'User={{.Config.User}} Readonly={{.HostConfig.ReadonlyRootfs}}
      CapAdd={{.HostConfig.CapAdd}} CapDrop={{.HostConfig.CapDrop}}
      SecOpt={{.HostConfig.SecurityOpt}} Privileged={{.HostConfig.Privileged}}' audit-178-api-1
  User=tucano Readonly=true CapAdd=[] CapDrop=[] SecOpt=[no-new-privileges:true] Privileged=false
  ```

  `CapBnd` (the bounding set the kernel gives the process) is `00000000a80425fb`, which is Docker's
  default set, not an empty one.
- **Observed:** the stack applies no `cap_drop`, so the container runs with Docker's default
  capability set although the service needs none of it — it listens on a TCP port above 1024, reads a
  bind-mounted data directory it owns, and writes to its own tmpfs. Every other hardening claim in the
  file **does** hold and is recorded as a pass entry: `User=tucano`, `ReadonlyRootfs=true`,
  `no-new-privileges:true`, `/tmp` as a tmpfs, and `/data` the only writable mount.
- **Expected:** a service that needs no capability runs with none. The control this is measured
  against is the hardening half of S3's scope sentence. Compose's `cap_drop: ["ALL"]` is the
  idiomatic way to say it, and `security_opt: [no-new-privileges:true]` — already present — is the
  same posture applied to setuid bits.
- **Impact:** Docker's default set includes no `CAP_SYS_ADMIN`, so this is not a privilege-escalation
  path on its own; it is a defence-in-depth control the audit could break, which the rubric scores on
  the impact axis as *Moderate* ("the defeat of one control that by itself grants nothing further").
  On the exploitability axis the attacker needs no account and no special position — nothing has to be
  earned beyond the companion defect that makes the extra capabilities matter — but that companion
  defect does not exist in this revision and is not a state the attacker sets up, so the pair is
  scored *Difficult* × *Moderate* rather than *Moderate* × *Moderate*: the matrix's *Moderate*
  exploitability test needs "one prerequisite step" that is repeatable, and an unknown second defect
  is a condition rather than a step. Difficult × Moderate = **Low**, matching the design's
  recommendation. Deployment configuration: independent of `TUCANO_AUTH_REQUIRED`.
- **Suggested fix:** add `cap_drop: ["ALL"]` to the `api` service in `docker-compose.yml`, alongside
  the read-only rootfs the stack already has; if a future feature needs a specific capability, add it
  back explicitly rather than inheriting the default set.
- **CWE:** CWE-250 (Execution with Unnecessary Privileges).
- **Duplicates / prerequisites:** none.

### F-178-4: Scan the artifact that is published, not only the one built locally

- **Severity:** Info
- **In scope:** S3 — the CI supply-chain controls, measured against the claimed container scan. This
  supplements S4's pinning findings rather than repeating them.
- **Where:** `.github/workflows/release.yml:35` (`docker/build-push-action@v6` with `push: true`),
  against `.github/workflows/security.yml:53-61` (`container-scan`, which scans the image it built in
  the job).
- **Affected revision:** `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`
- **Reproduction:**

  ```console
  $ grep -n 'run:\|uses:\|provenance\|sbom:\|attestations\|id-token' .github/workflows/release.yml
  21:      - uses: actions/checkout@v5
  22:      - uses: docker/setup-buildx-action@v3
  23:      - uses: docker/login-action@v3
  28:      - uses: docker/metadata-action@v5
  35:      - uses: docker/build-push-action@v6
  # no `run:` step, no provenance:/sbom:/attestations:, no id-token permission

  $ grep -n 'docker build\|trivy\|exit-code' .github/workflows/security.yml
  54:        run: docker build --file Dockerfile --tag tucano-test .
  55:      - uses: aquasecurity/trivy-action@v0.36.0
  59:          exit-code: "1"
  ```

- **Observed:** `container-scan` builds `tucano-test` in its own job and scans **that** image;
  `release.yml` performs a separate build and pushes it to `ghcr.io/tucanotechnology/tucanotestapi`
  with no scan step between the build and the push. Nothing verifies that the published artifact is
  the scanned one, and the builder stage is tag-pinned (F-179-1), so "same revision, same content" is
  not established by anything in the workflows. No `provenance:`, `sbom:`, or `attestations:` is
  passed, and no `id-token` is requested, so the pushed artifact also carries no signed statement
  about how it was built.
- **Expected:** the artifact a consumer deploys is the artifact the scan reported on, and a scan
  result that can be tied to a specific published digest. `scanning-policy.md`'s "Scans the production
  Docker image for OS and library vulnerabilities" is a claim about the production image; today the
  job satisfies it for the local build only.
- **Impact:** a vulnerability that appears in the release build but not in the CI build — through a
  republished builder tag, a different builder's cache, or a dependency resolved only in the release
  environment — reaches the registry without a gate. The exploitability axis is *Difficult* (it needs
  a vulnerable component to exist **and** the local scan to have missed it, which is not something the
  attacker sets up directly) and the impact axis is *Limited* (the gap's own effect is a missing
  detection, and the content-advisory scan on the local build still exists). Difficult × Limited =
  **Info**, and the one-level escalation for default configurations was considered and **not**
  applied: the defect has no remotely reachable manifestation of its own, and attributing F-178-1's
  reachability to it would inflate a coverage gap into a vulnerability. Deployment configuration:
  independent of `TUCANO_AUTH_REQUIRED`. Recorded at Info rather than Low so the decision to accept
  it is deliberate; a maintainer who wants the published digest gated can move to Low without
  disagreeing with the analysis.
- **Suggested fix:** add a scan step to `release.yml` between the build and the push (or scan the
  pushed digest and fail the job afterwards), and pass `provenance: true` so the artifact carries a
  build record. Both are the natural home for F-178-2's immutable tags as well.
- **CWE:** CWE-1357 (Reliance on Insufficiently Trustworthy Control Sphere) — the same class S4 used
  for F-179-1, since the control this defeats is "we scanned it"; CWE-1104 (Use of Unmaintained
  Third-Party Components) is adjacent for the vulnerable-component consequence.
- **Duplicates / prerequisites:** **not** in S4. F-179-1 and F-179-2 are prerequisites in the sense
  that pinning the builder by digest is what would make "the scanned build and the published build are
  the same bytes" verifiable — cite them, do not re-raise them.

### F-178-5: Publish the SBOM with the artifact, or record why it is not

- **Severity:** Info
- **In scope:** S3 — the CI supply-chain controls, measured against the claimed SBOM. Supplement to
  S4's finding about the SBOM's *content* validation.
- **Where:** `.github/workflows/security.yml:113-118` (the `upload-artifact` step) and
  `docs/security/scanning-policy.md:28` ("Uploaded as a build artifact (`sbom.json`) for compliance
  and auditing") versus `.github/workflows/release.yml` (no SBOM reference).
- **Affected revision:** `bb0ed2353cf9c50978b533cd5adc44563d60c4a3`
- **Reproduction:**

  ```console
  $ sed -n '113,118p' .github/workflows/security.yml
        - name: Upload SBOM artifact
          uses: actions/upload-artifact@v5
          with:
            name: sbom
            path: sbom.json
            retention-days: 90

  $ grep -rn 'sbom' .github/workflows/release.yml || echo "release.yml does not mention the SBOM"
  release.yml does not mention the SBOM
  ```

- **Observed:** the SBOM is generated, validated, and uploaded as a workflow artifact, where CI
  retention (`retention-days: 90`, matching `scanning-policy.md:109`'s "SBOM artifacts are retained
  for 90 days") makes it expire. It is never attached to the image, never referenced by
  `release.yml`, and never associated with a published digest. The claim in the policy — "Uploaded as
  a build artifact … for compliance and auditing" — is literally satisfied and practically weak: an
  auditor handed a deployed image has no SBOM to pair with it.
- **Expected:** the SBOM accompanies the artifact it describes, so the inventory is available for
  exactly the image a consumer runs.
- **Impact:** an operator or auditor assessing a deployed image cannot establish its component
  inventory from the artifact itself; they have to find the CI run, which is only possible while the
  artifact is retained and only for builds CI made. Exploitability *Difficult*, impact *Limited* —
  the same axes as F-178-4 — so **Info** by the matrix, at the same reasoning and with the same
  consideration of the escalation rule. Deployment configuration: independent of
  `TUCANO_AUTH_REQUIRED`.
- **Suggested fix:** attach the SBOM to the image (an OCI referrer or an attestation via
  `docker/build-push-action`'s `sbom: true`), or publish it beside the release tag, and either way
  state in `scanning-policy.md` where a consumer finds it. If workflow-artifact retention is the
  intended distribution, correct the policy sentence to say so.
- **CWE:** CWE-1104 (Use of Unmaintained Third-Party Components) is the adjacent consequence; as with
  S4's F-179-4, no single CWE fits a documentation/process divergence, and CWE-1059 (Insufficient
  Technical Documentation) is the closest description of the gap itself. The field is filled with the
  adjacent mapping and no stronger claim is made.
- **Duplicates / prerequisites:** **not** in S4. S4's pass entry for the SBOM job covers the
  *validation* control (which holds) and must not be confused with this distribution gap; F-178-4's
  suggested fix touches the same release step.

### Recorded observations (not findings)

Four observations were recorded as observations rather than findings, because in each case the
implementation is sound and the risk is latent or environmental. They are recorded here so the
decision is deliberate rather than accidental.

- **S3-6 — build context and `.dockerignore`.** The repository has no `.dockerignore`
  (`ls -a | grep -c dockerignore` → `0`) while both `container-scan` and `release.yml` build with
  `context: .`. The measured context is **11,311 files**, with `target/` alone 3.3 GB; the context
  tar is **3,506,821,120 bytes**, and BuildKit reports `transferring context: 2B done` because the
  instruction cache makes most of the transfer unnecessary on a warm builder. The Dockerfile's `COPY`
  instructions name only `Cargo.lock`, `Cargo.toml`, `src`, `openapi.json`, and `swagger.html`
  (`Dockerfile:6-8`, plus `:33`'s copy from the builder stage), so **no** unintended file enters the
  image in this revision. The consequences are context size, build-cache exposure on a shared runner,
  and a latent hazard: a future `COPY .` would sweep in whatever the worktree holds at build time —
  including the operator's `data/` directory, 224 KB in the audited worktree, which `.gitignore`
  excludes from version control but not from a build context. Recorded as an Info observation per the
  design; not raised as a finding, because the audit could not show a concrete file entering the
  image.
- **S3-7 — tooling and documentation consistency for the shipped ports.** `scripts/smoke.sh` defaults
  to `http://localhost:3000` (`scripts/smoke.sh:29`), while the shipped Compose publishes the API on
  `3100`. Against the shipped stack, the default invocation fails:
  `smoke: FAIL — GET /health: expected status=ok, got status=True — {"status":true}`, exit `1`. The
  response body is not this service's: the host port `3000` is held by another rootless container
  (`open-webui`, `0.0.0.0:3000->8080/tcp`, reached through `rootlesskit` pid 1710 on this host), which
  answers `/health` with `{"status":true}` where this service answers
  `{"status":"ok","storage":"filesystem"}`. The failure is therefore a port collision on the audit
  host compounded by the script's default, not a defect in the service. `bash scripts/smoke.sh
  http://127.0.0.1:3310` → `smoke: PASS — health, scratch project/case CRUD, and both deletions
  observed`. Recorded as an Info observation: a tooling/documentation mismatch with no security
  effect. The README's port table (`README.md:181-202`) already documents `3100`, so the mismatch is
  the script's default rather than the documentation's.
- **The data directory's first-run behaviour.** `README.md:187` says Docker "creates the folder on
  first run", and under **rootless** Docker it does not: a missing bind-mount source is not created,
  and the container fails to start until the host path exists. This is an environment property of
  rootless Docker (the audit host runs Docker 29.8.0 rootless with Storage Driver `overlayfs`), not a
  property of the project's configuration, and it is recorded here because the next audit will hit it
  and because the README sentence is true for a rootful daemon. The audit worked around it by
  pre-creating the directory (deviation 5 in §2.1). No finding: the shipped configuration does not
  claim otherwise, and a missing-bind-source failure is a loud refusal rather than a silent one.
- **The SIGSTOP non-delivery (S3-5).** `docker exec audit-178-api-1 sh -lc 'kill -STOP 1'` did not
  wedge the process — the signal did not reach pid 1 through this runtime — so the wedged-process
  behaviour was measured with `docker pause` instead: `docker ps` → `Up 57 seconds (Paused)`,
  `curl --max-time 5 .../health` → HTTP `000`, exit `28`, and `RestartCount=0`, so a wedged process is
  not noticed and `restart: unless-stopped` does not act on it. `docker unpause` → `/health` `200`
  again. The healthcheck finding itself is S3-5's: `docker inspect --format '{{json
  .Config.Healthcheck}}'` → `null`, no `HEALTHCHECK` in the Dockerfile, and no `healthcheck:` in
  Compose, while `/health` is a public endpoint and the service declares `restart: unless-stopped`.
  Recorded as an Info observation rather than a finding: it is a missing availability control in the
  operator's own container, with no data or credential impact, and the fix is a `HEALTHCHECK`
  instruction plus a Compose `healthcheck:` — the matrix's Info definition ("a control the audit
  recommends adding") fits it exactly. The non-delivery is recorded so that a later reader does not
  mistake a failed `kill` for a held control.

## 5. Pass entries

Controls the audit tested and could not break. Each entry names the test that proves it.

- **No secret reaches any inspection channel.** The file-supplied secret arm started from a
  read-only mount with `TUCANO_JWT_SECRET_FILE=/run/secrets/jwt`, and the value appeared in **no**
  channel: `docker inspect` → 0 occurrences; `docker logs` → 0; the container's process environment
  (`tr '\0' '\n' < /proc/1/environ | grep -c '…'`) → 0; every response body → 0; and the image's own
  history, `Config.Env`, and `Config.Labels` → 0. Invariant 7 (credentials never in the clear)
  holds for the channels S3 owns.
- **The image channel is clean for value-supplied secrets too.** `docker history --no-trunc`,
  `docker inspect {{json .Config.Env}}`, and the image labels carry no secret string for the arm that
  supplied one by value, so the layer-caching channel does not leak what the mount channel protects.
- **A file-supplied secret makes the enforcing arm reachable.** `TUCANO_AUTH_REQUIRED=true` with the
  secret supplied by file: unauthenticated `GET /projects` → `HTTP/1.0 401 Unauthorized
  {"error":{"code":"missing_token","message":"Authentication required","requestId":"…"}}`;
  `POST /auth/login` with the bootstrap pair → `HTTP/1.0 200 OK` with `accessToken` (225 characters),
  `refreshToken`, `tokenType: Bearer`, `expiresIn: 900`; authenticated `GET /auth/me` →
  `HTTP/1.0 200 OK {"id":"…","username":"admin","systemAdmin":true,"roles":{}}`; authenticated
  `GET /projects` → `HTTP/1.0 200 OK []`. The delivery mechanism S3 audits works.
- **Startup refusals name the setting and never the value.** Three arms, each exit code `1`, each
  with no secret string anywhere in its output (invariant 8): a 31-byte secret →
  `Error: ShortSecret { length: 31 }`; both sources supplied → `Error: SecretSourcesConflict`; a
  file path that does not exist → `Error: SecretFile { path: "/run/secrets/absent", source: Os {
  code: 2, kind: NotFound, message: "No such file or directory" } }`. The variable named is
  `TUCANO_JWT_SECRET` / `TUCANO_JWT_SECRET_FILE`, never a value.
- **Every unresolvable configuration refuses startup.** Four configuration-file arms, each
  `Running=false ExitCode=1`, four lines of output, and zero occurrences of either secret value or of
  the raw `/tmp/audit-178/conf` path: missing file → `Error: UnreadableFile { source: Os { code: 2,
  kind: NotFound, … } }`; unknown key → `Error: Malformed { detail: "unknown field \`unknown_key\`,
  expected one of \`version\`, \`auth_required\`, \`jwt_secret\`, \`jwt_secret_file\`,
  \`access_token_ttl\`, \`refresh_token_ttl\`, \`bootstrap_username\`, \`bootstrap_password\` at line
  10 column 15" }`; `version: 99` → `Error: UnsupportedVersion { found: 99 }`; truncated document →
  `Error: Malformed { detail: "EOF while parsing a value at line 1 column 35" }`. This is invariant 8:
  resolved once at startup, refused rather than defaulted, never a warning and never a plaintext
  fallback. The path-free wording is deliberate — `ConfigError::UnreadableFile`'s own doc comment
  says the ADR forbids a raw file path in an error.
- **The running service never writes its configuration.** A good config file mounted read-only:
  `Running=true ExitCode=0`; `docker diff` → `A /conf` only, i.e. the mount point and nothing else;
  `touch /conf/x` → `touch: cannot touch '/conf/x': Read-only file system`, exit `1`; and the host
  file's SHA-256 was unchanged after the run (`500b3bd00316d00ae94cb4c3df06096e6074cb0cf806c66bda6e2aa8eefe75a3`
  before and after). Invariant 9 holds, and the read-only rootfs the container's `read_only: true`
  provides stays intact.
- **The container writes nothing to its image layer.** `docker diff` on the running container is
  empty; the only writable locations are the data mount and the `/tmp` tmpfs; `touch /nope` →
  `touch: cannot touch '/nope': Read-only file system`, exit `1`; and `docker inspect {{json
  .Mounts}}` shows exactly one read-write bind mount (the data directory) plus the anonymous volume
  the Dockerfile's `VOLUME ["/data"]` created when no bind was supplied. Invariants 1 and 9.
- **The hardening claims the file makes all hold.** `User=tucano` (uid 10001, not root),
  `ReadonlyRootfs=true`, `SecurityOpt=[no-new-privileges:true]`, `/tmp` a tmpfs, `/data` the only
  writable mount, `Privileged=false`. The **absent** claim is F-178-3; everything the file asserts is
  true of the running container.
- **The image identifies its build and discloses no secret or internal path.** The metadata excerpt:
  `Id=sha256:2bad9aa8ea02…`, `Created` at the audited build, `Size=184770920`, `Env=[PATH=…,
  TUCANO_DATA_DIR=/data, PORT=3000]`, `Labels={com.docker.compose.project: audit-178,
  com.docker.compose.service: api, com.docker.compose.version: 5.4.0,
  org.opencontainers.image.version: local}`, `User: "tucano"`, `WorkingDir: null`,
  `Entrypoint: ["/usr/local/bin/tucano-test"]`, `Cmd: null`, `ExposedPorts: {"3000/tcp": {}}`,
  `Volumes: {"/data": {}}`. The version label is the build argument and is `local` for a local build,
  and nothing in the metadata names a secret or a host path (S3-11).
- **The policy's "fails the build" claims are enforceable.** A whole-file read of
  `.github/workflows/security.yml` finds **no** `continue-on-error` and **no** `|| true`, so the
  `audit` job's `cargo audit` and the `container-scan` job's `exit-code: "1"` really can fail the
  build; the SBOM validation step exits non-zero on a missing, empty, non-JSON, mis-labelled, or
  component-less document (S3-8, and S4's pass entry for the same job).
- **The scans run weekly as well as per-PR.** `security.yml:10` — `- cron: "17 3 * * 1"` — with the
  workflow triggered on `pull_request` and on pushes to `main` and `feature/**`, so a dependency
  published after the last commit is still found within a week (S3-8).

## 6. Calibration confirmed

- **The Critical worked example** ([audit-scope.md](audit-scope.md) § 5): "With the shipped Compose
  configuration, `GET /openapi.json` is public by design, and suppose some route derived a filesystem
  path from a request field without confinement… Exploitability *Trivial*, impact *Severe*, and
  reachable in the default configuration → **Critical**." Re-confirmed against this revision by
  construction rather than by argument: the example's shape is exactly F-178-1's, and F-178-1's
  reproduction shows the shipped default producing an unauthenticated `200` and `201` from a
  non-loopback address. The calibration therefore transfers: a single unauthenticated request against
  the shipped Compose stack reaching data outside the caller's scope is the Critical band, and an S3
  auditor who scored it lower would be disagreeing with the rubric rather than with this report. The
  example's own premise also holds here — `GET /openapi.json` is public by design
  ([authentication-decision.md](authentication-decision.md) names it one of the five public
  operations). Recorded as calibration confirmed, **not** raised as a finding of its own.
- **The Info worked example** ([audit-scope.md](audit-scope.md) § 5): `scripts/clear-data.mjs`. S4
  confirmed the calibration against the code and recorded it in its own §6; re-checked at this
  revision, the script's candidate list is unchanged (`scripts/clear-data.mjs:26-31`: `argv[2]`,
  `API_URL`, `TUCANO_API_URL`, `http://localhost:3100`, `http://localhost:8080/api`,
  `http://localhost:3000`, with `:46` falling back to the first candidate) and `Authorization` does
  not appear in the file at all. The calibration holds and this report adds nothing to S4's
  confirmation; it is **not** re-raised as a finding, per the audit's baseline rule.

## 7. Tear-down (step 7)

The scratch Compose project and its volume were removed (`docker compose -p audit-178 down -v`),
together with the `docker run` arms (`audit-178-api-1`, `audit-178-filesecret`), the local image built
for the audit (`tucano-test-audit:bb0ed23`), the candidate image the S3-4 rebuild produced
(`sha256:3400b0b874cb…`, which the retag left unaddressable by tag), and the scratch directory
`/tmp/audit-178` that held the compose copy, the config fixtures, and the data directory. The
operator's long-lived Compose project `tucano-test` and its container `tucano-test-api-1` were never a
target, were never stopped, and were never rebuilt; it reported `Up` before, during, and after the
audit, with `tucano-test-api:local` still resolving to `sha256:8f075af3c2e7`.

One detail worth recording for whoever runs the next audit, because it cost this one a debugging
cycle and is not a property of the application: the container runs as uid 10001, so files it creates
in a bind-mounted data directory are owned by uid 10001 and cannot be removed by the host user that
owns the directory. `rm -rf` on the scratch path fails on exactly those files; removing them needs
either `sudo` or a throwaway container with the path mounted. The same applies to the anonymous
volume the Dockerfile's `VOLUME ["/data"]` declaration creates when a `docker run` arm supplies no
bind mount.
