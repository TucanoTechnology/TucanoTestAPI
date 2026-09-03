# Git Usage Policy

- Follow the Gitflow branch roles and targets defined in `branch-strategy.md`.
- Do not commit directly to `main` or `develop`.
- Use conventional commit messages: `feat:`, `fix:`, `docs:`, `chore:`, `test:`
- Keep commits focused — one logical change per commit.
- Reference issue or ticket numbers in commit messages when applicable.
- Rebase only unpublished branches. Do not rewrite shared history unless explicitly approved.
- Merge only through pull requests, after review, required CI checks, and branch protection checks pass.
- Keep pull request branches up to date with their actual Gitflow target branch before merging.
- Follow the secret handling requirements in `../security/secret-protection.md`.
