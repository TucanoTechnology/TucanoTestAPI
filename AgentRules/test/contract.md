# Contract Test Policies

Contract tests should verify that producers and consumers agree on the structure and behavior of exchanged JSON data.

## Validation coverage

- Test valid payloads against the checked-in schema or API contract.
- Test malformed JSON, invalid types, missing required fields, unknown fields, boundary values, and oversized input.
- Verify invalid payloads are rejected before persistence or other side effects occur.
- Verify validation errors use the documented status, error shape, and field-level details.
- Verify sensitive values and implementation details are not included in validation errors.
- Test each supported schema version and document compatibility expectations.

## Test practices

- Keep contract tests deterministic and independent of external services where possible.
- Use representative fixtures that cover normal, boundary, and invalid payloads.
- Update contract tests whenever a schema or API payload changes.
- Run contract tests in CI before merging changes that affect an exchanged payload.