# Tasks Index: Native tf.json Parsing

Beads Issue Graph Index into the tasks and phases for this feature implementation.
This index does **not contain tasks directly**—those are fully managed through Beads CLI.

## Feature Tracking

* **Beads Epic ID**: `oxid-db0`
* **User Stories Source**: `specledger/001-tf-json-parsing/spec.md`
* **Research Inputs**: `specledger/001-tf-json-parsing/research.md`
* **Planning Details**: `specledger/001-tf-json-parsing/plan.md`
* **Data Model**: `specledger/001-tf-json-parsing/data-model.md`
* **Contract Definitions**: N/A (no API changes)

## Beads Query Hints

```bash
# Find all open tasks for this feature
bd list --label spec:001-tf-json-parsing --status open -n 20

# Find ready tasks to implement
bd ready --label spec:001-tf-json-parsing -n 5

# See full dependency tree
bd dep tree --reverse oxid-db0

# View issues by phase
bd list --label phase:foundational --label spec:001-tf-json-parsing
bd list --label phase:us1 --label spec:001-tf-json-parsing
bd list --label phase:us2 --label spec:001-tf-json-parsing
bd list --label phase:us3 --label spec:001-tf-json-parsing
bd list --label phase:polish --label spec:001-tf-json-parsing

# View test tasks only
bd list --label spec:001-tf-json-parsing --label test:integration
bd list --label spec:001-tf-json-parsing --label test:regression
```

## Tasks and Phases Structure

```
oxid-db0: Native tf.json Parsing (epic)
├── oxid-4j7: Phase 1 - Foundational (feature)
│   ├── oxid-1lw: Refactor parse_hcl → parse_hcl_body (task) [READY]
│   └── oxid-e53: Verify regression tests pass (task) [blocked by 1lw]
├── oxid-5jn: Phase 2 - US1 Core Parsing (feature) [blocked by Phase 1]
│   ├── oxid-6gz: Create json_parser.rs skeleton (task)
│   ├── oxid-ia7: Write credential-free fixture tests (task) [blocked by 6gz]
│   ├── oxid-wbl: Write comment/provider array tests (task) [blocked by 6gz]
│   ├── oxid-7rb: Implement json_to_body converter (task) [blocked by 6gz, 1lw]
│   ├── oxid-6kd: Update file discovery (task) [blocked by 7rb]
│   └── oxid-pwn: Write AWS fixture tests (task) [blocked by 7rb]
├── oxid-amm: Phase 3 - US2 Mixed Mode (feature) [blocked by Phase 2]
│   ├── oxid-drb: Write mixed directory tests (task)
│   └── oxid-qnz: Verify mixed merge behavior (task) [blocked by drb]
├── oxid-lab: Phase 4 - US3 Error Messages (feature) [blocked by Phase 2]
│   ├── oxid-p4r: Write error handling tests (task)
│   └── oxid-2cr: Implement error context (task) [blocked by p4r]
└── oxid-kzu: Phase 5 - Polish (feature) [blocked by Phase 3, Phase 4]
    ├── oxid-8u0: Write edge case tests (task)
    └── oxid-ec8: Run quality gates (task) [blocked by 8u0]
```

## Convention Summary

| Type    | Description                  | Labels                                 |
| ------- | ---------------------------- | -------------------------------------- |
| epic    | Full feature epic            | `spec:001-tf-json-parsing`             |
| feature | Implementation phase / story | `phase:*`, `story:US*`                 |
| task    | Implementation task          | `component:*`, `requirement:FR-*`, `test:*` |

## Test Plan Summary

| Phase | Test Tasks | What's Tested | Test Count |
|-------|-----------|---------------|------------|
| Phase 1 | oxid-e53 | Regression (existing tests pass after refactor) | All existing |
| Phase 2 | oxid-ia7 | Credential-free fixtures: foreach, multi-provider, modules | 3 tests |
| Phase 2 | oxid-wbl | Comment keys, provider array/object forms | 3 tests |
| Phase 2 | oxid-pwn | AWS fixtures: iam-grants, encryption, compute-events, storage-autoscaling, stepfunctions | 5 tests |
| Phase 3 | oxid-drb | Mixed .tf + .tf.json directories, cross-format references | 3 tests |
| Phase 4 | oxid-p4r | Invalid JSON, invalid structure, filename in errors, strict JSON | 5 tests |
| Phase 5 | oxid-8u0 | Edge cases: empty JSON, ordering, tfvars.json ignored, JSON-only dirs | 4 tests |
| Phase 5 | oxid-ec8 | Quality gates: clippy, rustfmt, full suite | Full suite |
| **Total** | | | **~23 new tests + full regression** |

### Test Patterns (from existing codebase)

All tests follow conventions established in `tests/config_test.rs`, `tests/dag_test.rs`:
- **Fixture-based**: Read from `tests/fixtures/tf-json/<name>/cdk.tf.json`
- **Inline-string**: Use `r#"{...}"#` for hand-crafted JSON
- **Tempdir**: Use `tempfile::TempDir` for mixed-directory tests (following `state_test.rs`)
- **Assertions**: `assert_eq!` for counts/values, `assert!` for membership, `result.unwrap_err().to_string().contains()` for errors
- **Self-contained**: Each test is independent, no shared helpers

## Implementation Strategy

### MVP (User Story 1 only)

Phase 1 + Phase 2 deliver a complete, working feature:
- Parse any `*.tf.json` file with all 8 block types
- Discover `.tf.json` files automatically
- Comment keys stripped, provider arrays handled
- Validated against 8 real CDKTF fixtures (11 tests)

### Incremental Delivery

| Increment | What Ships | Tests |
|-----------|-----------|-------|
| **MVP** (Phase 1+2) | Core tf.json parsing + file discovery | 11 fixture tests + regression |
| **+US2** (Phase 3) | Mixed .tf + .tf.json directories | +3 mixed tests |
| **+US3** (Phase 4) | Error messages with filenames | +5 error tests |
| **Polish** (Phase 5) | Edge cases, quality gates | +4 edge case tests + full suite |

### Parallel Execution Opportunities

Within Phase 2 (after skeleton T003):
- oxid-ia7 (fixture tests) ∥ oxid-wbl (comment/array tests) — both just write tests
- oxid-7rb (implementation) can start in parallel with test writing

Phase 3 ∥ Phase 4 — US2 and US3 are independent of each other

---

> This file is intentionally light and index-only. Implementation data lives in Beads. Update this file only to point humans and agents to canonical query paths and feature references.
