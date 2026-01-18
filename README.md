# probe-verus

Probe Verus projects: generate call graph atoms and analyze verification results.

## Installation

```bash
cargo install --path .
```

**Prerequisites:** Some commands require external tools (verus-analyzer, scip, cargo verus).  
See [INSTALL.md](INSTALL.md) for detailed installation instructions.

## Commands

```
probe-verus <COMMAND>

Commands:
  atomize         Generate call graph atoms with line numbers from SCIP indexes
  list-functions  List all functions in a Rust/Verus project
  specify         Extract function specifications from atoms.json
  verify          Run Verus verification and analyze results
```

---

### `atomize` - Generate Call Graph Data

Generate call graph atoms with line numbers from SCIP indexes.

```bash
probe-verus atomize <PROJECT_PATH> [OPTIONS]

Options:
  -o, --output <FILE>     Output file path (default: atoms.json)
  -r, --regenerate-scip   Force regeneration of the SCIP index
      --with-locations    Include detailed per-call location info (precondition/postcondition/inner)
```

**Examples:**
```bash
probe-verus atomize ./my-rust-project
probe-verus atomize ./my-rust-project -o output.json
probe-verus atomize ./my-rust-project --regenerate-scip
probe-verus atomize ./my-rust-project --with-locations  # extended output
```

**Output format:**

The output is a dictionary keyed by `probe-name` (a URI-style identifier):

```json
{
  "probe:curve25519-dalek/4.1.3/module/MyType#my_function()": {
    "display-name": "my_function",
    "dependencies": [
      "probe:curve25519-dalek/4.1.3/other_module/helper()"
    ],
    "code-module": "module",
    "code-path": "src/lib.rs",
    "code-text": { "lines-start": 42, "lines-end": 100 },
    "mode": "proof"
  }
}
```

**Field descriptions:**
- **Key (`probe-name`)**: URI-style identifier in format `probe:<crate>/<version>/<module>/<Type>#<method>()`
- **`display-name`**: The function/method name
- **`dependencies`**: List of probe-names this function calls (deduplicated)
- **`code-module`**: The module path (e.g., `"foo/bar"` for nested modules, empty for top-level)
- **`code-path`**: Relative file path
- **`code-text`**: Line range of the function body
- **`mode`**: Verus function mode (`"exec"`, `"proof"`, or `"spec"`)

**Extended output (`--with-locations`):**

When using `--with-locations`, an additional `dependencies-with-locations` field is included:

```json
{
  "probe:crate/1.0.0/module/my_function()": {
    "display-name": "my_function",
    "dependencies": ["probe:crate/1.0.0/other/helper()"],
    "dependencies-with-locations": [
      {
        "code-name": "probe:crate/1.0.0/other/helper()",
        "location": "precondition",
        "line": 45
      },
      {
        "code-name": "probe:crate/1.0.0/other/helper()",
        "location": "inner",
        "line": 52
      }
    ],
    "code-module": "module",
    "code-path": "src/lib.rs",
    "code-text": { "lines-start": 42, "lines-end": 100 },
    "mode": "exec"
  }
}
```

The `location` field indicates where the call occurs:
- **`precondition`**: Inside a `requires` clause
- **`postcondition`**: Inside an `ensures` clause
- **`inner`**: Inside the function body

This is useful for verification analysis since calls in specifications have different semantics than calls in executable code.

**Note:** Duplicate `probe-name` values are a fatal error (exit code 1).

---

### `list-functions` - List Functions

List all functions in a Rust/Verus project with optional metadata.

```bash
probe-verus list-functions <PATH> [OPTIONS]

Options:
  -f, --format <FORMAT>          text, json, or detailed (default: text)
      --exclude-verus-constructs Exclude spec/proof/exec functions
      --exclude-methods          Exclude trait and impl methods
      --show-visibility          Show pub/private
      --show-kind                Show fn/spec fn/proof fn/etc.
      --json-output <FILE>       Write JSON to file
```

**Examples:**
```bash
probe-verus list-functions ./src
probe-verus list-functions ./src --format detailed --show-visibility --show-kind
probe-verus list-functions ./my-project --format json
```

---

### `specify` - Extract Function Specifications

Extract function specifications (requires/ensures clauses) from source files, keyed by probe-name from atoms.json.

```bash
probe-verus specify <PATH> --with-code-names <ATOMS_FILE> [OPTIONS]

Options:
      --json-output <FILE>     Output file path (default: specs.json)
      --with-code-names <FILE> Path to atoms.json for code-name lookup (required)
      --with-spec-text         Include raw specification text in output
```

**Examples:**
```bash
# Extract specs using atoms.json for probe-name mapping
probe-verus specify ./src --with-code-names atoms.json

# Include raw requires/ensures text
probe-verus specify ./src --with-code-names atoms.json --with-spec-text

# Custom output file
probe-verus specify ./src --with-code-names atoms.json --json-output my-specs.json
```

**Output format:**

```json
{
  "probe:crate/1.0.0/module/my_function()": {
    "code-path": "src/lib.rs",
    "spec-text": {
      "lines-start": 42,
      "lines-end": 60
    },
    "has_requires": true,
    "has_ensures": true,
    "has_trusted_assumption": false
  }
}
```

**Field descriptions:**
- **Key**: The probe-name from atoms.json
- **`code-path`**: Source file path
- **`spec-text`**: Function span with `lines-start` and `lines-end`
- **`has_requires`**: Whether the function has a `requires` clause (precondition)
- **`has_ensures`**: Whether the function has an `ensures` clause (postcondition)
- **`has_trusted_assumption`**: Whether the function contains `assume()` or `admit()`

**Extended output (`--with-spec-text`):**

```json
{
  "probe:crate/1.0.0/module/my_function()": {
    "code-path": "src/lib.rs",
    "spec-text": {
      "lines-start": 42,
      "lines-end": 60
    },
    "has_requires": true,
    "has_ensures": true,
    "has_trusted_assumption": false,
    "requires_text": "x > 0 && y > 0",
    "ensures_text": "result == x + y"
  }
}
```

---

### `verify` - Run Verus Verification

Run Verus verification on a project and analyze results. Supports caching for quick re-analysis.

```bash
probe-verus verify [PROJECT_PATH] [OPTIONS]

Options:
      --from-file <FILE>         Analyze existing output file instead of running verification
      --exit-code <CODE>         Exit code (only used with --from-file)
  -p, --package <NAME>           Package to verify (for workspaces)
      --verify-only-module <MOD> Module to verify
      --verify-function <FUNC>   Function to verify
      --json-output <FILE>       Write JSON results to file (default: results.json)
      --no-cache                 Don't cache the verification output
      --with-code-names [FILE]   Enrich results with code-names from atoms.json
```

**Caching Workflow:**

```bash
# First run: runs verification and caches output to data/
probe-verus verify ./my-verus-project -p my-crate

# Subsequent runs: uses cached output (no need to re-run verification)
probe-verus verify
```

**Examples:**
```bash
# Run verification (caches output automatically)
probe-verus verify ./my-verus-project
probe-verus verify ./my-workspace -p my-crate

# Use cached output (no project path needed)
probe-verus verify

# Analyze existing output file (from CI, etc.)
probe-verus verify ./my-project --from-file verification_output.txt

# Enrich results with probe-names from atoms.json
probe-verus verify --with-code-names
probe-verus verify --with-code-names path/to/atoms.json
```

**Function Categorization:**

Functions with `requires`/`ensures` are categorized as:
- **verified**: Passed verification, no `assume()`/`admit()`
- **failed**: Had verification errors
- **unverified**: Contains `assume()` or `admit()`

**Output format:**
```json
{
  "status": "verification_failed",
  "summary": {
    "total_functions": 262,
    "failed_functions": 2,
    "verified_functions": 171,
    "unverified_functions": 89
  },
  "verification": {
    "failed_functions": [
      {
        "display-name": "my_function",
        "code-name": "probe:crate/1.0.0/module/my_function()",
        "code-path": "src/lib.rs",
        "code-text": { "lines-start": 10, "lines-end": 20 }
      }
    ],
    "verified_functions": [...],
    "unverified_functions": [...]
  }
}
```

Note: `code-name` is only present when using `--with-code-names` and uses the URI format `probe:<crate>/<version>/<path>`.

---

## How It Works

See [docs/HOW_IT_WORKS.md](docs/HOW_IT_WORKS.md) for detailed technical documentation on:

- SCIP-based call graph generation
- Accurate line spans with verus_syn parsing
- Disambiguation of trait implementations
- Verification output parsing and function categorization

See [docs/VERIFICATION_ARCHITECTURE.md](docs/VERIFICATION_ARCHITECTURE.md) for the verification analysis architecture.

---

## License

MIT

