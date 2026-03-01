# Parallel Safety Rules

These rules prevent multiple AI agents from stepping on each other when working in parallel.

## Worktree Requirement

**Always work in a git worktree** when modifying code. Never commit directly to `main` from a shared checkout. If you are not already in a worktree (check: your working directory should be under `.claude/worktrees/`), request one before making changes.

## File Locking by Convention

Before modifying any file listed as a "High-Conflict Zone" in `.claude/MULTI_AGENT.md`, check the scratchpad at `.claude/scratchpad/STATUS.md` (if it exists) to see if another agent is already working on that file. If so, coordinate or wait.

High-conflict files that require single-agent access:
- `src/routes.rs`
- `src/lib.rs`
- `src/handlers/mod.rs`
- `src/models/mod.rs`
- `Cargo.toml`
- Any `migrations/*/` directory (creating new migrations)

## Schema and Generated Files

- **Never hand-edit `src/schema.rs`** — it is overwritten by `diesel migration run`.
- **Never hand-edit `openapi.json`** — it is overwritten by `just gen`.
- If your changes affect the API surface, run `just gen` as the final step before committing.

## Test Isolation

- **Integration tests** (`cargo test`) are safe to run in parallel. Each test uses testcontainers to spin up a disposable Postgres on a random port.
- **E2E tests** (`just test-e2e`) are NOT safe to run in parallel. They use the shared Docker Compose stack with fixed ports. Only one agent at a time.
- When adding new tests, always use the existing testcontainers pattern from `tests/order_lifecycle.rs` — never hardcode ports or share databases between tests.

## Dependency Changes

If you need to add a crate to `Cargo.toml`:
1. Check that no other agent is currently modifying `Cargo.toml`.
2. Add your dependency in the correct alphabetical position within the `[dependencies]` section.
3. Run `cargo check` immediately to update `Cargo.lock`.
4. Commit both `Cargo.toml` and `Cargo.lock` together.

## Migration Ordering

Diesel migrations use timestamp prefixes for ordering. If two agents both need migrations:
1. **Do not create them in parallel.** The second migration may see a stale `src/schema.rs`.
2. Sequence the work: the first agent creates their migration and merges, then the second agent rebases and creates theirs.
3. Migration timestamps should use the format `YYYYMMDDHHMMSS` (e.g., `20260301150000`).

## Avoiding Merge Conflicts

- Prefer **adding** code over **modifying** existing lines. Additions at the end of a file rarely conflict.
- When adding new enum variants to `ApiError` or `OrderStatus`, add them at the end of the enum.
- When adding new match arms, add them at the end of the match block.
- When adding new test functions, add them at the end of the test file.
- Avoid reformatting code you did not change — it creates unnecessary diff noise.

## Scratchpad Protocol

The `.claude/scratchpad/` directory is gitignored and used for inter-agent communication:

1. Before starting work, write your intent to `.claude/scratchpad/STATUS.md`:
   ```
   ## Agent: <worktree-name>
   **Working on:** <brief description>
   **Files:** <list of files you plan to modify>
   **Started:** <timestamp>
   ```

2. When you hit a blocker, append to `.claude/scratchpad/TODO.md`:
   ```
   - [ ] <description of blocked task> (blocked by: <reason>)
   ```

3. When you finish, update your STATUS entry to mark it complete.
