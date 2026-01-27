# BAIF Verus Verify Action

A GitHub Action to run [Verus](https://github.com/verus-lang/verus) formal verification on Rust projects and produce structured verification results.

## Features

- Auto-detects Verus and Rust versions from `Cargo.toml`
- Installs all required tooling (Verus, verus-analyzer, scip, probe-verus)
- Caches installations for faster subsequent runs
- Produces JSON output suitable for certification with [certify-action](https://github.com/beneficial-ai-foundation/certify)
- Outputs verification statistics (verified/total counts)

## Usage

### Basic Usage

```yaml
- uses: beneficial-ai-foundation/probe-verus/action@v1
  id: verify
  with:
    project-path: ./my-verus-crate
```

### With Explicit Versions

```yaml
- uses: beneficial-ai-foundation/probe-verus/action@v1
  id: verify
  with:
    project-path: ./my-verus-crate
    verus-version: '1.85.0'
    rust-version: 'nightly-2025-01-01'
```

### Workspace Project

```yaml
- uses: beneficial-ai-foundation/probe-verus/action@v1
  id: verify
  with:
    project-path: ./my-workspace
    package: my-verus-crate
```

### Using Outputs

```yaml
- uses: beneficial-ai-foundation/probe-verus/action@v1
  id: verify
  with:
    project-path: ./my-verus-crate

- name: Display results
  run: |
    echo "Verified: ${{ steps.verify.outputs.verified-count }} / ${{ steps.verify.outputs.total-functions }}"
    echo "Results file: ${{ steps.verify.outputs.results-file }}"
```

## Inputs

| Input | Required | Default | Description |
|-------|----------|---------|-------------|
| `project-path` | Yes | | Path to the Verus project directory |
| `package` | No | | Package name for workspace projects |
| `verus-version` | No | auto-detect | Verus version (e.g., `1.85.0`) |
| `rust-version` | No | auto-detect | Rust toolchain version |
| `output-dir` | No | `.` | Directory for output files |

## Outputs

| Output | Description |
|--------|-------------|
| `results-file` | Path to verification results JSON |
| `atoms-file` | Path to atoms JSON (call graph) |
| `verified-count` | Number of functions verified |
| `total-functions` | Total number of functions |

## Auto-Detection

If `verus-version` or `rust-version` are not provided, the action looks for them in your project's `Cargo.toml`:

```toml
[package.metadata.verus]
release = "1.85.0"
rust-version = "nightly-2025-01-01"
```

## Complete Example: Verify and Certify

This example shows how to combine with the [certify-action](https://github.com/beneficial-ai-foundation/certify) to record verification results on Ethereum:

```yaml
name: Verify and Certify

on:
  push:
    branches: [main]
    paths:
      - 'src/**/*.rs'

jobs:
  verify-and-certify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # Run Verus verification
      - uses: beneficial-ai-foundation/probe-verus/action@v1
        id: verify
        with:
          project-path: ./my-verus-crate

      # Certify results on Ethereum
      - uses: beneficial-ai-foundation/certify/action@v1
        id: certify
        with:
          source: ${{ steps.verify.outputs.results-file }}
          description: "Verus verification: ${{ steps.verify.outputs.verified-count }}/${{ steps.verify.outputs.total-functions }} verified"
          network: sepolia
          rpc-url: ${{ secrets.SEPOLIA_RPC_URL }}
          private-key: ${{ secrets.SEPOLIA_PRIVATE_KEY }}
          certify-address: ${{ vars.CERTIFY_ADDRESS }}

      - name: Summary
        run: |
          echo "## Verification Results" >> $GITHUB_STEP_SUMMARY
          echo "" >> $GITHUB_STEP_SUMMARY
          echo "- **Verified**: ${{ steps.verify.outputs.verified-count }} / ${{ steps.verify.outputs.total-functions }}" >> $GITHUB_STEP_SUMMARY
          echo "- **Certification**: [${{ steps.certify.outputs.tx-hash }}](${{ steps.certify.outputs.etherscan-url }})" >> $GITHUB_STEP_SUMMARY
```

## Output File Format

### results.json

```json
{
  "summary": {
    "verified": 42,
    "failed": 2,
    "total": 44
  },
  "functions": [
    {
      "name": "my_function",
      "status": "verified",
      "file": "src/lib.rs",
      "line": 10
    }
  ]
}
```

### atoms.json

Contains call graph information mapping functions to their dependencies, used to enrich verification results with human-readable names.

## Requirements

- Linux runner (ubuntu-latest recommended)
- Project must be a valid Verus/Rust project
- Either provide versions via inputs or include `[package.metadata.verus]` in Cargo.toml

## License

MIT
