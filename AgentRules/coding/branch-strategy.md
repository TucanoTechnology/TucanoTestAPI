# Branch Strategy

Follow Gitflow as the branching strategy:
- Use `main` for production-ready releases only.
- Use `develop` as the integration branch for ongoing work.
- Create feature branches from `develop`: `feature/your-feature-name`
- Create release branches from `develop`: `release/x.y.z`
- Create hotfix branches from `main`: `hotfix/your-fix-name`
- Merge feature branches into `develop` through pull requests.
- Merge release branches into both `main` and `develop` through pull requests.
- Merge hotfix branches into both `main` and `develop` through pull requests.

When developing a feature or carrying out a ticket:
- Create the branch from the correct Gitflow base branch.
- Do not commit directly to `main` or `develop`.
- Keep the branch up to date with its pull request target branch.
- Resolve conflicts locally before requesting review.
- Open a pull request when the work is ready.
- Merge through the pull request after review and required checks pass.
