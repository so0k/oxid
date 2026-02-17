# CDKTF Test Fixture Generator

Generates `cdk.tf.json` files as test fixtures for Oxid's tf.json parsing feature. Uses [CDKTF](https://developer.hashicorp.com/terraform/cdktf) to synthesize realistic Terraform JSON configurations from TypeScript.

No cloud credentials are needed — CDKTF synthesis is purely local code-to-JSON.

## Usage

```bash
bun install          # first time only
bun run synth        # synthesize all packages
bun run collect      # copy fixtures to tests/fixtures/tf-json/
```

## Packages

### Credential-free providers

| Package | Providers | Patterns exercised |
|---|---|---|
| `foreach` | random, null | `for_each` over list/map/static, `each.value`/`each.key` |
| `modules` | random, null, local | `TerraformHclModule` (registry + local), construct composition, `depends_on` |
| `multi-provider` | random, null, local, tls, time | 5 providers, locals, sensitive outputs, `depends_on` |

### AWS via [terraconstructs](https://github.com/TerraConstructs/base)

| Package | Patterns exercised |
|---|---|
| `iam-grants` | IAM roles, managed policies, grant chains, conditions with `Lazy` refs, org principals |
| `encryption` | KMS keys, aliases, resource policies, asymmetric keys, encrypt/decrypt grants |
| `compute-events` | Lambda + SQS event source mapping, DLQ, queue grants, inline code |
| `storage-autoscaling` | DynamoDB + autoscaling targets/policies/scheduled actions, resource policies |
| `stepfunctions` | State machine (Choice/Fail/Pass), Lambda invoke tasks, ASL definition |

## Adding a new fixture

1. Create a directory under `packages/`
2. Add `package.json`, `cdktf.json`, `tsconfig.json`, and `main.ts`
3. Run `bun install && bun run synth && bun run collect`

Use existing packages as templates. For AWS resources, depend on `terraconstructs` and follow the patterns in its [integ tests](https://github.com/TerraConstructs/base/tree/main/integ/aws).
