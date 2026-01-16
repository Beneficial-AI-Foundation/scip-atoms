# probe-verus Docker

Self-contained Docker image for running `probe-verus atomize` and `probe-verus verify` commands.

## Quick Start

Pull the pre-built image:

```bash
docker pull ghcr.io/beneficial-ai-foundation/probe-verus:latest
```

Or build locally:

```bash
cd /path/to/probe-verus
docker build -t probe-verus -f docker/Dockerfile .
```

## Usage

```bash
# Using the helper script (recommended)
./docker/run.sh /path/to/project [OPTIONS]

# Or directly with docker (using pre-built image)
docker run --rm --user root \
  -v /path/to/project:/workspace/project \
  -v /path/to/output:/workspace/output \
  ghcr.io/beneficial-ai-foundation/probe-verus:latest \
  /workspace/project -o /workspace/output [OPTIONS]
```

**Options:**
- `-o, --output <dir>` - Output directory (default: ./output)
- `--atomize-only` - Run only the atomize command
- `--verify-only` - Run only the verify command
- `-p, --package <name>` - Package name for workspace projects
- `--regenerate-scip` - Force regeneration of the SCIP index
- `-v, --verbose` - Enable verbose output

**Output files:**
- `atoms.json` - Call graph from atomize
- `results.json` - Verification results from verify
- `run_summary.json` - Overall run status

## Examples

### Workspace project with package selection

```bash
# For a Cargo workspace, specify the package to verify
docker run --rm --user root \
  -v ~/my-workspace:/workspace/project \
  -v ~/output:/workspace/output \
  probe-verus /workspace/project -o /workspace/output --package my-crate
```

### Atomize only (skip verification)

```bash
docker run --rm --user root \
  -v ~/my-project:/workspace/project \
  -v ~/output:/workspace/output \
  probe-verus /workspace/project -o /workspace/output --atomize-only
```

### Force SCIP regeneration

```bash
docker run --rm --user root \
  -v ~/my-project:/workspace/project \
  -v ~/output:/workspace/output \
  probe-verus /workspace/project -o /workspace/output --regenerate-scip
```

### Verbose output for debugging

```bash
docker run --rm --user root \
  -v ~/my-project:/workspace/project \
  -v ~/output:/workspace/output \
  probe-verus /workspace/project -o /workspace/output --verbose
```

## Output Files

| File | Description |
|------|-------------|
| `atoms.json` | Call graph with dependencies and line ranges |
| `results.json` | Verification results (verified/failed/unverified functions) |
| `run_summary.json` | Overall run status and summary |

### atoms.json format

```json
{
  "probe:crate/1.0.0/module/function()": {
    "display-name": "function",
    "dependencies": ["probe:crate/1.0.0/module/helper()"],
    "code-module": "module",
    "code-path": "src/lib.rs",
    "code-text": { "lines-start": 10, "lines-end": 25 }
  }
}
```

### results.json format

```json
{
  "status": "verification_failed",
  "summary": {
    "total_functions": 100,
    "failed_functions": 2,
    "verified_functions": 80,
    "unverified_functions": 18
  },
  "verification": {
    "failed_functions": [...],
    "verified_functions": [...],
    "unverified_functions": [...]
  }
}
```

### run_summary.json format

```json
{
  "status": "success",
  "atomize": {
    "success": true,
    "output_file": "/workspace/output/atoms.json",
    "total_functions": 42
  },
  "verify": {
    "success": true,
    "output_file": "/workspace/output/results.json",
    "summary": {
      "total_functions": 42,
      "verified": 40,
      "failed": 0,
      "unverified": 2
    }
  }
}
```

## What's Included

The Docker image includes:

- **Rust** (base toolchain + version required by Verus, auto-detected)
- **Verus** (latest stable release)
- **verus-analyzer** (latest, required for SCIP index generation)
- **scip** CLI (latest, required for SCIP to JSON conversion)
- **probe-verus** (built from source)

## Build Arguments

Customize the build with these arguments:

```bash
# Use specific user/group IDs
docker build -t probe-verus -f docker/Dockerfile \
  --build-arg USER_UID=1001 \
  --build-arg USER_GID=1001 \
  .
```

| Argument | Default | Description |
|----------|---------|-------------|
| `RUST_VERSION` | `1.88.0` | Base Rust toolchain (for building probe-verus) |
| `USER_UID` | `1000` | UID for the non-root user |
| `USER_GID` | `1000` | GID for the non-root user |

**Note:** The Rust toolchain required by Verus is detected and installed automatically from the latest Verus release.

## Security

The container image defaults to a non-root user (`verus`, UID 1000) for security.

However, the `run.sh` helper script uses `--user root` because Verus verification needs to write build artifacts to the mounted project directory. If your host user isn't UID 1000, you'll get permission errors otherwise.

For direct `docker run` usage, add `--user root` if you encounter permission issues:

```bash
docker run --rm --user root -v ... probe-verus ...
```

## Troubleshooting

### Permission denied on output directory

The container runs as UID 1000 by default. Make sure your output directory is writable:

```bash
mkdir -p ./output
chmod 777 ./output  # Or use matching UID
```

### SCIP index errors

Try regenerating the SCIP index:

```bash
docker run ... probe-verus /workspace/project -o /workspace/output --regenerate-scip
```

### Debugging issues

Use verbose mode to see detailed output:

```bash
docker run ... probe-verus /workspace/project -o /workspace/output --verbose
```

### Verification timeout

For large projects, verification may take a long time. The Docker container doesn't impose time limits.

## Helper Script

The `run.sh` script simplifies common usage:

```bash
# Basic usage (uses pre-built GHCR image by default)
./docker/run.sh ~/my-project

# With options
./docker/run.sh ~/my-project --package my-crate --verbose

# Custom output directory
./docker/run.sh ~/my-project --output ./my-output

# Use locally built image instead
PROBE_VERUS_IMAGE=probe-verus ./docker/run.sh ~/my-project
```
