# Worktree and Branch Conventions

## When to Use a Worktree

Use a git worktree for any task that involves modifying source files. This applies to:
- Feature development
- Bug fixes
- Refactoring
- Test additions
- Documentation updates that touch code examples

Read-only tasks (analysis, code review, answering questions) do not require a worktree.

## Branch Naming

Branches created in worktrees must follow this pattern:

```
<type>/<short-kebab-description>
```

Where `<type>` is one of the Conventional Commits types:

| Type | Use for |
|---|---|
| `feat` | New features or capabilities |
| `fix` | Bug fixes |
| `refactor` | Code restructuring without behavior change |
| `test` | Adding or improving tests |
| `docs` | Documentation changes |
| `perf` | Performance improvements |
| `chore` | Maintenance tasks (deps, tooling) |
| `ci` | CI/CD changes |

Examples:
- `feat/add-product-aggregate`
- `fix/outbox-null-payload`
- `refactor/extract-pagination-params`
- `test/full-cancellation-coverage`

## Worktree Naming

When Claude Code creates a worktree, prefer a descriptive name over the auto-generated hash:

```
Good:  .claude/worktrees/add-product-crud/
Good:  .claude/worktrees/fix-outbox-race/
Bad:   .claude/worktrees/agent-a458e538/
```

If creating a worktree manually, use the branch description as the worktree name.

## Worktree Lifecycle

1. **Create:** Start a worktree from latest `main` (or from a feature branch if building on another agent's work).
2. **Work:** Make changes, commit frequently with Conventional Commit messages.
3. **Validate:** Run `just quality` before considering work complete.
4. **PR:** Push the branch and create a pull request.
5. **Clean up:** After the PR is merged, remove the worktree.

## Environment Setup in Worktrees

Each worktree needs its own `.env` file (it's gitignored). Copy from `.env.example`:

```bash
cp .env.example .env
```

The default `DATABASE_URL` in `.env.example` points to `localhost:5432`. This is fine for integration tests (which use testcontainers on random ports) but matters for `just dev` or `just smoke`.

If multiple agents need to run `just dev` simultaneously, each should use a different `PORT` value in their `.env`.

## Sharing Work Between Worktrees

If Agent B needs to build on Agent A's uncommitted work:
1. Agent A should commit and push their branch first.
2. Agent B creates their worktree from Agent A's branch: the base branch should be Agent A's branch, not `main`.
3. Agent B's commits will stack on top of Agent A's.
4. When creating the PR, set the base to whatever branch Agent A's PR targets.
