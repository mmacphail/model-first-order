# Quality Checks

After completing any code change, **always** run `just quality` before presenting the work as done.

This runs: `just gen` → `cargo fmt --check` → `cargo clippy` → `cargo test`.

If any step fails, fix the issue and re-run until it passes.
