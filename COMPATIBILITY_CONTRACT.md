# API Compatibility Contract

## Status

This contract is the baseline for the Rust evaluation. As of 2026-09-02, this repository contains no Rust HTTP service, Node reference service, endpoint implementation, JSON fixtures, or Swagger document. No endpoint behavior is assumed or invented here.

The existing TucanoTCM implementation remains the behavioral authority. Capture observations from that implementation before declaring a Rust endpoint compatible.

## Fixture layout

When the reference API is available, store sanitized fixtures under `compatibility/` using this layout:

```text
compatibility/
  endpoints/<resource>/<operation>-<case>.json
  responses/<resource>/<operation>-<case>.json
  persistence/<resource>/<operation>-<case>.json
  openapi.json
```

Fixtures must not contain credentials, tokens, personal data, production paths, or real attachment contents.

## Required observation record

Each endpoint case must record:

- HTTP method and path
- Query, path, and header inputs
- Request body and content type
- Authentication and authorization context
- Status code and response headers
- Response JSON shape and error envelope
- Request ID behavior and correlation headers
- Filesystem files changed, including exact JSON format
- Repeat-request and concurrent-request behavior

## Compatibility rules

1. Preserve status codes, response shape, required headers, and documented error codes unless a deviation is approved.
2. Preserve JSON field names, types, nullability, date formats, and omission behavior.
3. Preserve file names and JSON formats during the evaluation; incompatible changes require explicit versioning.
4. Compare persistence effects as well as HTTP responses.
5. The GUI consumes this same HTTP contract and never accesses storage directly.
6. Internal paths, stack traces, raw filesystem errors, secrets, and file contents must never appear in client errors or logs.
7. Rust deviations must be listed with rationale, migration impact, and a test proving the new behavior.

## Required case matrix

| Case | Expected evidence |
| --- | --- |
| Create valid resource | Success status, response, and persisted JSON |
| Read existing resource | Success status, headers, and exact representation |
| List resources | Ordering, pagination, empty result, and limits |
| Update valid resource | Replacement/merge semantics and persistence |
| Delete existing resource | Status and missing-resource behavior |
| Missing resource | Status and stable error envelope |
| Missing required field | Validation status and field details |
| Unknown field | Reject, ignore, or preserve behavior |
| Malformed JSON | Parse error status and safe response |
| Unauthorized request | Authentication status and response shape |
| Forbidden request | Authorization status and response shape |
| Conflict/concurrent update | Conflict behavior and file integrity |
| Traversal or symlink path | Rejection without access outside data root |
| Oversized request or attachment | Bounded rejection without unbounded allocation |

## Definition of done for the baseline

- Reference endpoint observations are captured in sanitized fixtures.
- A Swagger/OpenAPI document is checked in and matches the observed routes.
- Contract tests compare Node and Rust status, headers, bodies, and persistence effects.
- Negative security cases cover every threat in [THREAT_MODEL.md](THREAT_MODEL.md).
- Every approved deviation is documented before migration work begins.
