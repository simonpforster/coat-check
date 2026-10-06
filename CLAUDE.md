# Project Rules

## API OpenAPI documentation

Every HTTP handler in the API crate (`crates/api`) must have a `#[utoipa::path]` annotation with correct method, path, request body, path/query params, all response status codes, and a tag. All request/response DTOs must derive `ToSchema`. New endpoints must be registered in the `ApiDoc` struct's `paths` and `components(schemas(...))` lists. The Swagger UI at `/swagger-ui` must always reflect the full API surface.

## Commit conventions and release-please

This project uses [Conventional Commits](https://www.conventionalcommits.org/) with release-please. **GitHub squash-merges use the PR title as the commit message**, so the PR title must follow the same convention.

| Type | Release? | Use when |
|------|----------|----------|
| `feat` | minor bump | New functionality |
| `fix` | patch bump | Bug fix, or any change needing a new release artifact (Docker image, crate) |
| `chore` | no release | Tooling, deps, CI config |
| `refactor` | no release | Internal restructuring |
| `docs`, `test`, `ci`, `style` | no release | Self-explanatory |

**Scopes**: use crate/component name — `feat(api):`, `fix(db):`, `feat(backoffice):`.

**Key rule**: if a change needs a new Docker image or crate release, the **PR title** must use `feat` or `fix`. `refactor`, `chore`, etc. will not trigger a version bump.
