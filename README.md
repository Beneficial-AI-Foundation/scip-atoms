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
4. Outputs compact JSON with line number ranges instead of full code

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
- `dependencies`: Map of called functions
- `code-path`: Source file path
- `code-function`: Fully qualified function path
- `code-text`: Line range where function is defined (1-based)

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

Step 4/4: Converting to atoms format with line numbers...
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
