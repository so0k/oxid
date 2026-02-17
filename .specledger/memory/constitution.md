<!--
Sync Impact Report
- Version change: 0.0.0 → 1.0.0 (initial ratification)
- Added principles:
  - I. Shortest Path to MVP
  - II. Short-Lived Branches
  - III. YAGNI / Simplicity
  - IV. Correctness Over Speed
  - V. Terraform & OpenTofu Compatibility
  - VI. Validate Before Merge
  - VII. Separation of Concerns
  - VIII. DRY / Single Responsibility
- Added sections:
  - Scope & Delivery
  - Quality & Compliance
  - Governance
- Templates requiring updates:
  - .specledger/templates/plan-template.md ✅ aligned (Constitution Check
    references constitution.md generically)
  - .specledger/templates/spec-template.md ✅ aligned (no constitution refs)
  - .specledger/templates/tasks-template.md ✅ aligned (no constitution refs)
- Follow-up TODOs: none
-->

# Oxid Constitution

## Core Principles

### I. Shortest Path to MVP

Every feature MUST be scoped to the minimum viable slice that
delivers user value.

- Identify the smallest increment that is independently testable,
  deployable, and demonstrable to users.
- Spec user stories with explicit priorities (P1, P2, P3). P1 alone
  MUST constitute a viable MVP.
- Cut scope aggressively during planning. Features that can ship
  later MUST ship later.
- If a feature requires more than one sprint/cycle to land, it is
  too large — decompose further.

### II. Short-Lived Branches

Branches MUST merge fast. Long-running feature branches create
integration risk and stale context.

- Break work into phases that can land independently with passing
  tests.
- Each PR SHOULD represent a single logical change — small enough
  to review in one sitting.
- Prefer incremental delivery (feature flags, behind-CLI-flag,
  internal-only) over holding back a large branch.
- If a branch lives longer than a few days, re-evaluate whether it
  can be split.

### III. YAGNI / Simplicity

Do not build what is not needed yet.

- No speculative abstractions. Three similar lines of code are
  better than a premature generic.
- No feature flags, config options, or extension points until a
  second use case exists.
- Complexity MUST be justified in the plan's Complexity Tracking
  table. If the justification is weak, simplify.
- Prefer deleting code over maintaining unused code. Dead code is
  a liability, not an asset.

### IV. Correctness Over Speed

Oxid manages real cloud infrastructure. A wrong apply or corrupt
state can cost hours or money.

- State mutations (apply, destroy, import) MUST be correct. When
  in doubt, fail explicitly rather than proceed optimistically.
- Partial failures MUST be surfaced clearly — never silently
  swallowed or hidden behind a success message.
- Architectural decisions MUST prioritize data integrity over
  performance. Optimize only after correctness is proven.
- Edge cases in infrastructure (eventual consistency, API rate
  limits, transient errors) MUST be designed for, not ignored.

### V. Terraform & OpenTofu Compatibility

Oxid is a drop-in replacement. Users MUST be able to point Oxid at
existing `.tf` configurations and get correct results without file
modifications.

- HCL parsing MUST accept standard Terraform 1.x / OpenTofu syntax
  for all features Oxid implements.
- Provider communication MUST implement the tfplugin5 and tfplugin6
  gRPC protocols faithfully.
- Plan output MUST use the familiar `+`/`~`/`-`/`-/+` symbols so
  users can read diffs without retraining.
- `.tfvars`, `TF_VAR_` environment variables, and `.tfstate` import
  MUST work as documented.
- Compatibility extends to both Terraform and OpenTofu ecosystems.
  Do not introduce behaviors that work with one but break the other.

### VI. Validate Before Merge

Intended behavior MUST be validated through automated checks, not
manual verification.

- All changes MUST pass lint, format, and test gates before merging.
  The specific tooling (clippy, rustfmt, cargo test) is defined in
  the project's CI pipeline.
- New features MUST include tests that validate the intended behavior.
  Tests are the specification — if it is not tested, it is not
  guaranteed.
- Regressions in existing tests block the merge. Fixing a failing
  test by deleting it requires explicit justification.
- CI is the source of truth. "Works on my machine" is not a valid
  merge rationale.

### VII. Separation of Concerns

Clear architectural boundaries prevent tangled code and enable
independent evolution of components.

- Parsing, execution, state management, and user output are distinct
  layers. They communicate through defined interfaces, not shared
  mutable state.
- Backend-agnostic abstractions (e.g., `StateBackend` trait) MUST
  be used when the system supports multiple implementations.
- User-facing output and diagnostic logging are separate channels.
  Progress goes to stdout; diagnostics go through structured logging.
- New modules MUST have a single clear responsibility. If a module
  description requires "and", consider splitting it.

### VIII. DRY / Single Responsibility

Duplication is tolerated until the pattern is proven. Extraction
happens at the right time, not prematurely.

- Extract shared logic only when it appears in 3+ places with the
  same semantics. Two occurrences is not yet a pattern.
- Each module, struct, and function SHOULD have one reason to change.
  God-objects and catch-all utility modules are not permitted.
- Dependencies between modules MUST flow in one direction. Circular
  dependencies indicate a design problem.
- When DRY conflicts with Simplicity (Principle III), Simplicity
  wins. A small amount of duplication is preferable to a wrong
  abstraction.

## Scope & Delivery

These principles shape how features move from idea to merged code:

- **Spec phase**: Scope to MVP (Principle I). Prioritize user stories.
  Mark anything beyond P1 as future work.
- **Plan phase**: Identify the shortest path through the dependency
  graph. Break into independently-landable phases (Principle II).
- **Design phase**: Justify any complexity (Principle III). Verify
  compatibility promises (Principle V). Define clear module
  boundaries (Principle VII).
- **Implementation phase**: Validate continuously (Principle VI).
  Prefer correctness over cleverness (Principle IV).

## Quality & Compliance

- All PRs MUST pass the project's automated quality gates (lint,
  format, test) as defined in CI.
- New features MUST include tests that exercise the intended
  behavior and relevant edge cases.
- Compatibility-affecting changes MUST be tested against both
  Terraform and OpenTofu provider ecosystems.
- Architectural deviations from these principles MUST be documented
  in the PR description with rationale.

## Governance

This constitution guides architectural and design decisions for the
Oxid project. It is consulted during spec, plan, and review phases.

- **Amendments** require: (1) documented rationale, (2) maintainer
  approval, (3) version bump per semantic versioning rules below.
- **Version policy**:
  - MAJOR: principle removed or redefined incompatibly
  - MINOR: new principle added or existing principle materially
    expanded
  - PATCH: clarification, wording fix, non-semantic refinement
- **Scope**: This constitution covers architectural and design
  principles. Language-specific coding standards (linting rules,
  error handling patterns, async idioms) belong in a code review
  checklist, not here.

**Version**: 1.0.0 | **Ratified**: 2026-02-17 | **Last Amended**: 2026-02-17
