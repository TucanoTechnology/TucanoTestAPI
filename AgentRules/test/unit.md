# Unit Test Policies

## Test structure

Unit tests should verify one unit of behavior at a time.

Recommended structure:
- Arrange: prepare inputs and mocks
- Act: execute the function or module behavior
- Assert: verify the expected result

## Best practices

- Keep tests small and focused.
- Mock only what is outside the unit boundary.
- Cover success, failure, and edge cases.
- Prefer behavior verification over implementation verification.
- Keep setup minimal and readable.
- Avoid over-mocking where it harms confidence.
- Use tests as executable documentation for expected behavior.
- Keep unit tests fast so they can run frequently in CI.
