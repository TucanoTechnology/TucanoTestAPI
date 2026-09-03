# Security and Commit Rules

- Follow the secret handling requirements in `./secret-protection.md`.
- Protect `main` with branch protection rules; all changes to `main` must merge through a pull request.
- Require at least one approving review and all required CI checks to pass before a pull request can merge into `main`.
- Keep pull request branches up to date with `main` before merging into `main`.
- Do not bypass branch protection or required security checks.
- Security-sensitive changes require review by an appropriate project maintainer.
- Pull requests that expose secrets or introduce unresolved security risks must not be merged.
