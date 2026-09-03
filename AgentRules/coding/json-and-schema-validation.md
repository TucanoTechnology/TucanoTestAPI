# JSON and Schema Validation

- Validate every external JSON payload against the checked-in schema or API contract before business logic or persistence.
- Validate all create and update operations, including POST, PUT, and PATCH requests.
- Reject malformed JSON, invalid types, missing required fields, unknown fields, oversized values, and schema-incompatible values.
- Apply validation at the system boundary and do not rely on client-side validation alone.
- Return a consistent client error for validation failures that identifies the affected fields without exposing internals, secrets, or stack traces.
- Do not partially persist invalid input; validation must complete before a write or side effect occurs.
- Keep schemas versioned with the code that consumes them and document intentional breaking changes.
- Use the contract test requirements in `../test/contract.md` to verify schemas and validation behavior.
