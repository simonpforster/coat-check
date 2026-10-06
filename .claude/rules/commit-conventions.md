This project uses [Conventional Commits](https://www.conventionalcommits.org/) with release-please for automated versioning.

## Commit types and release impact

| Type | Release? | Use when |
|------|----------|----------|
| `feat` | minor bump | New functionality visible to users or API consumers |
| `fix` | patch bump | Bug fix, or any change that should trigger a new release (e.g. Dockerfile changes that need a new image) |
| `chore` | **no release** | Tooling, deps, CI config that doesn't need a new artifact |
| `refactor` | **no release** | Internal restructuring with no behavioral change |
| `docs` | **no release** | Documentation only |
| `test` | **no release** | Adding or updating tests |
| `ci` | **no release** | CI/CD pipeline changes |
| `style` | **no release** | Formatting, whitespace |

## Scopes

Use the crate or component name as scope: `feat(api):`, `fix(web):`, `fix(db):`, `feat(backoffice):`.

## Key rule

If a change **needs a new release artifact** (Docker image, crate publish), it **must** use `feat` or `fix` — not `refactor`, `chore`, or `ci`. release-please only bumps versions for `feat`, `fix`, and `BREAKING CHANGE`.
