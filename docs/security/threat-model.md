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
| Resource ID or filename to filesystem | Traversal, absolute paths, separators, symlinks | Allow-list validation, canonical confinement, symlink checks |
| Service to stored JSON | Corruption, partial writes, concurrent writers | Same-directory temp file, flush, atomic rename, locking policy |
| Attachment upload to storage | Oversized or unexpected file | Streaming limits, type/size validation, safe permissions |
| Service logs and audit events | Secrets, paths, file contents | Structured redaction and bounded fields |
| GUI to storage | Direct filesystem access | GUI uses the documented HTTP API only |

## Abuse cases and required behavior

| Threat | Requirement | Verification |
| --- | --- | --- |
| Malicious JSON | Reject malformed, oversized, deeply nested, or schema-invalid input with a stable safe error | Negative parser and schema tests |
| Path traversal | Reject `..`, absolute paths, forbidden separators, and any path escaping the configured root | Unit and integration path tests |
| Symlink escape | Refuse symlink-based escape from the configured root and unexpected file types | Symlink fixture tests |
| Oversized upload | Enforce request, JSON, attachment, and concurrency limits before unbounded allocation | Limit and streaming tests |
| Corrupted file | Return a safe storage error and preserve the original file | Corruption and recovery tests |
| Concurrent write | Define lock and overwrite behavior; never publish a partial JSON document | Concurrent writer and atomicity tests |
| Denial of service | Bound body size, JSON depth, filesystem work, uploads, and request duration | Timeout, cancellation, and resource-limit tests |
| Data disclosure | Never return internal paths, stack traces, raw filesystem errors, secrets, or file contents in logs | Error and log-redaction tests |
| Unauthorized access | Authenticate before protected operations and authorize by resource/action | Auth matrix tests (`tests/auth.rs`; hand-marked public surface in `tests/service.rs`) |
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

## Open decisions

- Authentication mechanism and token/session lifetime — **implemented**: a short-lived JWT access token
  (default 15 minutes) plus a rotating opaque refresh token (default 14 days), no database
  ([authentication-decision.md](authentication-decision.md); #130).
- Authorization roles and resource ownership model — **implemented**: project-scoped RBAC, a role
  (`viewer` < `editor` < `owner`) granted per project, with a `systemAdmin` global bypass
  ([authentication-decision.md](authentication-decision.md)).
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
- ✅ Container image scanning in CI (security workflow `container-scan` job)
- ✅ SBOM generation for release artifacts (security workflow `sbom` job)
- ✅ Authentication and authorization implemented (#130): HS256 access tokens, rotating refresh
  tokens, Argon2id password hashes, and project-scoped RBAC enforced in every guarded handler.
  Passwords and refresh tokens are stored only as hashes; the auth matrix is `tests/auth.rs`.

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

### Pending

- ⏳ Grant-administration endpoints (create accounts, grant roles) — no API surface yet
- ⏳ Contract tests against Node reference implementation
