# Performance Test Policies

## Test structure

Performance tests should be structured to measure a single performance characteristic at a time.

Recommended structure:
- Arrange: define the system state, dataset, and environment
- Act: run the workload or operation under test
- Measure: record timing, throughput, memory, or resource usage
- Assert: compare results to the expected threshold or baseline

## Best practices

- Measure one metric clearly whenever possible.
- Use a stable environment to reduce noise.
- Compare results against a known baseline.
- Define acceptance thresholds before running the test.
- Keep performance tests separate from fast unit test suites.
- Document what the test is measuring and why it matters.
- Avoid flaky tests caused by environment variance.
- Use repeat runs when needed to smooth out noise.
