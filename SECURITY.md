# Security Policy

SessionMesh processes agent transcripts, file contents, command output, tokens,
and other potentially sensitive local data. Security and provenance are product
requirements, not optional deployment hardening.

## Reporting a vulnerability

Do not open a public issue containing exploit details, credentials, session
content, or private paths. Use GitHub's private vulnerability-reporting feature
for the repository when available. If private reporting is unavailable, open a
minimal public issue requesting a private contact channel without including
sensitive details.

## Repository hygiene

- Never commit `.env`, local SessionMesh settings, authentication exports,
  credentials, private keys, tokens, runtime databases, or native session data.
- Use `.env.example` only for documented names and non-secret defaults.
- Keep test fixtures synthetic and review them before publication.
- Remove a leaked secret from history and rotate it immediately; adding the
  file to `.gitignore` does not invalidate an already published credential.
- Run `npm run check` before publication. Its repository-safety tests enforce
  the baseline ignore and template contract.

## Runtime defaults

- Native agent stores are opened read-only.
- The HTTP service binds to loopback unless network access is explicitly
  enabled.
- Database and blob files use user-only permissions on Unix.
- Raw input is redacted before optional model or embedding processing.
- Remote endpoints and arbitrary adapter scripts require explicit opt-in.

The complete architectural controls are documented in the
[deployment model](docs/architecture/deployment.md) and
[storage architecture](docs/architecture/storage.md).
