# Workflow

- Every change must have a ticket — if you're working on something and no ticket exists, create one first
- Update tickets as you work — add comments documenting progress, decisions, and blockers
- Mark tickets complete — when done, add a final comment summarizing what was accomplished and close the ticket
- Link PRs to tickets — reference ticket numbers in PR descriptions and commit messages

## CI/CD Validation

**All CI/CD jobs must pass locally before committing and raising a PR.**

Before pushing changes:

1. Run all linting and formatting checks locally
2. Run all unit and integration tests locally
3. Build the production image/bundle locally (if applicable)
4. Verify all checks pass before committing

This prevents wasted CI cycles and enables faster iteration. If CI fails after push, fix
immediately before working on other tasks.

Repository-specific commands are documented in each project's `AGENTS.md` file.

## PR Merge Dependencies

When a PR depends on another PR being merged first:

1. **Add dependency to PR title** — Use format: `[Depends on #XX]` or `[Blocked by #XX]`
2. **Document in description** — Explain why the dependency exists
3. **Rebase before merge** — Always rebase on main before final merge to resolve conflicts

**Example:**
- PR Title: `feat: tags UI component [Depends on #58]`
- Description: "This PR implements the tags UI. Depends on API PR #58 (tags support) being merged first."

**Dependency Order:**
- API changes before GUI changes that use them
- Foundation features before dependent features
- Breaking changes before migrations
