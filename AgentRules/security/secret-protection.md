# Secret Protection

- Never commit sensitive data, including:
  - API keys and tokens
  - Passwords and secrets
  - Private keys and certificates
  - Database credentials
  - Environment-specific configuration
- Use environment variables or `.env` files for secrets, and ensure `.env` files are ignored.
- If a secret is exposed, revoke or rotate it immediately and report the exposure through the approved security process.