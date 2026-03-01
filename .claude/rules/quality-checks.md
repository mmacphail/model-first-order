# Quality Checks

After completing any code change, **always** run these checks before presenting the work as done:

1. `cargo fmt -- --check` — fix any formatting issues with `cargo fmt`
2. `cargo clippy -- -D warnings` — fix all warnings
3. `cargo test` — ensure all unit/integration tests pass

If any check fails, fix the issue and re-run until all three pass.
