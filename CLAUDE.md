# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Oxid is a standalone infrastructure-as-code engine (Apache-2.0) — an alternative to Terraform/OpenTofu. It parses HCL (.tf) files natively and communicates directly with Terraform providers via gRPC (tfplugin5/6). No Terraform binary required.

**Key differentiators from Terraform:** event-driven per-resource parallelism (not wave-based), SQLite local state with SQL query support, optional PostgreSQL for teams.

## Development Setup

**Tool management:** This project uses [mise](https://mise.jdx.dev) to pin tool versions. Running `mise install` in the repo root installs everything defined in `mise.toml`.

**Prerequisites (all managed via mise.toml):**
- **Rust 1.93+** — mise uses rustup under the hood. If rustup is not installed, install it first: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **protoc** (protobuf compiler) — required by `build.rs` for gRPC codegen from `proto/tfplugin5.proto` and `proto/tfplugin6.proto`

```bash
mise install                   # Install all tools (rust, protoc, bd, etc.)
```

**Shell setup** — ensure both are in `~/.zshrc`:
```bash
eval "$(mise activate zsh)"
source "$HOME/.cargo/env"
```

## Build & Quality Gates

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run all integration tests
cargo test <name>              # Run a single test by name
cargo fmt --all -- --check     # Check formatting (must pass before merge)
cargo clippy --all-targets -- -D warnings   # Lint (must pass before merge)
```

Feature flags: `sqlite` (default), `postgres` (optional, enables sqlx).

## Standards & Checklists

- **Architecture principles:** `.specledger/memory/constitution.md` — 8 design principles (MVP-first, YAGNI, correctness, compatibility, etc.) consulted during spec/plan phases
- **Code review checklist:** `.specledger/memory/rust-code-quality-checklist.md` — 42-item Rust-specific checklist (clippy, error handling, async patterns, safety, testing, output conventions)

## Architecture

```
.tf / .yaml → [Config Parser] → WorkspaceConfig → [DAG Builder] → ResourceGraph
    → [Provider Manager / gRPC] → [Planner] → [DAG Walker] → [StateBackend]
```

### Core Pipeline

1. **Config** (`src/config/`) — Loads and merges .tf (via hcl-rs) or .yaml files into `WorkspaceConfig`, the unified intermediate representation. Applies .tfvars and TF_VAR_* env vars with Terraform-compatible precedence.

2. **DAG** (`src/dag/`) — Builds a `petgraph::DiGraph<DagNode, DependencyEdge>` from WorkspaceConfig. Expands `count` and `for_each` into individual nodes. Resolves explicit `depends_on` plus implicit expression references. Validates acyclicity.

3. **Provider** (`src/provider/`) — Downloads providers from registry.terraform.io, caches binaries in `.oxid/providers/`, manages gRPC connections (tfplugin5/6 protocols compiled from `proto/` via `build.rs`). Connection pooling uses `Arc<RwLock<HashMap>>`.

4. **Executor** (`src/executor/engine.rs`, ~1,775 LOC) — Orchestrates plan/apply. `EvalContext` resolves expressions including cross-resource references (`aws_vpc.main.id`), `count.index`, `each.key`. This is the largest and most complex file.

5. **DAG Walker** (`src/dag/walker.rs`) — Event-driven async executor. Uses `tokio::sync::Semaphore` for parallelism control (default 10, configurable via `--parallelism`). Spawns async tasks per node; dependents start immediately when dependencies complete.

6. **State** (`src/state/`) — `StateBackend` trait with `SqliteBackend` implementation. Stores resources, dependencies, workspaces, outputs, and run history. Supports SQL queries via `oxid query`.

7. **Output** (`src/output/`) — Terraform-compatible colored terminal output with `+`, `~`, `-`, `-/+` symbols.

### CLI Entry Point

`src/main.rs` (~1,091 LOC) — Clap-based CLI with commands: init, plan, apply, destroy, state, import, query, workspace, graph, providers, drift, validate.

### Key Types

- `WorkspaceConfig` (`src/config/types.rs`) — Unified IR for all infrastructure config
- `DagNode` / `ResourceGraph` (`src/dag/resource_graph.rs`) — Per-resource graph nodes with typed edges
- `ResourceAction` (`src/planner/plan.rs`) — Create/Update/Delete/Replace/Read/NoOp
- `StateBackend` trait (`src/state/backend.rs`) — Async pluggable storage interface
- `ProviderManager` (`src/provider/manager.rs`) — Provider lifecycle and gRPC connection pooling

## Testing

All tests are integration tests in `tests/` (no unit tests in `src/`):
- `tests/config_test.rs` — Config parsing and validation
- `tests/dag_test.rs` — DAG building and dependency resolution
- `tests/executor_test.rs` — Plan/apply execution
- `tests/state_test.rs` — SQLite backend (uses `SqliteBackend::open_memory()`)
- `tests/integration_test.rs` — Full pipeline

## Concurrency Patterns

- `Arc<Semaphore>` for DAG walker parallelism
- `DashMap` for concurrent resource state access during planning
- `Arc<RwLock<>>` for provider connection pooling
- All state backend operations are async (`#[async_trait]`)
- Error handling: `anyhow::Result` with `.context()` chains, `thiserror` for custom types

## Issue Tracking

This project uses **bd (beads)** for issue tracking — not markdown TODOs. See AGENTS.md for workflow. Key commands: `bd ready`, `bd create`, `bd update`, `bd close`, `bd sync`.

## Commit Style

Conventional prefixes: `feat:`, `fix:`, `chore:`, `docs:`. Imperative mood, under 72 chars. Mention proto changes, migrations, or new binaries in the body.
