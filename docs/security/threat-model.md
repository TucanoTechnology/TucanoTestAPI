# Service Threat Model

## Scope

This threat model covers the Rust HTTP service, its JSON filesystem repository, attachment handling, and the [TucanoTestGUI](https://github.com/TucanoTechnology/TucanoTestGUI) client.

The service now exposes CRUD and attachment endpoints, so the controls below are live requirements rather than design intent. Controls that remain unimplemented are called out explicitly in the Open decisions section.

## Assets

- Project, test case, suite, and run records
- Attachment contents and metadata
- Authentication credentials, request identifiers, and audit events
- Filesystem integrity, availability, and deployable artifacts

## Trust boundaries

| Boundary | Untrusted input or actor | Required control |
| --- | --- | --- |
| HTTP client to service | Anonymous or authenticated request | Authentication, authorization, body limits, timeouts, request IDs |
| JSON payload to domain model | Malformed, oversized, or unknown fields | Draft 2020-12 validation, bounded parsing, explicit schema policy |
| Resource ID or filename to filesystem | Traversal, absolute paths, separators, symlinks, hardlinks | Allow-list validation, canonical confinement, symlink checks, open-handle confinement for attachment reads |
| Service to stored JSON | Corruption, partial writes, concurrent writers | Same-directory temp file, flush, atomic rename, locking policy |
| Attachment upload to storage | Oversized or unexpected file | Streaming limits, type/size validation, safe permissions |
| Configuration file to service startup | Malformed, unknown-keyed, tampered, or wrongly-encrypted file; a stray file on an implicit path | Strict schema with unknown keys refused, explicit path only, no plaintext fallback, fail-closed startup errors that name the setting and never the value |
| Configuration key to service startup | A key that is absent, wrong, readable from the image, or disclosed in a log or error | Key supplied from outside the artifact by a read-only mount, authenticated encryption with a key identifier, refusal to boot rather than warn, no value ever logged |
| Service logs and audit events | Secrets, paths, file contents | Structured redaction and bounded fields |
| GUI to storage | Direct filesystem access | GUI uses the documented HTTP API only |

## Abuse cases and required behavior

| Threat | Requirement | Verification |
| --- | --- | --- |
| Malicious JSON | Reject malformed, oversized, deeply nested, or schema-invalid input with a stable safe error | Negative parser and schema tests |
| Path traversal | Reject `..`, absolute paths, forbidden separators, and any path escaping the configured root | Unit and integration path tests |
| Symlink escape | Refuse symlink-based escape from the configured root and unexpected file types | Symlink fixture tests |
| Hardlink read-through | Refuse an attachment whose opened descriptor has a link count above one or a device other than the data root's — a path check cannot see a second name for the same inode (#327) | Hardlink fixture tests |
| Oversized upload | Enforce request, JSON, attachment, and concurrency limits before unbounded allocation | Limit and streaming tests |
| Corrupted file | Return a safe storage error and preserve the original file | Corruption and recovery tests |
| Concurrent write | Define lock and overwrite behavior; never publish a partial JSON document | Concurrent writer and atomicity tests |
| Denial of service | Bound body size, JSON depth, filesystem work, uploads, and request duration | Timeout, cancellation, and resource-limit tests |
| Data disclosure | Never return internal paths, stack traces, raw filesystem errors, secrets, or file contents in logs | Error and log-redaction tests |
| Unauthorized access | Authenticate before protected operations and authorize by resource/action | Auth matrix tests (`tests/auth.rs`; hand-marked public surface in `tests/service.rs`) |
| Unusable configuration | Refuse to boot on a malformed, unknown-keyed, or tampered configuration file, or on a secret that is too short, from two conflicting sources, or unreadable; name the setting, never the value | Configuration loader unit tests and the startup-error redaction test (#188, #190) |
| Secret disclosure through the file | Keep a secret supplied by file out of the image, out of `docker inspect`, out of logs, errors, and responses; never fall back to plaintext when a secret-bearing file has no usable key | Log-redaction and startup-refusal tests (#189, #190) |
| Vulnerable dependency or image | Run advisory, secret, and container scans in CI | Security workflow and clean-baseline checks |

## Security invariants

1. The configured data root is the only filesystem area the repository may read or write.
2. Persistence publishes complete documents atomically; failed writes do not replace valid data.
3. Client-visible errors use stable codes and safe messages with request IDs.
4. The GUI never reads or writes JSON files directly.
5. `unsafe` Rust is forbidden by the crate lint unless the policy is explicitly revised and reviewed.
6. Security-sensitive events exclude credentials, tokens, raw payloads, attachment contents, and internal paths.
7. Credentials are never stored or logged in the clear: passwords are persisted only as Argon2id PHC hashes
   and refresh tokens only as SHA-256 digests of an opaque value the client keeps.
8. Configuration is resolved once, at startup, before the listener binds: environment first, then an
   optional configuration file, then built-in defaults, per key. A configuration that cannot be
   resolved is a startup refusal, never a warning and never a plaintext fallback.
9. The running service never writes its configuration. The configuration file is read-only input, so
   the container's read-only root filesystem stays intact and the key that protects an encrypted
   file never lives beside the file it protects.

## Open decisions

- Authentication mechanism and token/session lifetime — **implemented**: a short-lived JWT access token
  (default 15 minutes) plus a rotating opaque refresh token (default 14 days), no database
  ([authentication-decision.md](authentication-decision.md); #130).
- Authorization roles and resource ownership model — **implemented**: project-scoped RBAC, a role
  (`viewer` < `editor` < `owner`) granted per project, with a `systemAdmin` global bypass
  ([authentication-decision.md](authentication-decision.md)).
- Configuration model and the storage of secrets — **decided**: environment variables stay
  authoritative and an optional single configuration file is added as a second source, resolved
  per key in the order environment, file, default; secrets may live in the file but never *only* in
  it, encryption is authenticated with an externally supplied key, and a configuration that cannot
  be resolved refuses to boot ([configuration-decision.md](configuration-decision.md); #187). The
  loader, the encryption, and the precedence rules are tracked in #188–#190.
- Maximum request, JSON, attachment, and nesting sizes
- Locking implementation and overwrite/conflict semantics
- Whether to reject unknown JSON fields during the compatibility period
- Supported operating systems and filesystem behavior

These decisions must be resolved before the HTTP compatibility layer is exposed beyond a trusted local network.

## Implementation Status

### Completed (as of 2026-09-11)

- ✅ Threat model documented with trust boundaries and abuse cases
- ✅ Security invariants defined and enforced in code
- ✅ Path traversal tests added (`tests/security_tests.rs`)
- ✅ Symlink escape tests added
- ✅ Malformed JSON tests added
- ✅ Data integrity and concurrent write tests added
- ✅ Negative security fixtures created (`compatibility/endpoints/`)
- ✅ Unknown field rejection implemented via `deny_unknown_fields`
- ✅ Atomic write semantics implemented in repository layer
- ✅ Container image scanning in CI (security workflow `container-scan` job) — temporarily skipped
  since PR #349 (runner saturation, 2026-09-24); re-enabling it and its required status check is
  tracked in #362. The release-time scan of the published digest (`release.yml`, #330) still runs.
- ✅ SBOM generation for release artifacts (security workflow `sbom` job) — temporarily skipped by
  the same PR #349, restored by the same #362; publishing the SBOM with the artifact is #331
  (audit finding F-178-5).
- ✅ Authentication and authorization implemented (#130): HS256 access tokens, rotating refresh
  tokens, Argon2id password hashes, and project-scoped RBAC enforced in every guarded handler.
  Passwords and refresh tokens are stored only as hashes; the auth matrix is `tests/auth.rs`.
- ✅ The shipped Compose stack authenticates by default (#278, merged 2026-09-17): the shipped
  `docker-compose.yml` resolves `TUCANO_AUTH_REQUIRED` to `true` unless the operator overrides it,
  so the shipped configuration no longer combines an all-interfaces publish with authentication
  off — the exposure audit finding F-178-1 measured in
  [audit-s3-container-and-deployment.md](audit-s3-container-and-deployment.md). The service-level
  default when the variable is absent entirely remains off (`src/auth/config.rs`), so a deployment
  that does not use the shipped file must turn authentication on explicitly.

### Decided (documented, not yet implemented)

- ✅ Configuration model and the secrets model decided (#187) in
  [configuration-decision.md](configuration-decision.md): environment variables stay authoritative,
  an optional single configuration file is added as a second source, and the resolved order is
  environment, then file, then default — per key. The two trust boundaries above (the configuration
  file and its key) and invariants 8 and 9 were added with it. The implementation is tracked in
  #188–#190; until those land, this repository reads its configuration from the environment only,
  exactly as the *Current state* section of that decision records.
- ✅ The configuration file's schema and loader landed (#188): `src/config.rs` reads only the file
  named by the environment-only `TUCANO_CONFIG_FILE`, refuses an unknown key, an unsupported
  `version`, a malformed document and an unreadable (including missing) file at startup, and none of
  its error text carries a secret value or a raw path. Precedence is per key — environment, then
  file, then default — and no `TUCANO_CONFIG_FILE` means no file is consulted at all, so the
  env-only behaviour is unchanged. Invariant 8 is therefore now enforced in code rather than only
  decided. **The file has no encryption yet**: #189's AEAD envelope and externally supplied key are
  still pending, so a secret held in the file is in the clear and the *Configuration key* boundary
  above is not yet exercised.

### Known limitations

- **Run scope can be narrowed by the caller that holds the run.** A run's reachable projects are the
  projects its `projects` array names, and an `editor` may update that array, so a caller with write
  access to a run can shrink the run's scope (for itself and for everyone else). The alternative — a
  fixed scope stamped at creation — is deliberately deferred; raising a run's privileges is not
  possible, so the weakness is a denial of access rather than an escalation.
- **Authorization is decided before existence.** Because a handler checks the caller against the
  project it names before it loads the resource, a restricted caller can receive `403` where an
  anonymous-old deployment would have answered `404` for an identifier that does not exist. With
  `TUCANO_AUTH_REQUIRED` off every guard returns, so behaviour is unchanged.
- **There is no grant-administration endpoint.** Accounts and grants are read from
  `TUCANO_DATA_DIR/auth/`, which must be provisioned out of band; creating a project does not grant
  its creator a role, so a project can exist with no grant-holder until an administrator adds one.
- **File encryption protects against accidents, not against an actor who can read the volume.** Once
  the encrypted configuration file exists, its key is supplied from outside it, so an attacker who
  can read the mounted volume cannot read the key from the same place — but an attacker who can read
  the *environment* of the container, or the secret mount, still can. Encryption of the file raises
  the cost of a leaked backup or an accidental commit; it is not a control against host compromise,
  and it must not be described as one.
- **The auto-merge path carries its authority in a personal access token.** The `Auto Merge`
  workflow runs with `secrets.AUTO_MERGE_TOKEN || github.token`. When the secret is set, that arm
  is a personal access token of the repository owner, and it can satisfy the pull request's
  *review* requirement — the built-in `GITHUB_TOKEN` cannot. The project keeps this token
  deliberately (owner decision recorded in #336, audit finding F-179-3): the repository is
  owner-operated, and the trade buys merged-without-a-human-second-approval in exchange for the
  full suite passing. Compensating gates, enforced by the workflow itself: it fires only as a
  `workflow_run` after the PR's own CI, and it merges only when the head is a branch of this
  repository, the base is the default branch, the author's collaborator permission is exactly
  `admin`, the pull request is open, not a draft, and mergeable (a `behind` head is brought
  up to date and retried, never parked silently), and every check run reported against the head
  concluded `success`, `skipped`, or `neutral` — under `strict` branch protection that means all
  required checks green on an up-to-date head. Unsetting the secret degrades gracefully to the
  built-in token, which cannot bypass the review requirement — it is a downgrade, never an
  escalation. Rotation belongs to org owners (repository secret); a leaked PAT is equivalent on
  this repository to a compromised owner credential for the merge path, and the response is the
  same: revoke at GitHub and rotate the secret. The token lives in CI only — it does not touch any
  boundary, asset, or runtime surface of the deployed service.

### Pending

- ⏳ Grant-administration endpoints (create accounts, grant roles) — no API surface yet
- ⏳ Contract tests against Node reference implementation
- ⏳ Encrypted secrets at rest (#189) and the full precedence-and-validation matrix across file,
  environment, and defaults (#190). The schema and loader half of the configuration file (#188) has
  landed; the decision is recorded in [configuration-decision.md](configuration-decision.md), and
  the boundary rows, invariants, and abuse cases above are the requirements those remaining tickets
  must satisfy.
