# Data Model: tf.json → WorkspaceConfig Mapping

**Feature**: 001-tf-json-parsing | **Date**: 2026-02-17

This feature introduces no new data types. The JSON parser produces the same `WorkspaceConfig` as the existing HCL parser. This document defines the mapping rules from Terraform JSON Configuration Syntax to Oxid's existing types.

## Entity: tf.json File → hcl::Body

A `.tf.json` file is a JSON object whose top-level keys map to HCL block types. Each block type has a defined label count that determines JSON nesting depth.

### Block-to-JSON Mapping

```
JSON Key         Labels   JSON Nesting → HCL Block
─────────────────────────────────────────────────────
"resource"       2        resource.TYPE.NAME.{body}     → Block("resource", [TYPE, NAME], body)
"data"           2        data.TYPE.NAME.{body}         → Block("data", [TYPE, NAME], body)
"provider"       1        provider.NAME.{body|[body]}   → Block("provider", [NAME], body)
"variable"       1        variable.NAME.{body}          → Block("variable", [NAME], body)
"output"         1        output.NAME.{body}            → Block("output", [NAME], body)
"module"         1        module.NAME.{body}            → Block("module", [NAME], body)
"terraform"      0        terraform.{body}              → Block("terraform", [], body)
"locals"         0        locals.{key: expr, ...}       → Block("locals", [], [Attribute(k,v)...])
```

### Label Peeling Algorithm

For a block type with N labels, the converter peels N levels of JSON object nesting:

```
Input: {"resource": {"aws_s3_bucket": {"my_bucket": {"bucket": "my-bucket"}}}}

Step 1: Top-level key "resource" → block type, 2 labels expected
Step 2: First nested key "aws_s3_bucket" → label[0]
Step 3: Second nested key "my_bucket" → label[1]
Step 4: Remaining object {"bucket": "my-bucket"} → block body attributes
Result: Block("resource", ["aws_s3_bucket", "my_bucket"], {bucket = "my-bucket"})
```

At any nesting level, if the value is an array instead of an object, each array element produces a separate block with the same labels up to that point.

### Value Type Mapping

```
JSON Type    → hcl::Expression         → Oxid Expression (after parse_hcl_body)
───────────────────────────────────────────────────────────────────────────────
String       → Expression::String       → Expression::Literal(String) or Expression::Template
Number       → Expression::Number       → Expression::Literal(Int/Float)
Boolean      → Expression::Bool         → Expression::Literal(Bool)
Null         → Expression::Null         → Expression::Literal(Null)
Array        → Expression::Array        → Expression::Literal(List) or block expansion
Object       → Expression::Object       → Expression::Literal(Map) or nested block
```

### Expression Context Rules

| Block / Attribute | Template-Parsed? | Reason |
|---|---|---|
| Resource attributes | Yes | Standard expression context |
| Data source attributes | Yes | Standard expression context |
| `locals` values | Yes | All locals are expressions |
| `output.value` | Yes | Output values are expressions |
| `output.description` | No | Literal metadata |
| `output.sensitive` | No | Boolean literal |
| `variable.default` | Yes | Default can reference other vars |
| `variable.type` | No | Type constraint literal (e.g., `"list(string)"`) |
| `variable.description` | No | Literal metadata |
| `variable.sensitive` | No | Boolean literal |
| `module.source` | No | Registry/path literal |
| `module.version` | No | Version constraint literal |
| `module.*` (other) | Yes | Module input expressions |
| `terraform.*` | No | All terraform block values are literals |
| `provider.*` | Yes | Provider config can use expressions |
| `depends_on` values | No | Resource address strings (no `${}`) |
| `lifecycle` attributes | Varies | `prevent_destroy` is literal; `ignore_changes` may reference |

### Comment Key Handling

JSON keys named `"//"` are stripped at all nesting levels before block conversion. CDKTF emits these as metadata objects:

```json
"//": {
  "metadata": {
    "path": "foreach/triggered",
    "uniqueId": "triggered"
  }
}
```

These are silently discarded. They carry no semantic meaning for infrastructure configuration.

## Existing Types (unchanged)

No modifications to `src/config/types.rs`. The following types are reused as-is:

- `WorkspaceConfig` — unified IR populated by both `.tf` and `.tf.json` parsing
- `ResourceConfig` — individual resource definition
- `DataSourceConfig` — data source definition
- `ProviderConfig` — provider configuration
- `VariableConfig` — variable definition
- `OutputConfig` — output definition
- `ModuleConfig` — module reference
- `TerraformSettings` — terraform block settings
- `Expression` — expression AST (references, templates, literals, function calls)
- `Value` — literal value types (string, int, float, bool, list, map)
