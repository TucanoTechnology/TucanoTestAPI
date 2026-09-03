# Code Review Best Practices

## Review goals

Code reviews should improve:
- Correctness
- Security
- Maintainability
- Test coverage
- Readability
- Consistency with project policies

## Reviewer expectations

Reviewers should:
- Read the change with the feature intent in mind
- Check for hidden edge cases and failure modes
- Verify tests are appropriate and meaningful
- Confirm security-sensitive logic is handled carefully
- Look for regressions in existing behavior
- Check whether the change matches the repository structure and policy files
- Ask for clarification when the design or intent is unclear

## Review checklist

- Is the change scoped appropriately?
- Does the code follow repository conventions?
- Are tests present and meaningful?
- Are performance implications understood?
- Are there security concerns or data handling risks?
- Are error messages and edge cases handled well?
- Does the change avoid unnecessary complexity?
- Is the documentation updated if needed?

## Best practices for authors

Authors should:
- Keep pull requests small and focused
- Include context in the PR description
- Explain tradeoffs and known limitations
- Link related tickets or requirements
- Add or update tests for behavior changes
- Call out any intentionally skipped checks or exceptions
- Respond clearly to reviewer questions

## Best practices for reviewers

Reviewers should:
- Be specific in feedback
- Separate must-fix issues from suggestions
- Focus on the impact of the change
- Prefer actionable comments
- Approve once the code is safe, tested, and understandable

## When to block approval

A review should block approval when:
- Tests are missing for important behavior
- Security or data handling is unclear
- The change introduces a regression
- The implementation is difficult to understand and cannot be justified
- The PR conflicts with repository policy and the conflict is not documented
