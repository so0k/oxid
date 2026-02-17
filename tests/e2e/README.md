# E2E Test Fixtures

End-to-end test fixtures for `oxid` CLI validation. Each directory contains a
complete infrastructure configuration in different formats.

## Fixtures

| Directory | Format | Providers | Tier |
|-----------|--------|-----------|------|
| `01-pure-hcl` | HCL only (.tf) | random, null, local | 1 (no creds) |
| `02-pure-tf-json` | JSON only (.tf.json) | random, null, local | 1 (no creds) |
| `03-mixed` | HCL + JSON | random, null, local | 1 (no creds) |
| `04-aws-hcl` | HCL only (.tf) | aws, random | 2 (AWS creds) |
| `05-aws-tf-json` | JSON only (.tf.json) | aws, random | 2 (AWS creds) |
| `06-aws-mixed` | HCL + JSON | aws, random | 2 (AWS creds) |

## Running Tests

```bash
# Fast unit/integration tests only (e2e tests are #[ignore]'d)
cargo test

# Tier 1: no-credentials e2e (needs network for provider downloads)
cargo test -- --ignored

# Tier 1 + Tier 2: all e2e (needs AWS credentials)
OXID_E2E_AWS=1 cargo test -- --ignored

# Single fixture
cargo test -- --ignored e2e_03_mixed
```

## What Each Command Exercises

| Command | Network | What it tests |
|---------|---------|---------------|
| `validate` | No | Config parsing (.tf, .tf.json, mixed), variable resolution, DAG construction |
| `init` | Yes | Provider registry discovery, binary download, gRPC handshake |
| `plan` | Yes | Full pipeline: parse -> DAG -> provider schemas -> diff -> plan output |

## Design Notes

- **`NO_COLOR=1`** is set on every command so the `colored` crate outputs plain text, making `predicates::str::contains` assertions reliable.
- **Temp work dirs** (`TempDir`) give each test its own `.oxid/` — isolated SQLite state and provider cache. No shared state between tests.
- **Provider cache is not shared** between tests (each downloads independently). This is intentional for isolation but means `init` + `plan` tests are slower (~60MB for Tier 1 providers, ~400MB for AWS).
