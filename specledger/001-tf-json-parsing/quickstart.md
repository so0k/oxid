# Quickstart: tf.json Support in Oxid

## Usage

Point Oxid at a directory containing `.tf.json` files — no flags or configuration needed:

```bash
# JSON-only directory (e.g., CDKTF output)
oxid plan

# Mixed directory (.tf + .tf.json)
oxid plan

# Specific directory
oxid plan --config ./cdktf.out/stacks/my-stack/
```

Oxid auto-detects `.tf.json` files alongside `.tf` files. Both formats merge into a single plan.

## Generating tf.json Files

### CDKTF (CDK for Terraform)

```bash
cd my-cdktf-project
cdktf synth           # Produces cdktf.out/stacks/*/cdk.tf.json
oxid plan --config cdktf.out/stacks/my-stack/
```

### Pulumi (Terraform export)

```bash
pulumi preview --json > main.tf.json
oxid plan
```

### Hand-Written JSON

Create a `main.tf.json` file:

```json
{
  "provider": {
    "random": [{}]
  },
  "resource": {
    "random_pet": {
      "example": {
        "length": 3,
        "prefix": "oxid"
      }
    }
  },
  "output": {
    "pet_name": {
      "value": "${random_pet.example.id}"
    }
  },
  "terraform": {
    "required_providers": {
      "random": {
        "source": "hashicorp/random",
        "version": "3.7.2"
      }
    }
  }
}
```

```bash
oxid init && oxid plan
```

## Supported Block Types

All standard Terraform block types are supported:

| Block | Example JSON Key | Label Nesting |
|-------|-----------------|---------------|
| `resource` | `"resource": {"type": {"name": {...}}}` | 2 levels |
| `data` | `"data": {"type": {"name": {...}}}` | 2 levels |
| `provider` | `"provider": {"name": [{...}]}` | 1 level |
| `variable` | `"variable": {"name": {...}}` | 1 level |
| `output` | `"output": {"name": {...}}` | 1 level |
| `module` | `"module": {"name": {...}}` | 1 level |
| `terraform` | `"terraform": {...}` | 0 levels |
| `locals` | `"locals": {"key": "value"}` | 0 levels |

## Expression Syntax

In `.tf.json` files, expressions use `${...}` interpolation within JSON strings:

```json
{
  "resource": {
    "aws_instance": {
      "web": {
        "ami": "${var.ami_id}",
        "instance_type": "t2.micro",
        "tags": {
          "Name": "${var.project}-web"
        }
      }
    }
  }
}
```

- `"${var.ami_id}"` → variable reference (returns the variable's typed value)
- `"t2.micro"` → literal string (no `${}` = no interpolation)
- `"${var.project}-web"` → string template with embedded reference

## Limitations

- **Dynamic blocks**: Not supported in `.tf.json` (CDKTF does not generate them)
- **`//` suffix convention**: Expression-mode meta-arguments like `"count//"` are not supported; use `"${...}"` instead
- **`.tfvars.json`**: Not supported by this feature (use `.tfvars` or `TF_VAR_` env vars)
- **Override files**: `_override.tf.json` / `override.tf.json` are not supported
