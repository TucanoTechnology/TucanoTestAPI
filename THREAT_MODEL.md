# Rust Service Threat Model

## Scope

This threat model covers the planned Rust HTTP service, its JSON filesystem repository, attachment handling, and the future GUI client. The current repository is an evaluation skeleton and does not expose HTTP endpoints yet.

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
| Unauthorized access | Authenticate before protected operations and authorize by resource/action | Auth matrix tests |
| Vulnerable dependency or image | Run advisory, secret, and container scans in CI | Security workflow and clean-baseline checks |

## Security invariants

1. The configured data root is the only filesystem area the repository may read or write.
2. Persistence publishes complete documents atomically; failed writes do not replace valid data.
3. Client-visible errors use stable codes and safe messages with request IDs.
4. The GUI never reads or writes JSON files directly.
5. `unsafe` Rust is forbidden by the crate lint unless the policy is explicitly revised and reviewed.
6. Security-sensitive events exclude credentials, tokens, raw payloads, attachment contents, and internal paths.

## Open decisions

- Authentication mechanism and token/session lifetime
- Authorization roles and resource ownership model
- Maximum request, JSON, attachment, and nesting sizes
- Locking implementation and overwrite/conflict semantics
- Whether to reject unknown JSON fields during the compatibility period
- Supported operating systems and filesystem behavior

These decisions must be resolved before the HTTP compatibility layer is exposed beyond a trusted local network.
