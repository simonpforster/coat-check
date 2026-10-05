# Project Rules

## API OpenAPI documentation

Every HTTP handler in the API crate (`crates/api`) must have a `#[utoipa::path]` annotation with correct method, path, request body, path/query params, all response status codes, and a tag. All request/response DTOs must derive `ToSchema`. New endpoints must be registered in the `ApiDoc` struct's `paths` and `components(schemas(...))` lists. The Swagger UI at `/swagger-ui` must always reflect the full API surface.
