# scip-atoms

Generate compact function call graph data from SCIP indexes and analyze Verus verification results.

## Installation

```bash
cargo install --path .
```

**Prerequisites:** Some commands require external tools (verus-analyzer, scip, cargo verus).  
See [INSTALL.md](INSTALL.md) for detailed installation instructions.

## Commands

```
scip-atoms <COMMAND>

Commands:
  atoms      Generate SCIP-based call graph atoms with line numbers
  functions  List all functions in a Rust/Verus project
  verify     Run Verus verification and analyze results
```

---

### `atoms` - Generate Call Graph Data

Generate SCIP-based call graph atoms with line numbers.

```bash
scip-atoms atoms <PROJECT_PATH> [OPTIONS]

Options:
  -o, --output <FILE>     Output file path (default: atoms.json)
  -r, --regenerate-scip   Force regeneration of the SCIP index
```

**Examples:**
```bash
scip-atoms atoms ./my-rust-project
scip-atoms atoms ./my-rust-project -o output.json
scip-atoms atoms ./my-rust-project --regenerate-scip
```

**Output format:**

The output is a dictionary keyed by `scip-name` (a URI-style identifier):

```json
{
  "scip:curve25519-dalek/4.1.3/module/MyType#my_function()": {
    "display-name": "my_function",
    "dependencies": [
      "scip:curve25519-dalek/4.1.3/other_module/helper()"
    ],
    "code-module": "module",
    "code-path": "src/lib.rs",
    "code-text": { "lines-start": 42, "lines-end": 100 }
  }
}
```

**Field descriptions:**
- **Key (`scip-name`)**: URI-style identifier in format `scip:<crate>/<version>/<module>/<Type>#<method>()`
- **`display-name`**: The function/method name
- **`dependencies`**: List of scip-names this function calls
- **`code-module`**: The module path (e.g., `"foo/bar"` for nested modules, empty for top-level)
- **`code-path`**: Relative file path
- **`code-text`**: Line range of the function body

**Note:** Duplicate `scip-name` values are a fatal error (exit code 1).

---

### `functions` - List Functions

List all functions in a Rust/Verus project with optional metadata.

```bash
scip-atoms functions <PATH> [OPTIONS]

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
scip-atoms functions ./src
scip-atoms functions ./src --format detailed --show-visibility --show-kind
scip-atoms functions ./my-project --format json
```

---

### `verify` - Run Verus Verification

Run Verus verification on a project and analyze results. Supports caching for quick re-analysis.

```bash
scip-atoms verify [PROJECT_PATH] [OPTIONS]

Options:
      --from-file <FILE>         Analyze existing output file instead of running verification
      --exit-code <CODE>         Exit code (only used with --from-file)
  -p, --package <NAME>           Package to verify (for workspaces)
      --verify-only-module <MOD> Module to verify
      --verify-function <FUNC>   Function to verify
      --json-output <FILE>       Write JSON results to file (default: results.json)
      --no-cache                 Don't cache the verification output
      --with-scip-names [FILE]   Enrich results with scip-names from atoms.json
```

**Caching Workflow:**

```bash
# First run: runs verification and caches output to data/
scip-atoms verify ./my-verus-project -p my-crate

# Subsequent runs: uses cached output (no need to re-run verification)
scip-atoms verify
```

**Examples:**
```bash
# Run verification (caches output automatically)
scip-atoms verify ./my-verus-project
scip-atoms verify ./my-workspace -p my-crate

# Use cached output (no project path needed)
scip-atoms verify

# Analyze existing output file (from CI, etc.)
scip-atoms verify ./my-project --from-file verification_output.txt

# Enrich results with scip-names from atoms.json
scip-atoms verify --with-scip-names
scip-atoms verify --with-scip-names path/to/atoms.json
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
        "scip-name": "scip:crate/1.0.0/module/my_function()",
        "code-path": "src/lib.rs",
        "code-text": { "lines-start": 10, "lines-end": 20 }
      }
    ],
    "verified_functions": [...],
    "unverified_functions": [...]
  }
}
```

Note: `scip-name` is only present when using `--with-scip-names` and uses the URI format `scip:<crate>/<version>/<path>`.

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

