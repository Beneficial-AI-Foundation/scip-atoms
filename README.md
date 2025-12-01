# scip-atoms

Generates a JSON file of functions with their dependencies and line number ranges from SCIP indexes.

## Installation

```bash
cargo install --path .
```

### Prerequisites

Install [verus-analyzer](https://github.com/verus-lang/verus-analyzer) and [scip](https://github.com/sourcegraph/scip/).
For convenience, one can use the bellow scripts.
```bash
# Install using Python scripts (recommended)
git clone https://github.com/Beneficial-AI-Foundation/installers_for_various_tools
cd installers_for_various_tools
python3 verus_analyzer_installer.py
python3 scip_installer.py
```

## Usage

```bash
scip-atoms <project_path> <output_json>
```

Example:
```bash
scip-atoms ./my-rust-project output.json
```

## How It Works

1. Runs `verus-analyzer scip` on your project to generate a SCIP index
2. Converts the binary index to JSON using `scip print --json`
3. Parses the SCIP data to extract functions and their call dependencies
4. **Parses source files with `verus_syn`** to get accurate function body spans
5. Outputs compact JSON with line number ranges instead of full code

### Accurate Line Spans with verus_syn

SCIP indexes only provide the location of function **names**, not their full body spans. To get accurate `lines-start` and `lines-end` values, this tool uses [`verus_syn`](https://crates.io/crates/verus_syn) to parse the actual source files.

**Why verus_syn?**
- Standard `syn` doesn't understand Verus-specific syntax
- `verus_syn` handles `verus! { }` macro blocks which contain most Verus function definitions
- It correctly parses `proof fn`, `spec fn`, and other Verus constructs

**How it works:**
1. Parse each source file into an AST using `verus_syn::parse_file`
2. Visit all function definitions (including those inside `verus!` blocks)
3. Extract the full span (start line to end line) of each function
4. Match parsed functions to SCIP symbols by name and approximate line number

This achieves **~95% accuracy** for function spans in typical Verus projects.

## Output Format

```json
[
  {
    "display-name": "my_function",
    "visible": true,
    "dependencies": {
      "module::dependency_fn": { "visible": true }
    },
    "code-path": "src/lib.rs",
    "code-function": "crate::module::my_function",
    "code-text": {
      "lines-start": 42,
      "lines-end": 100
    }
  }
]
```

**Fields:**
- `display-name`: Function name
- `visible`: Always `true` (for visualization tool compatibility)
- `dependencies`: Map of called functions (both project and external)
- `code-path`: Source file path
- `code-function`: Fully qualified function path
- `code-text`: Line range where function is defined (1-based)

### Function Entries vs Dependencies

- **Entries**: Only functions **defined in your project** appear as top-level entries
- **Dependencies**: Both project functions AND external functions (from std, other crates) are tracked

This means you get a complete picture of what each function calls, while keeping the output focused on your project's code.

## Example Output

```
═══════════════════════════════════════════════════════════
  SCIP Atoms - Generate Compact Call Graph Data
═══════════════════════════════════════════════════════════

Checking prerequisites...
  ✓ verus-analyzer found
  ✓ scip found
  ✓ Valid Rust project found

Step 1/4: Running verus-analyzer scip on ./my-project...
  ✓ SCIP index generated successfully

Step 2/4: Converting index.scip to JSON...
  ✓ SCIP JSON generated

Step 3/4: Parsing SCIP JSON and building call graph...
  ✓ Call graph built with 355 functions

Step 4/4: Converting to atoms format with accurate line numbers...
  Parsing source files with verus_syn for accurate function spans...
  ✓ Converted 355 functions to atoms format

═══════════════════════════════════════════════════════════
  ✓ SUCCESS
═══════════════════════════════════════════════════════════

Output written to: output.json

Summary:
  - Total functions: 355
  - Total dependencies: 433
  - Output format: atoms with line numbers and visibility flags

Cleaned up temporary file: index.scip.json
```

## License

MIT


