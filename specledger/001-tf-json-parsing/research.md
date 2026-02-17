# Research: Terraform JSON Configuration Syntax & Rust Tooling

## Context

Oxid needs to parse `*.tf.json` files (Terraform JSON configuration syntax) alongside native `.tf` (HCL) files. This research covers the JSON spec, how Terraform/OpenTofu parse it, and what Rust crates are available.

---

## 1. Terraform JSON Configuration Syntax Specification

**Official Sources:**
- [HashiCorp docs](https://developer.hashicorp.com/terraform/language/syntax/json)
- [OpenTofu docs](https://opentofu.org/docs/language/syntax/json/)
- [HCL JSON spec (GitHub)](https://github.com/hashicorp/hcl/blob/main/json/spec.md)

### File Convention
- Files use `.tf.json` suffix (OpenTofu also supports `.tofu.json`)
- Strict JSON only — no comments, no trailing commas
- `"//"` property keys are treated as comments and ignored

### Root Structure
A `.tf.json` file is a JSON object whose top-level keys are block types:
```json
{
  "terraform": { ... },
  "provider": { ... },
  "variable": { ... },
  "locals": { ... },
  "resource": { ... },
  "data": { ... },
  "module": { ... },
  "output": { ... }
}
```

### Block-to-JSON Mapping Rules

Each HCL block label becomes a level of JSON nesting:

| Block Type | HCL Labels | JSON Nesting |
|---|---|---|
| `resource "type" "name"` | 2 labels | `"resource": { "type": { "name": { ... } } }` |
| `data "type" "name"` | 2 labels | `"data": { "type": { "name": { ... } } }` |
| `variable "name"` | 1 label | `"variable": { "name": { ... } }` |
| `output "name"` | 1 label | `"output": { "name": { ... } }` |
| `module "name"` | 1 label | `"module": { "name": { ... } }` |
| `provider "name"` | 1 label | `"provider": { "name": { ... } }` (or array for multiple) |
| `terraform` | 0 labels | `"terraform": { ... }` |
| `locals` | 0 labels | `"locals": { "key": "expr", ... }` |

### Multiple Blocks of Same Type
When multiple blocks share the same type, the innermost level becomes an array:
```json
{
  "provider": {
    "aws": [
      { "region": "us-east-1" },
      { "alias": "west", "region": "us-west-1" }
    ]
  }
}
```

A single block can be either an object or a one-element array (both valid).

### Nested Blocks (e.g., `ingress`, `lifecycle`, `provisioner`)
Nested blocks within resources follow the same pattern — arrays for multiple, objects for single:
```json
{
  "resource": {
    "aws_security_group": {
      "main": {
        "ingress": [
          { "from_port": 80, "to_port": 80, "protocol": "tcp" }
        ]
      }
    }
  }
}
```

Labeled nested blocks (like `provisioner "local-exec"`) use the label as a nested key:
```json
{
  "provisioner": {
    "local-exec": { "command": "echo hello" }
  }
}
```

### Expression Handling in JSON

| JSON Value Type | Interpretation |
|---|---|
| String | Parsed as HCL template — `${...}` interpolations are evaluated |
| Number | Literal number |
| Boolean | Literal boolean |
| Null | Literal null |
| Object | Object/map value |
| Array | Tuple/list value |

**Critical rule:** A string containing *only* a single `${...}` interpolation returns the raw value (not stringified). E.g., `"${var.count}"` returns a number if `var.count` is a number.

### Block-Specific Rules
- **`variable.type`** — literal string, NOT an expression template
- **`module.source`** and **`module.version`** — literal strings
- **`terraform`** block — all values are literals, not expressions
- **`locals`** — all values ARE expressions (template-parsed)
- **`output.value`** — is an expression

---

## 2. How Terraform/OpenTofu Parse JSON (Go Source Analysis)

Source: `hashicorp/hcl` v2 repo (`/tmp/hashicorp-hcl/json/`) and `opentofu/opentofu` (`/tmp/opentofu/internal/configs/`)

### Architecture Overview

The parsing pipeline has 3 layers:

```
Layer 1: JSON lexing/parsing     json/parser.go, json/scanner.go
         ↓ produces json/ast.go node tree (objectVal, stringVal, etc.)
Layer 2: Schema-driven decoding  json/structure.go (body.PartialContent + unpackBlock)
         ↓ produces hcl.Body / hcl.Block / hcl.Attribute
Layer 3: Config interpretation   opentofu/internal/configs/parser_config.go
         ↓ produces configs.File (resources, providers, variables, etc.)
```

Key insight: **JSON and HCL converge at the `hcl.Body` interface** (layer 2). OpenTofu doesn't care about source format after that.

### Layer 1: JSON Parser (`json/parser.go`)

Custom JSON lexer/parser (NOT `encoding/json`). Preserves:
- Property ordering (critical for block label ordering)
- Duplicate property names (needed for multiple blocks of same type)
- Source locations (byte/line/column ranges for error messages)

**AST node types** (`json/ast.go`):
```go
type objectVal struct { Attrs []*objectAttr; SrcRange, OpenRange, CloseRange hcl.Range }
type objectAttr struct { Name string; Value node; NameRange hcl.Range }
type arrayVal  struct { Values []node; SrcRange, OpenRange hcl.Range }
type stringVal struct { Value string; SrcRange hcl.Range }
type numberVal struct { Value *big.Float; SrcRange hcl.Range }
type booleanVal struct { Value bool; SrcRange hcl.Range }
type nullVal   struct { SrcRange hcl.Range }
```

The parser is a recursive descent parser (`parseValue` → `parseObject`/`parseArray`/`parseString`/etc.) with error recovery. It builds a tree of these AST nodes.

### Layer 2: Schema-Driven Block Extraction (`json/structure.go`)

This is the critical layer. The JSON `body` struct wraps an AST node and implements `hcl.Body`:

```go
type body struct {
    val         node
    hiddenAttrs map[string]struct{} // for PartialContent remaining body
}
```

**`PartialContent(schema)`** is the core method. Given a schema that says which names are attributes vs. blocks:

1. Calls `collectDeepAttrs(val)` to flatten the root object (or array of objects) into a list of `objectAttr`
2. For each JSON property:
   - If schema says it's an **attribute** → wrap the value as an `expression` (lazy eval)
   - If schema says it's a **block** → call `unpackBlock()` to recursively peel labels

**`unpackBlock(v, typeName, typeRange, labelsLeft, ...)`** — the label-peeling algorithm:

```
Given labelsLeft = ["type", "name"] and JSON:
  {"aws_instance": {"web": {"ami": "..."}}}

Step 1: labelsLeft=["type","name"], peel "aws_instance" as first label
  → recurse with labelsLeft=["name"], v = {"web": {"ami": "..."}}
Step 2: labelsLeft=["name"], peel "web" as second label
  → recurse with labelsLeft=[], v = {"ami": "..."}
Step 3: labelsLeft=[], reached block body
  → if objectVal: create single hcl.Block{Type:"resource", Labels:["aws_instance","web"], Body:...}
  → if arrayVal:  create multiple hcl.Blocks with same type+labels (one per array element)
  → if nullVal:   skip (no block content)
```

**`collectDeepAttrs(v)`** handles both single objects and arrays of objects:
- `objectVal` → return its `Attrs` directly
- `arrayVal` → flatten: iterate elements, each must be `objectVal`, collect all attrs
- `nullVal` → return empty (no error)

**`"//"` comment handling**: In `Content()` and `JustAttributes()`, properties named `"//"` are explicitly skipped.

### Layer 2: Expression Evaluation (`json/structure.go` — `expression.Value()`)

Expressions are **lazily evaluated** — the JSON parser stores raw AST nodes, and evaluation happens when `Value(ctx)` is called:

```go
func (e *expression) Value(ctx *hcl.EvalContext) (cty.Value, hcl.Diagnostics) {
    switch v := e.src.(type) {
    case *stringVal:
        if ctx != nil {
            // Parse as HCL template — this is where ${...} interpolation happens
            expr, diags := hclsyntax.ParseTemplate([]byte(v.Value), ...)
            val, evalDiags := expr.Value(ctx)
            return val, diags
        }
        return cty.StringVal(v.Value), nil  // nil ctx = literal mode
    case *numberVal:  return cty.NumberVal(v.Value), nil
    case *booleanVal: return cty.BoolVal(v.Value), nil
    case *arrayVal:   // → cty.TupleVal (recursive)
    case *objectVal:  // → cty.ObjectVal (recursive, keys also template-parsed)
    case *nullVal:    return cty.NullVal(cty.DynamicPseudoType), nil
    }
}
```

**Critical distinction**: `ctx == nil` means literal mode (no template parsing), `ctx != nil` means full expression mode. This is how Terraform differentiates:
- `variable.type` = literal (evaluated with nil context)
- `resource.ami` = expression (evaluated with full context)

### Layer 3: OpenTofu Config Loading (`opentofu/internal/configs/`)

**File discovery** (`parser_config_dir.go`):
```go
const (
    tfExt       = ".tf"
    tfJSONExt   = ".tf.json"
    tofuExt     = ".tofu"
    tofuJSONExt = ".tofu.json"
)
```
`dirFiles()` scans a directory, matches extensions, sorts into primary vs. override files. `.tofu.json` takes precedence over `.tf.json`.

**Format selection** (`parser.go:LoadHCLFile()`):
```go
switch {
case strings.HasSuffix(path, ".json"):
    file, diags = p.p.ParseJSON(src, path)   // → json.Parse()
default:
    file, diags = p.p.ParseHCL(src, path)    // → hclsyntax.Parse()
}
```
Both return `hcl.Body`. After this point, the code is format-agnostic.

**Config schema** (`parser_config.go:configFileSchema`):
```go
var configFileSchema = &hcl.BodySchema{
    Blocks: []hcl.BlockHeaderSchema{
        {Type: "terraform"},
        {Type: "provider",  LabelNames: []string{"name"}},
        {Type: "variable",  LabelNames: []string{"name"}},
        {Type: "locals"},
        {Type: "output",    LabelNames: []string{"name"}},
        {Type: "module",    LabelNames: []string{"name"}},
        {Type: "resource",  LabelNames: []string{"type", "name"}},
        {Type: "data",      LabelNames: []string{"type", "name"}},
        {Type: "moved"},
        {Type: "import"},
        {Type: "check",     LabelNames: []string{"name"}},
        {Type: "removed"},
    },
}
```

This schema is the **key to the entire JSON parsing**. It tells `body.PartialContent()`:
- "resource" is a block with 2 labels → `unpackBlock` peels 2 levels of JSON nesting
- "provider" is a block with 1 label → peel 1 level
- "terraform" is a block with 0 labels → direct body
- Everything else in the schema is an attribute

**Terraform block sub-schema** (`terraformBlockSchema`):
```go
var terraformBlockSchema = &hcl.BodySchema{
    Attributes: []hcl.AttributeSchema{
        {Name: "required_version"},
    },
    Blocks: []hcl.BlockHeaderSchema{
        {Type: "backend",           LabelNames: []string{"type"}},
        {Type: "required_providers"},
    },
}
```

### Summary: What Oxid Needs to Replicate

For Oxid, we don't have the `hcl.Body` interface or schema system. Instead we need to replicate the **effect** of `body.PartialContent(configFileSchema)` + `unpackBlock()`:

1. **Know the schema** — hardcode which top-level keys are blocks and how many labels each has (same as `configFileSchema`)
2. **Peel labels** — for "resource" (2 labels), walk 2 levels of nested JSON objects
3. **Handle arrays** — at any nesting level, an array means multiple blocks/labels
4. **Skip `"//"`** — ignore comment properties
5. **Expression handling** — strings containing `${...}` are template expressions (Oxid already handles this in the HCL parser via `parse_template_string()`)

---

## 3. Rust/Cargo Ecosystem Survey

### hcl-rs (v0.18 — already used by Oxid)
- **Repo:** [martinohmann/hcl-rs](https://github.com/martinohmann/hcl-rs)
- **JSON parsing support: NO** — `hcl::parse()` and `hcl::from_str()` only parse native HCL syntax
- **JSON serialization: YES** — `json_spec.rs` module converts HCL Body → JSON-spec-compliant output (for `hcl2json`)
- The `de` module's serde deserializer follows the JSON spec for its *output* shape, but does NOT parse JSON *input*
- **Expression evaluation:** Built-in via `hcl::eval` module
- **Template parsing:** Supports `${...}` interpolation
- **Conclusion: Oxid needs a custom JSON→WorkspaceConfig parser**

### Sub-crates in hcl-rs ecosystem
| Crate | Purpose | Useful for tf.json? |
|---|---|---|
| `hcl-edit` | Parse/modify HCL preserving formatting | No (HCL only) |
| `hcl-primitives` | Low-level types for expressions/templates | Potentially (expression parsing) |
| `hcl2json` | CLI: HCL→JSON conversion | No (wrong direction) |
| `json2hcl` | CLI: JSON→HCL round-trip | **No** (see below) |

### json2hcl Deep Dive (NOT useful)
The `json2hcl` crate (v0.1.0, 3KB, 1,550 downloads, last updated Aug 2022) is a trivial 4-line binary:
```rust
fn main() {
    let value: hcl::Body = serde_json::from_reader(std::io::stdin()).unwrap();
    hcl::to_writer(std::io::stdout(), &value).unwrap();
}
```
This does **NOT** parse Terraform JSON configuration syntax. It parses hcl-rs's own internal serde format:
- `Body` has `#[derive(Deserialize)]` with `#[serde(rename = "$hcl::Body")]`
- `Body` is a newtype: `pub struct Body(pub Vec<Structure>)`
- `Structure` is a tagged enum: `Attribute(Attribute) | Block(Block)`
- serde_json expects: `{"$hcl::Body": [{"Attribute": {"key": "...", "expr": ...}}]}`

This is a round-trip format for hcl-rs internals, completely different from the Terraform `.tf.json` format.

### Other Crates
| Crate | Purpose | Useful? |
|---|---|---|
| `terraform-parser` | Parse .tfstate and plan.json | No (wrong file types) |
| `tf-provider` | Build Terraform providers in Rust | No (provider development) |
| `tfschema-bindgen` | Generate Rust types from provider schemas | No (codegen tool) |
| `terrars` | Write IaC in Rust, generates .tf.json | No (generates JSON, doesn't parse it) |
| `serde_json` | JSON parsing | YES — already in Cargo.toml |

### Key Finding
**No existing Rust crate parses `.tf.json` files into Terraform-style block structures.** The implementation must be custom, using `serde_json` for JSON parsing and applying the Terraform JSON spec mapping rules manually to produce `WorkspaceConfig`.

---

## 4. Oxid's Current Architecture (relevant files)

| File | Role | Impact |
|---|---|---|
| `src/config/loader.rs` | Detects mode (HCL/YAML), calls `parse_directory()` | Must add `.tf.json` detection |
| `src/hcl/mod.rs` | Scans for `.tf` files, merges into WorkspaceConfig | Must also scan `.tf.json` files |
| `src/hcl/parser.rs` | Parses HCL content → WorkspaceConfig blocks | New JSON parser needed (parallel to this) |
| `src/config/types.rs` | `WorkspaceConfig`, `ResourceConfig`, `Expression`, etc. | Reused as-is (target types) |

### Current parsing flow
```
.tf file → hcl::from_str() → hcl::Body → parse_hcl() → WorkspaceConfig
```

### Required new flow
```
.tf.json file → serde_json::from_str() → serde_json::Value → parse_tf_json() → WorkspaceConfig
```

Both flows produce the same `WorkspaceConfig`, then merge via `merge_workspace()`.

---

---

## 5. Implementation Approaches

### Approach A: JSON → WorkspaceConfig directly
- Parse `.tf.json` with `serde_json` → `serde_json::Value`
- Write `parse_tf_json()` function that maps JSON keys to WorkspaceConfig fields directly
- Parallel to but independent of the HCL parser
- **Pro:** Simple, direct
- **Con:** Duplicates block parsing logic from `src/hcl/parser.rs`

### Approach B: JSON → hcl::Body → reuse existing parse_hcl() (RECOMMENDED)
- Parse `.tf.json` with `serde_json` → `serde_json::Value`
- Write `json_to_body()` converter that constructs `hcl::Body` programmatically using `Block::builder()`, `Attribute::new()`, etc.
- Feed the resulting `hcl::Body` to the existing `parse_hcl()` function which converts to `WorkspaceConfig`
- **Pro:** Reuses ALL existing block parsing logic (providers, resources, variables, etc.)
- **Con:** Extra conversion step (JSON → Body → WorkspaceConfig vs. JSON → WorkspaceConfig)
- **Why recommended:** The block parsing logic in `parser.rs` is 793 lines of carefully tested code. Reusing it avoids bugs and ensures feature parity. The JSON→Body conversion is straightforward since `hcl::Body`, `Block`, and `Attribute` have builder APIs.

### Note on hcl-rs JSON deserialize path
`hcl::Body` derives `serde::Deserialize` and can be deserialized via `serde_json::from_reader()`, but the expected format is hcl-rs's **internal tagged enum format** (`{"$hcl::Body": [{"Attribute": ...}, {"Block": ...}]}`), NOT the Terraform JSON configuration syntax. The `json2hcl` crate (v0.1.0, hcl-rs 0.6.4) uses this internal format and is not useful for our use case.

---

## 6. CDKTF Fixture Analysis

8 real CDKTF-generated `.tf.json` fixtures exist at `tests/fixtures/tf-json/`:
- `foreach/`, `iam-grants/`, `stepfunctions/`, `compute-events/`, `encryption/`, `modules/`, `multi-provider/`, `storage-autoscaling/`

### CDKTF-Specific Patterns Observed

| Pattern | Example | Handling Notes |
|---|---|---|
| `"//"` comment metadata | `"//": {"metadata": {"path": "...", "uniqueId": "..."}}` | Strip and ignore (per spec) |
| Provider arrays | `"aws": [{"region": "us-east-1"}]` | Array = multiple configs (even for 1) |
| `for_each` as expression | `"${toset([\"alpha\", \"beta\"])}"` | String → template expression |
| `for_each` as map literal | `{"ap": "ap-southeast-1", "us": "us-east-1"}` | Object → direct value |
| Nested blocks as arrays | `"statement": [{"actions": [...]}]` | Array of objects = repeated blocks |
| Nested blocks as objects | `"tracing_config": {"mode": "Active"}` | Single object = single block |
| `depends_on` as array | `["aws_iam_role.X", "data.aws_iam_policy_document.Y"]` | String array (no `${}`) |
| Cross-resource refs | `"${aws_iam_role.AppRole_DC883459.arn}"` | Template interpolation |
| Backend labeled block | `"backend": {"local": {"path": "..."}}` | Label = nested key |
| Embedded JSON strings | Step Functions `definition` field with ASL JSON | Literal string (not parsed) |
| Output `value` as object | `"value": {"arn": "${...}", "name": "${...}"}` | Object value with interpolations |
| Variable `type` as string | `"list(string)"`, `"map(object({...}))"` | Literal string (not template) |

### Key CDKTF Observations
1. CDKTF always wraps providers in arrays (even for single provider)
2. CDKTF generates `"//"` metadata everywhere (must be ignored)
3. `depends_on` values are plain strings (no `${}` wrapping) — e.g., `"aws_iam_role.X"` not `"${aws_iam_role.X}"`
4. Nested blocks like `statement`, `condition`, `principals` use arrays
5. Single nested blocks like `tracing_config`, `environment`, `backend` use objects

---

## Summary

- **Spec is well-defined** — straightforward JSON-to-block mapping with clear rules
- **No existing Rust crate** handles `.tf.json` → Terraform blocks
- **Custom parser needed** using `serde_json` (already a dependency) + Terraform JSON spec rules
- **Recommended: Approach B** — convert JSON → `hcl::Body` → reuse existing `parse_hcl()`
- **Reuses** existing `WorkspaceConfig` types, `merge_workspace()`, and all block parsing logic
- **Main complexity** is in the JSON→Body conversion: applying label nesting rules and handling array-vs-object block disambiguation
