# Playwright Test Policies

## Test structure

Playwright tests should validate user journeys in a clear and maintainable way.

Recommended structure:
- Arrange: set up required data and state
- Act: navigate, click, fill, or interact with the UI
- Assert: confirm the visible behavior and result

## Best practices

- Prefer stable selectors such as `data-testid`.
- Test user-visible behavior, not internal implementation details.
- Avoid arbitrary sleeps or fixed waits.
- Wait for specific conditions instead of timing assumptions.
- Keep tests isolated and reset state between runs.
- Reuse helper functions for common flows like login or navigation.
- Make screenshots or traces available when debugging failures.
- Keep assertions focused on observable outcomes.
