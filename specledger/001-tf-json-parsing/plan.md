# Implementation Plan: Native tf.json Parsing

**Branch**: `001-tf-json-parsing` | **Date**: 2026-02-17 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specledger/001-tf-json-parsing/spec.md`

## Summary

Add native parsing of `*.tf.json` files (Terraform JSON Configuration Syntax) to Oxid's core config pipeline. The approach converts JSON → `hcl::Body` → reuses the existing `parse_hcl` block parser, avoiding duplication of 793 lines of block-parsing logic. File discovery and mode detection are extended to recognize `.tf.json` alongside `.tf`. No new crate dependencies needed — `serde_json` is already present.

## Technical Context

**Language/Version**: Rust 1.93+ (pinned via mise.toml)
**Primary Dependencies**: hcl-rs 0.18, serde_json (already in Cargo.toml)
**Storage**: N/A (no state changes)
**Testing**: cargo test — integration tests in `tests/` using 8 CDKTF fixtures at `tests/fixtures/tf-json/`
**Target Platform**: Same as Oxid (Linux, macOS, Windows)
**Project Type**: Single Rust binary
**Performance Goals**: Parsing `.tf.json` must be no slower than parsing equivalent `.tf` files
**Constraints**: No new crate dependencies; must pass existing clippy + rustfmt gates
**Scale/Scope**: ~300-400 lines of new code (one new file + minor edits to 2 existing files)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- [x] **Specification-First**: Spec.md complete with 3 prioritized user stories (P1-P3), 12 FRs, 4 SCs
- [x] **Test-First**: 8 CDKTF-generated fixtures exist at `tests/fixtures/tf-json/`; integration test strategy defined below
- [x] **Code Quality**: rustfmt + clippy already in CI pipeline; no new tooling needed
- [x] **UX Consistency**: Acceptance scenarios defined for all 3 user stories; output identical to `.tf` parsing
- [x] **Performance**: Same parsing pipeline as `.tf` files; JSON→Body conversion adds negligible overhead
- [x] **Observability**: Existing `tracing::debug!("Parsing HCL file: ...")` pattern extended to `.tf.json` files
- [x] **Issue Tracking**: Beads epic oxid-db0 created with 5 phases, 16 tasks

**Complexity Violations**: None identified. Single new file, minimal edits to 2 existing files.

## Architecture Decision: JSON → hcl::Body → parse_hcl_body (Approach B2)

### Decision

Convert `.tf.json` content to `hcl::Body` using the hcl-rs builder API, then feed it to the existing block parser. This requires a minor refactor of `parse_hcl()` to extract the body-iteration logic.

### Rationale

- **Reuses all existing block parsing logic** — 793 lines in `parser.rs` handling terraform, provider, resource, data, variable, output, module, and locals blocks
- **Zero duplication** of block type detection, attribute extraction, expression conversion, nested block handling
- **Guaranteed feature parity** — any block type the HCL parser handles, the JSON path handles identically
- **hcl-rs builder API is fully capable**: `Block::builder()`, `Attribute::new()`, `hcl::to_expression(&serde_json_value)` all work

### Alternatives Rejected

| Approach | Why Rejected |
|----------|-------------|
| A: JSON → WorkspaceConfig directly | Duplicates 793 lines of block parsing logic. Higher maintenance burden, divergence risk. |
| B1: JSON → Body → serialize to HCL string → parse_hcl | Wasteful serialization roundtrip (Body → string → Body). Correct but unnecessary overhead. |
| B3: Skip Body, build WorkspaceConfig directly | Same duplication problem as Approach A. |

### How It Works

```
.tf.json file
  → serde_json::from_str()         # Parse strict JSON
  → json_to_body()                  # Apply Terraform JSON spec rules:
      - Top-level keys → block types (resource, provider, etc.)
      - Label peeling (2 labels for resource/data, 1 for provider/variable/output/module)
      - Array vs object block disambiguation
      - "//" comment key stripping
      - Expression creation via hcl::to_expression()
  → hcl::Body                       # Standard hcl-rs AST
  → parse_hcl_body()                # EXISTING parser logic (refactored)
  → WorkspaceConfig                  # Same output as .tf parsing
  → merge_workspace()                # EXISTING merge logic (unchanged)
```

## Project Structure

### Documentation (this feature)

```text
specledger/001-tf-json-parsing/
├── spec.md              # Feature specification
├── plan.md              # This file
├── research.md          # Phase 0 research (tf-json-research.md)
├── data-model.md        # Phase 1: JSON-to-block mapping rules
├── quickstart.md        # Phase 1: Developer guide
└── tasks.md             # Phase 2 output (/specledger.tasks)
```

### Source Code (repository root)

```text
src/hcl/
├── mod.rs              # MODIFIED: add .tf.json discovery to parse_directory()
├── parser.rs           # MODIFIED: extract parse_hcl_body() from parse_hcl()
└── json_parser.rs      # NEW: json_to_body() converter (~250-300 lines)

src/config/
└── loader.rs           # MODIFIED: add .tf.json to has_tf_files() mode detection

tests/
├── tf_json_test.rs     # NEW: integration tests using CDKTF fixtures
└── fixtures/tf-json/   # EXISTING: 8 CDKTF-generated fixture directories
    ├── foreach/
    ├── modules/
    ├── multi-provider/
    ├── iam-grants/
    ├── encryption/
    ├── compute-events/
    ├── storage-autoscaling/
    └── stepfunctions/
```

**Structure Decision**: No new directories or modules. The JSON parser lives in `src/hcl/` alongside the existing HCL parser since they share the same output type (`hcl::Body`) and are consumed by the same pipeline.

## File Change Details

### 1. `src/config/loader.rs` — Mode detection (minor edit)

**What changes**: `has_tf_files()` must also match files ending in `.tf.json`.

**Current logic** (line 71):
```rust
.any(|e| e.path().extension().map(|ext| ext == "tf").unwrap_or(false))
```

**New logic**: Check for `.tf` extension OR filename ending with `.tf.json`. Note: `.tf.json` has extension `"json"`, so simple extension matching fails — need to check the full filename string.

**No changes to**: `detect_mode()`, `load_workspace()`, `has_yaml_files()` — these work transitively once `has_tf_files()` is updated.

### 2. `src/hcl/mod.rs` — File discovery + parsing dispatch (moderate edit)

**What changes**:
1. `parse_directory()` file filter (line 18) — also collect `.tf.json` files
2. Remove the "No .tf files found" error when `.tf.json` files are present (line 22-24)
3. For each `.tf.json` file: read content, call `json_parser::parse_tf_json()`, merge result
4. Update tracing messages

**No changes to**: `merge_workspace()`, tfvars handling, alphabetical sort logic.

### 3. `src/hcl/parser.rs` — Refactor to expose body parsing (minor refactor)

**What changes**: Extract the body iteration loop into a public `parse_hcl_body()` function.

**Before**:
```rust
pub fn parse_hcl(content: &str, file_path: &Path) -> Result<WorkspaceConfig> {
    let body: hcl::Body = hcl::from_str(content)?;
    // ... 793 lines of block parsing
}
```

**After**:
```rust
pub fn parse_hcl(content: &str, file_path: &Path) -> Result<WorkspaceConfig> {
    let body: hcl::Body = hcl::from_str(content)
        .with_context(|| format!("Failed to parse HCL in {}", file_path.display()))?;
    parse_hcl_body(body, file_path)
}

pub fn parse_hcl_body(body: hcl::Body, file_path: &Path) -> Result<WorkspaceConfig> {
    // ... exact same 793 lines, unchanged
}
```

**No logic changes** — pure extraction refactor. All existing tests must continue to pass.

### 4. `src/hcl/json_parser.rs` — NEW: JSON → Body conversion (~250-300 lines)

**Public API**:
```rust
pub fn parse_tf_json(content: &str, file_path: &Path) -> Result<WorkspaceConfig>
```

**Internal functions**:

| Function | Purpose | FR |
|----------|---------|-----|
| `parse_tf_json()` | Entry point: JSON string → WorkspaceConfig | FR-002, FR-007, FR-008 |
| `json_to_body()` | Root JSON object → hcl::Body (top-level block dispatch) | FR-002 |
| `convert_block()` | Recursively peel labels from nested JSON objects | FR-002, FR-012 |
| `json_value_to_expression()` | serde_json::Value → hcl::Expression (via `hcl::to_expression`) | FR-009 |
| `strip_comments()` | Remove `"//"` keys at any nesting level | FR-010 |

**Block type schema** (hardcoded, matching Terraform's `configFileSchema`):

| Block Type | Label Count | JSON Nesting Depth |
|------------|-------------|-------------------|
| `resource` | 2 | `resource.TYPE.NAME.{body}` |
| `data` | 2 | `data.TYPE.NAME.{body}` |
| `provider` | 1 | `provider.NAME.{body}` or `provider.NAME.[{body}]` |
| `variable` | 1 | `variable.NAME.{body}` |
| `output` | 1 | `output.NAME.{body}` |
| `module` | 1 | `module.NAME.{body}` |
| `terraform` | 0 | `terraform.{body}` |
| `locals` | 0 | `locals.{key: expr, ...}` |

**Expression handling rules** (FR-009, FR-011):

| Context | JSON String Treatment |
|---------|----------------------|
| Resource/data attributes | String template (`${...}` interpolation) |
| `locals` values | String template |
| `output.value` | String template |
| `variable.type` | Literal string (no template parsing) |
| `variable.default` | String template |
| `module.source`, `module.version` | Literal string |
| `terraform` block values | Literal string |
| `depends_on` array values | Resource address string (no `${...}`) |

Note: Template parsing is handled downstream by `hcl_expr_to_expression()` in `parser.rs` which checks for `${` in string values. JSON strings without `${` are naturally treated as literals. The block-specific literal rules (FR-011) are enforced by storing certain values as `Expression::String` without template markers, matching how the HCL parser already handles them.

**Array/object disambiguation** (FR-012):
- If a block value is a JSON array → each element is a separate block instance
- If a block value is a JSON object → single block instance
- Both forms valid at any nesting level (providers, nested blocks, etc.)

### 5. `tests/tf_json_test.rs` — NEW: Integration tests

**Test strategy**: Parse each CDKTF fixture, verify the resulting `WorkspaceConfig` contains expected resources, providers, variables, outputs, and terraform settings.

| Test | Fixture | What It Validates |
|------|---------|-------------------|
| `test_parse_foreach` | `foreach/` | `for_each` expressions, `each.value`/`each.key`, variables with complex types |
| `test_parse_multi_provider` | `multi-provider/` | 5 providers, locals, outputs with interpolation, `depends_on` |
| `test_parse_modules` | `modules/` | Module blocks with `source`/`version` as literals |
| `test_parse_mixed_tf_and_json` | Hand-crafted | `.tf` + `.tf.json` in same directory, cross-format references |
| `test_parse_empty_json` | Hand-crafted | `{}` → valid empty workspace |
| `test_parse_invalid_json` | Hand-crafted | Trailing comma → error with filename |
| `test_parse_invalid_structure` | Hand-crafted | Unknown block type → error with filename |
| `test_comment_keys_ignored` | All fixtures | `"//"` keys don't appear in parsed output |
| `test_provider_array_form` | `foreach/` | Providers wrapped in arrays (CDKTF style) |

AWS-dependent fixtures (`iam-grants`, `encryption`, `compute-events`, `storage-autoscaling`, `stepfunctions`) are used for parsing-only tests (verify WorkspaceConfig structure) — they don't require AWS credentials.

## Complexity Tracking

No violations. The implementation adds one new file and makes minor edits to two existing files. No new abstractions, no new crate dependencies, no new configuration options.
