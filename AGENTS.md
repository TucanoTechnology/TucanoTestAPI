# AI Agent Rules — TucanoTCM

This file contains rules and guidelines for AI agents working on the TucanoTCM project.

---

## Core Project Philosophy

**TucanoTCM is a file-based test case management system.** All test data (projects, test cases, test suites, test runs) is stored as JSON files on the filesystem. There is no database. The system is designed to be portable via container + local storage mount.

When making decisions about features, architecture, or implementation:
- Preserve the file-based approach — all CRUD operations read/write JSON files in `TucanoTCM/json_files/`
- Do not introduce databases, ORMs, or external storage services
- Ensure the container works with just a volume mount for the data directory
- Keep the system simple, inspectable, and movable
- **GUI and API are equal citizens** — all actions can be performed via the graphical interface or directly via API calls. Neither is secondary; both must support the same functionality
- **All API functionality must be documented in Swagger** — every endpoint, parameter, and response must be captured in the OpenAPI/Swagger specification. If it's not in Swagger, it doesn't exist

---

## Development Workflow

### Branch Strategy
When developing a new feature or carrying out a ticket:
1. **Create a new branch** for that feature: `git checkout -b feature/your-feature-name`
2. **Work on the feature branch** — do not commit directly to `main`
3. **Rebase onto main before pushing** — run `git fetch origin && git rebase origin/main` to check for conflicts and ensure your branch is up-to-date
4. **Resolve any conflicts** — if rebase reveals conflicts, resolve them locally before pushing
5. **Open a PR** when ready for review
6. **Merge via PR** after verification

### Dependencies
When adding a feature or new dependency:
- **Always check online for the latest stable version** before adding
- Use `npm view <package> version` or check the package's official repository
- Do not assume the version in `package.json` is current — verify before installing
- Prefer well-maintained, stable releases over bleeding-edge versions

---

## Code Standards

### File Structure
- Routes: `TucanoTCM/routes/<resource>.js`
- Schemas: `TucanoTCM/schemas/<resource>.schema.json`
- Tests: `TucanoTCM/test/<resource>.test.js`
- Data: `TucanoTCM/json_files/<resource>/` (runtime, mounted via Docker volume)

### JSON Schema Validation
- All API payloads must be validated against JSON schemas using AJV
- Schemas use Draft 2020-12 with local `$ref` resolution
- Validate on POST and PUT operations

### API Design
- RESTful endpoints for each resource
- OpenAPI/Swagger documentation in route files
- Consistent structured JSON error responses
- Secure filenames and prevent path traversal attacks

### Testing
- Write tests for new features using Node's built-in test runner
- Run tests with: `npm test`
- Cover CRUD operations for each resource
- Test file persistence and volume mount behavior

---

## Docker & Deployment

### Local Development
```bash
docker compose up -d --build
```

### Ports
- WAF: `8080` (host) → `80` (container)
- App: `3001` (host) → `80` (container)

### Volume Mount
- Host: `./data` → Container: `/app/json_files`
- Data persists across container restarts
- Files are plain JSON — inspectable and editable on host

### WAF
- Uses external repository: `ECiurleo/owasp-crs-nginx-waf`
- Built via Docker Compose from remote Git context
- Provides ModSecurity protection with OWASP CRS

---

## Security

- Validate all input against JSON schemas
- Sanitize filenames to prevent path traversal
- Use atomic writes for file operations
- Implement overwrite protection where appropriate
- Keep dependencies updated (check for security advisories)

---

## Documentation

### README.md
- **Always keep README.md up to date** — it must reflect the latest description, build process, goals, and feature set
- Update README whenever you:
  - Add new features or endpoints
  - Change the build or deployment process
  - Modify the project structure
  - Add new dependencies or requirements
- README should include:
  - Project overview and goals
  - Quick start guide (how to build and run)
  - Feature list with brief descriptions
  - API documentation link (Swagger UI)
  - Configuration options
  - Testing instructions

### Code Documentation
- Update README.md for user-facing changes
- Add JSDoc comments for new API endpoints
- Keep Swagger/OpenAPI specs in sync with routes
- Document any changes to file format or schema

---

## Git & Version Control

- **Protect the main branch** — never commit directly to `main`. All changes must go through pull requests
- Use conventional commit messages: `feat:`, `fix:`, `docs:`, `chore:`, `test:`
- Keep commits focused — one logical change per commit
- Reference issue/ticket numbers in commit messages when applicable
- **NEVER commit sensitive data** — this includes:
  - API keys and tokens (GitHub, AWS, etc.)
  - Passwords and secrets
  - Private keys and certificates
  - Database credentials
  - Any environment-specific configuration
  - Use environment variables or `.env` files (added to `.gitignore`) instead
- **Require CI to pass** — PRs must pass all CI checks before merging
- **Require up-to-date branches** — branches must be up-to-date with main before merging

---

## Project Board Management

**Project board:** https://github.com/users/ECiurleo/projects/1

### Ticket Requirements
Every ticket on the project board must have:

1. **Description** — clear explanation of what needs to be done and why
2. **Definition of Done** — specific criteria that must be met for the ticket to be considered complete
3. **Labels/Tags** — appropriate labels (e.g., `enhancement`, `documentation`, `security`, `bug`, `testing`)
4. **Complexity Estimate** — effort estimation (e.g., `small`, `medium`, `large` or story points)
5. **Priority** — one of:
   - **Security** — highest priority, security-related issues
   - **Bug fix** — fixing broken functionality
   - **Feature** — new functionality or enhancements

### Workflow
- **Every change must have a ticket** — if you're working on something and no ticket exists, create one first
- **Update tickets as you work** — add comments documenting progress, decisions, and blockers
- **Mark tickets complete** — when done, add a final comment summarizing what was accomplished and close the ticket
- **Link PRs to tickets** — reference ticket numbers in PR descriptions and commit messages

### Ticket Template
When creating a new ticket, include:
```
**Description:**
What needs to be done and why.

**Definition of Done:**
- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Tests pass
- [ ] Documentation updated
- [ ] PR reviewed and merged

**Labels:** [appropriate labels]
**Complexity:** [small/medium/large]
**Priority:** [security/bug fix/feature]
```

---

## Questions?

If these rules conflict with a ticket or requirement:
1. Prioritize the file-based philosophy
2. Clarify with the project maintainer
3. Document any exceptions in the PR description
