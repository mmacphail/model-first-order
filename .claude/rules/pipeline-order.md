# Model-First Pipeline Order

This rule enforces the model-first pipeline. Code flows in one direction and **must be written in this order**. Skipping steps or going backwards will cause compilation failures or stale artifacts.

## The Pipeline

```
SQL Migration -> Rust Model -> Handler + utoipa -> Route Registration -> just gen
     (1)            (2)              (3)                 (4)              (5)
```

## Step-by-Step Requirements

### Step 1: SQL Migration

Write `up.sql` and `down.sql` in `migrations/<timestamp>_<name>/`. Then run:
```bash
diesel migration run
```
This regenerates `src/schema.rs`. **Never hand-edit `src/schema.rs`.**

Verify the migration is reversible:
```bash
diesel migration revert
diesel migration run
```

### Step 2: Rust Model

Create or update structs in `src/models/`. Every model must derive:
- `Queryable`, `Selectable` for reading from DB
- `Insertable` for writing to DB
- `Serialize` for JSON responses
- `ToSchema` for OpenAPI generation

If creating a new model file, register it in `src/models/mod.rs`.

### Step 3: Handler + utoipa

Create or update handlers in `src/handlers/`. Every endpoint must:
- Have a `#[utoipa::path]` annotation with accurate request/response types
- Use `conn.transaction()` for mutating operations
- Call `insert_outbox_event()` inside the transaction for mutations
- Return appropriate `ApiError` variants for error cases

If creating a new handler file, register it in `src/handlers/mod.rs`.

### Step 4: Route Registration

Add the new route(s) to `src/routes.rs` inside the `configure()` function.

### Step 5: Regenerate OpenAPI

Run `just gen` to regenerate `openapi.json` from utoipa annotations. This must be the last code-generation step before committing.

## What Happens If You Skip Steps

| Skipped Step | Consequence |
|---|---|
| Migration | `src/schema.rs` won't have the table/columns — model won't compile |
| Model | Handler can't reference types — handler won't compile |
| Handler | No utoipa annotations — `just gen` produces stale OpenAPI |
| Route registration | Endpoint exists in code but is unreachable at runtime |
| `just gen` | `openapi.json` is stale — `just pre-commit` will fail |

## For Agents: Respect the Boundary

When decomposing work across multiple agents:
- **One agent** should handle Steps 1-2 (migration + model skeleton) sequentially.
- Steps 3-4 can be parallelized if agents work on different handler files.
- Step 5 (`just gen`) should be run by whichever agent finishes last, or by the agent doing the final merge.
- Always run `just quality` at the end to verify the full pipeline is intact.
