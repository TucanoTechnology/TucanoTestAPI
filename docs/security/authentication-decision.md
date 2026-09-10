# Authentication Decision (Issue #107)

Issue: [#107](https://github.com/TucanoTechnology/TucanoTestAPI/issues/107) — make and document the
authentication decision. Split from [#14](https://github.com/TucanoTechnology/TucanoTestAPI/issues/14)
(operational hardening). This document is the decision of record; implementation is tracked separately.

## Decision

**Authentication is deferred, and the approach is fixed.** No authentication or authorization code is written
under this issue. When it is implemented, it will follow the approach below.

| Aspect | Decision |
| --- | --- |
| Timing | Deferred. No user model or identity-bearing consumer exists today, so auth is not a release gate; the deployment stays on a trusted network. |
| Authorization model | **Project-scoped RBAC.** Permissions and access are granted **per project by role** (for example owner / editor / viewer). The project is the authorization boundary; a role is scoped to a project, not global. |
| Authentication mechanism | **Short-lived JWT access token plus a refresh token.** Stateless and database-free. |
| Persistence | **No database.** The decision must not introduce one; the file-based deployment and its persistent volume remain the only state. |
| Authentication middleware | **Open.** Whether a distinct auth middleware/component is required (versus inline checks) is deliberately left to a later decision. |

The intended balance is: no database, no per-request file I/O for the common case, and revocation risk bounded
by the access-token expiry window, with refresh handling carrying the revocation window.

## Current state (verified against `main`)

- The service has **no authentication or authorization**: there are no user, role, or token types and no auth
  middleware in `src/`.
- `openapi.json` publishes **no `securitySchemes`** and no `security` requirement; the `info.description` says
  only that data is stored as JSON with no database.
- The [threat model](threat-model.md) already lists "Unauthorized access" as **not yet implemented** and states
  the service must not be exposed beyond a trusted network until auth lands.
- The architecture record already places an "authentication boundary" and "authorization" in the target
  architecture ([rust-service-core.md](../architecture/rust-service-core.md)).

## Rationale

- **No identity to authenticate yet.** The API has no users or roles, and the primary consumer (the GUI) has not
  defined a user model. Building hooks speculatively is explicitly out of scope for #14.
- **The no-database constraint drives the mechanism.** A stateless JWT avoids a server-side session store and
  per-request file reads, which keeps the file-based, portable deployment model intact.
- **Short expiry bounds the revocation trade-off.** Stateless tokens cannot be revoked instantly; a short
  access-token lifetime keeps that window small without a database, and the refresh token carries renewal.
- **The project is the natural boundary.** Suites and cases live inside projects (the real-home tree of #65), so
  authorizing per project by role maps onto the storage model with no new top-level concept.

## Requirements this places on the implementation

1. No database, and no per-replica session state — consistent with `AGENTS.md` and the stateless-replica model.
2. Principals and their per-project roles must be stored without a database; if any legacy Draft 2020-12 schema
   gains a field, that is a breaking change and needs its own versioning plan before code.
3. The security scheme must be published in `openapi.json` and usable through Swagger UI; today the document
   records none.
4. The "Unauthorized access" abuse case in the [threat model](threat-model.md) must gain its auth-matrix tests.
5. The API must **not** be exposed beyond a trusted network until this is implemented.

## Tracking

Implementation is tracked in **[#130 — Auth: implement authentication](https://github.com/TucanoTechnology/TucanoTestAPI/issues/130)**,
which carries this approach as its description, alongside the parent hardening ticket #14.
