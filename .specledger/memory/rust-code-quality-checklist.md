# Rust Code Quality Checklist: Oxid

**Purpose**: Language-specific coding standards for Rust code review.
Companion to the architectural constitution at `constitution.md`.
**Created**: 2026-02-17

## Linting & Formatting

- [ ] CHK001 `cargo fmt --all -- --check` passes
- [ ] CHK002 `cargo clippy --all-targets -- -D warnings` passes
- [ ] CHK003 Any `#[allow(clippy::...)]` has a justification comment
- [ ] CHK004 `#[allow(clippy::all)]` only on generated code (prost)
- [ ] CHK005 No new `#[allow(dead_code)]` on public items

## Error Handling

- [ ] CHK006 Public functions return `anyhow::Result<T>`, not raw
      `Option` or panics
- [ ] CHK007 No `unwrap()` on fallible I/O, network, or user input
- [ ] CHK008 Permitted `unwrap()` (mutex poisoning, compile-time
      constants, structurally infallible) has a justification comment
- [ ] CHK009 Errors wrapped with `.context()` / `.with_context()`
      describing what operation failed
- [ ] CHK010 `bail!()` used for validation guards with descriptive
      messages (not `return Err(anyhow!(...))`)
- [ ] CHK011 `unwrap_or_default()` only where missing field has a
      well-defined default (e.g., SQLite row mapping)
- [ ] CHK012 `thiserror` used when callers need to match on error
      variants; `anyhow` at CLI boundaries

## Async & Concurrency

- [ ] CHK013 No blocking calls on async threads without
      `spawn_blocking`
- [ ] CHK014 All gRPC RPCs wrapped in `tokio::time::timeout` with
      operation-appropriate duration
- [ ] CHK015 `tokio::sync::RwLock` for read-heavy shared state;
      `tokio::sync::Mutex` for exclusive access
- [ ] CHK016 `DashMap` preferred over `Mutex<HashMap>` for concurrent
      read access
- [ ] CHK017 `async-trait` used for trait definitions with async
      methods
- [ ] CHK018 `BoxFuture` / `Box::pin` for async closures shared via
      `Arc`

## Safety

- [ ] CHK019 No new `unsafe` blocks without documented safety
      invariants and `#[cfg(...)]` gating where applicable
- [ ] CHK020 All child processes use `kill_on_drop(true)`
- [ ] CHK021 All SQL queries use parameterized binding (`params![]`,
      `?1`/`?2`); no string interpolation into SQL
- [ ] CHK022 Input validated at system boundaries before processing

## Testing

- [ ] CHK023 New features include tests validating intended behavior
- [ ] CHK024 Filesystem tests use `tempfile::TempDir`, not hardcoded
      paths
- [ ] CHK025 SQLite tests use `open_memory()` or temp directory
- [ ] CHK026 Test helpers (e.g., `create_test_store()`) used for
      repeated setup
- [ ] CHK027 Test naming follows `test_<what>_<expected_behavior>`
- [ ] CHK028 Integration tests in `tests/`; unit tests in inline
      `#[cfg(test)]` modules

## Output & Observability

- [ ] CHK029 User-facing output uses `println!` + `colored` crate,
      not `tracing` macros
- [ ] CHK030 Diagnostic logging uses `tracing` with structured fields
      (`key = %value`)
- [ ] CHK031 Output symbols follow convention: `+` green (create),
      `~` yellow (update), `-` red (destroy), `✓` green (success)
- [ ] CHK032 `--verbose` flag controls tracing filter (`warn` →
      `debug`)
- [ ] CHK033 Provider stderr routed through `tracing` with
      `target: "provider_stderr"`

## Dependencies

- [ ] CHK034 New dependency justified (compile-time, maintenance,
      license compatibility with Apache-2.0)
- [ ] CHK035 Optional backends behind Cargo feature flags (not
      compiled by default)
- [ ] CHK036 `rusqlite` uses `features = ["bundled"]`
- [ ] CHK037 Cargo.toml dependency has category comment

## Style Conventions

- [ ] CHK038 Module files: `snake_case.rs`
- [ ] CHK039 Types: `PascalCase`; functions: `snake_case` verb-noun;
      constants: `SCREAMING_SNAKE_CASE`
- [ ] CHK040 `pub` only where cross-module access is needed
- [ ] CHK041 `#[derive(Default)]` preferred over manual
      `impl Default` for config/filter types
- [ ] CHK042 ASCII art section separators for logical sections in
      large files (`// ─── Section Name ───`)