# Feature Specification: Native tf.json Parsing

**Feature Branch**: `001-tf-json-parsing`
**Created**: 2026-02-17
**Status**: Draft
**Input**: User description: "Oxid today parses .tf (HCL) files only. This spec adds native parsing of *.tf.json (Terraform JSON configuration format) to the core engine. This is a small, high-value change that benefits any workflow producing Terraform JSON — CDK, Pulumi export, hand-written JSON, code generators, etc."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Parse tf.json Files from CDK/Pulumi Output (Priority: P1)

A developer uses CDK for Terraform (CDKTF) or Pulumi to generate infrastructure definitions. These tools output `*.tf.json` files. The developer points Oxid at the directory containing these JSON files and runs `oxid plan` or `oxid apply`. Oxid discovers and parses the `.tf.json` files, producing the same plan/apply results as if the equivalent HCL `.tf` files had been provided.

**Why this priority**: This is the core value proposition. Without parsing `.tf.json` files, Oxid cannot be used with any JSON-based Terraform workflow. This single capability unlocks the entire CDK/Pulumi/code-generator ecosystem.

**Independent Test**: Can be fully tested by placing a `.tf.json` file in a directory and running `oxid plan` — delivers immediate value by showing a correct execution plan.

**Acceptance Scenarios**:

1. **Given** a directory containing only `*.tf.json` files with valid Terraform JSON configuration, **When** the user runs `oxid plan`, **Then** Oxid discovers and parses the JSON files and produces a correct execution plan.
2. **Given** a `main.tf.json` file defining a resource with provider, **When** the user runs `oxid apply`, **Then** the resource is created just as it would be from an equivalent `.tf` file.
3. **Given** a `*.tf.json` file containing provider, resource, data, variable, output, locals, and module blocks, **When** parsed, **Then** all block types are correctly represented in the workspace configuration.

---

### User Story 2 - Mixed HCL and JSON Configuration (Priority: P2)

A team maintains some hand-written `.tf` files alongside generated `.tf.json` files in the same directory. This is a common Terraform pattern — for example, hand-written provider configuration in `providers.tf` with CDKTF-generated resources in `cdk.tf.json`. Oxid discovers and merges both file types, producing a unified plan.

**Why this priority**: Mixed directories are the standard Terraform workflow when using code generators. Blocking mixed usage would severely limit adoption.

**Independent Test**: Can be tested by placing both a `.tf` file and a `.tf.json` file in the same directory and verifying that `oxid plan` includes resources from both files.

**Acceptance Scenarios**:

1. **Given** a directory containing both `main.tf` (HCL) and `generated.tf.json` (JSON) files, **When** the user runs `oxid plan`, **Then** resources from both files appear in the plan.
2. **Given** a provider defined in `providers.tf` and resources referencing that provider in `resources.tf.json`, **When** the user runs `oxid plan`, **Then** provider resolution works correctly across file formats.
3. **Given** a variable defined in a `.tf` file and referenced in a `.tf.json` file (or vice versa), **When** parsed, **Then** cross-file-format references resolve correctly.

---

### User Story 3 - Clear Error Messages for Invalid JSON (Priority: P3)

A developer has a malformed `.tf.json` file (syntax error, invalid structure, wrong block types). When Oxid encounters this file, it reports the error with the filename, location, and a clear description of what went wrong — rather than silently skipping the file or producing a cryptic error.

**Why this priority**: Good error reporting is essential for developer experience, but the feature delivers value even without perfect error messages.

**Independent Test**: Can be tested by providing intentionally malformed `.tf.json` files and verifying that error messages include the filename and a description of the problem.

**Acceptance Scenarios**:

1. **Given** a `.tf.json` file with invalid JSON syntax (e.g., trailing comma), **When** the user runs `oxid plan`, **Then** the error message names the file and describes the JSON syntax error.
2. **Given** a `.tf.json` file with valid JSON but invalid Terraform structure (e.g., unknown block type), **When** parsed, **Then** the error message names the file and describes the structural problem.
3. **Given** a directory with one valid `.tf.json` and one invalid `.tf.json`, **When** the user runs `oxid plan`, **Then** parsing fails with a clear error pointing to the invalid file (not the valid one).

---

### Edge Cases

- What happens when a `.tf.json` file is empty (`{}`)? Expected: treated as valid but contributing no resources (same as an empty `.tf` file).
- What happens when a file is named `.tf.json` without a prefix (just the extension)? Expected: ignored, as Terraform also ignores such files.
- What happens when both `main.tf` and `main.tf.json` exist? Expected: both are parsed and merged, matching Terraform behavior (Terraform loads both).
- What happens when a `.tf.json` file contains JSON5 or JSONC (comments, trailing commas)? Expected: rejected with a clear error, as Terraform JSON requires strict JSON.
- What happens with `.tfvars.json` files? Expected: out of scope for this feature (addressed separately); only `*.tf.json` files are targeted.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST discover `*.tf.json` files in the configuration directory using the same non-recursive directory scan used for `.tf` files.
- **FR-002**: System MUST parse `*.tf.json` files according to the Terraform JSON Configuration Syntax specification, supporting all standard block types: `terraform`, `provider`, `resource`, `data`, `variable`, `output`, `module`, and `locals`.
- **FR-003**: System MUST merge resources, providers, variables, outputs, locals, modules, and data sources from `.tf.json` files into the same `WorkspaceConfig` used by `.tf` files.
- **FR-004**: System MUST support mixed directories containing both `.tf` and `.tf.json` files, merging all discovered files into a single workspace configuration.
- **FR-005**: System MUST process `.tf.json` files in alphabetical order, consistent with existing `.tf` file ordering.
- **FR-006**: System MUST detect the presence of `.tf.json` files during configuration mode detection, treating them equivalently to `.tf` files for determining HCL mode.
- **FR-007**: System MUST report errors from `.tf.json` parsing with the source filename included in the error message.
- **FR-008**: System MUST accept only strict JSON (no comments, no trailing commas) in `.tf.json` files, consistent with Terraform behavior.
- **FR-009**: System MUST handle the Terraform JSON expression syntax where JSON string values are parsed as HCL string templates — only `${...}` interpolation creates references (e.g., `"${var.name}"` is a variable reference, but `"var.name"` is the literal string "var.name"). A string containing only a single `${...}` interpolation MUST return the raw typed value (e.g., `"${var.count}"` returns a number, not a string).
- **FR-010**: System MUST silently skip JSON keys named `"//"` at any nesting level, treating them as comments per the Terraform JSON specification. CDKTF emits these as metadata.
- **FR-011**: System MUST respect block-specific literal rules: `variable.type`, `module.source`, `module.version`, and all values in the `terraform` block are literal strings (NOT parsed as expression templates). `depends_on` values are resource address strings (no `${}` wrapping).
- **FR-012**: System MUST accept both JSON objects and single-element JSON arrays for block values (e.g., a provider can be `{"aws": {...}}` or `{"aws": [{...}]}`), as both forms are valid per the specification and CDKTF always uses arrays.

### Key Entities

- **tf.json File**: A JSON file following the Terraform JSON Configuration Syntax. Has a `.tf.json` extension. Contains the same logical block types as a `.tf` file but encoded in JSON with specific structural conventions.
- **WorkspaceConfig**: The existing unified intermediate representation. After this feature, it is populated from `.tf`, `.tf.json`, and `.yaml` files.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can run `oxid plan` and `oxid apply` on a directory containing only `*.tf.json` files and receive correct results identical to equivalent `.tf` configurations.
- **SC-002**: Users can mix `.tf` and `.tf.json` files in the same directory with no additional configuration or flags required.
- **SC-003**: All block types supported in `.tf` parsing (resource, data, provider, variable, output, module, locals, terraform) are equally supported in `.tf.json` parsing.
- **SC-004**: Error messages from malformed `.tf.json` files include the filename and a human-readable description of the problem.

### Assumptions

- The Terraform JSON Configuration Syntax is well-documented and stable. We follow the specification as defined by HashiCorp.
- `.tfvars.json` parsing is out of scope for this feature and will be addressed separately if needed.
- Override files (`_override.tf.json`, `override.tf.json`) are out of scope for this feature. Oxid does not currently support `.tf` override files either.
- Dynamic blocks (`dynamic "name" {...}`) in JSON are out of scope. CDKTF does not generate them, and they can be added in a follow-up feature.
- The `//` suffix expression convention (e.g., `"count//": "length(var.list)"`) is out of scope. CDKTF uses `${}` template interpolation exclusively.
- JSON Schema validation of `.tf.json` structure is not required; structural errors are caught during block-level parsing.

### Dependencies & External References

This feature references the Terraform JSON Configuration Syntax specification. Consider using `sl deps add` to add the external specification as a reference dependency.

Detailed research on the Terraform JSON spec, Rust ecosystem survey, and CDKTF fixture analysis is in [`tf-json-research.md`](tf-json-research.md).

### Previous work

No related beads issues identified. This is a new capability area for Oxid.

## Clarifications

### Session 2026-02-17

- Q: How should expression handling work in JSON strings? → A: Template interpolation (`${}`) only. Bare `"var.name"` is a literal string, not a reference. Single-interpolation strings return the raw typed value. Corrected FR-009 and added FR-011 for block-specific literal rules.
- Q: Should `"//"` comment keys be handled? → A: Skip silently at all nesting levels. Added FR-010.
- Q: Should dynamic blocks be in scope? → A: Out of scope. CDKTF does not generate them. Added to Assumptions.
- Q: Should the `//` suffix expression convention be in scope? → A: Out of scope. CDKTF uses `${}` exclusively. Added to Assumptions.
- Q: How should provider arrays vs objects be handled? → A: Both forms valid. CDKTF always uses arrays. Added FR-012.
