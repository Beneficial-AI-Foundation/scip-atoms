# How It Works

This document explains the internals of scip-atoms and how each command works.

---

## SCIP-based Call Graph (`atoms` command)

The `atoms` command generates a call graph from SCIP (Source Code Index Protocol) data.

### Pipeline

1. **Check for cached SCIP data** in `<project_path>/data/`
2. If not found (or `--regenerate-scip` is used):
   - Run `verus-analyzer scip` to generate a binary SCIP index
   - Convert the binary index to JSON using `scip print --json`
   - Cache both files in `<project_path>/data/` for future runs
3. Parse the SCIP JSON to extract:
   - Function definitions and their symbols
   - Call relationships between functions
   - Type information for disambiguation
4. **Parse source files with `verus_syn`** to get accurate function body spans
5. Output compact JSON with line number ranges instead of full code

### SCIP Data Caching

To speed up subsequent runs, the tool caches generated SCIP data:

```
<project_path>/
└── data/
    ├── index.scip       # Binary SCIP index
    └── index.scip.json  # JSON conversion (used by the tool)
```

**Benefits:**
- Subsequent runs skip the slow `verus-analyzer scip` and `scip print --json` steps
- When using cached data, `verus-analyzer` and `scip` tools don't need to be installed
- Use `--regenerate-scip` when your source code changes and you need fresh data

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

### Disambiguation of Trait Implementations

A key challenge is disambiguating trait implementations that share the same symbol. For example:

```rust
impl Mul<Scalar> for Point { fn mul(...) }
impl Mul<Point> for Scalar { fn mul(...) }
```

Both produce similar SCIP symbols. The tool uses multiple strategies:
1. **Signature text** - extracts type parameters from function signatures
2. **Self type** - extracts the implementing type from `self` parameter
3. **Definition type context** - looks at nearby type references
4. **Line number fallback** - uses line numbers when types can't disambiguate

---

## Function Parsing (`functions` command)

The `functions` command uses `verus_syn` to parse Rust/Verus source files directly, without needing SCIP data.

### What it parses

- Regular Rust functions (`fn`)
- Verus-specific functions (`spec fn`, `proof fn`, `exec fn`)
- Trait methods
- Impl methods
- Functions inside `verus! { }` macro blocks
- Functions inside `cfg_if! { }` macro blocks

### Visitor pattern

The parser uses the visitor pattern from `verus_syn::visit::Visit`:

```rust
impl<'ast> Visit<'ast> for FunctionVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) { ... }
    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) { ... }
    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) { ... }
    fn visit_item_macro(&mut self, node: &'ast ItemMacro) { ... }
}
```

### Metadata extraction

When `--show-visibility` or `--show-kind` are enabled:

- **Visibility**: `pub`, `pub(crate)`, `pub(super)`, or `private`
- **Kind**: `fn`, `spec fn`, `proof fn`, `exec fn`, `const fn`, etc.
- **Context**: `standalone`, `impl`, or `trait`

---

## Verification Analysis (`verify` command)

The `verify` command can either run Verus verification or analyze existing output (with `--from-file`).
It parses Verus/Cargo output to extract structured information.

### Compilation Error Parsing

Parses rustc/cargo error output to extract:

- Error messages and their locations
- Warning messages
- Process failure errors
- Memory allocation errors

Patterns matched:
- `error[E0123]: message` - standard rustc errors
- `error: could not compile` - cargo build failures
- `error: process didn't exit successfully` - subprocess failures
- `--> file.rs:10:5` - file location annotations

### Verification Error Parsing

Parses Verus-specific verification output:

- `error: assertion failed`
- `error: postcondition not satisfied`
- `error: precondition not satisfied`
- `error: loop invariant not preserved`
- `error: loop invariant not satisfied on entry`
- `error: assertion not satisfied`

### Function-to-Error Mapping

The tool maps errors to specific functions:

1. Parse all functions in the project with their line ranges
2. For each error location (file, line), find the enclosing function
3. Mark that function as "failed"
4. All other functions are marked as "verified"

### Status Determination

The analysis determines an overall status:

| Condition | Status |
|-----------|--------|
| `verification results:: N verified, 0 errors` present | `success` |
| `verification results:: N verified, M errors` (M > 0) | `verification_failed` |
| Compilation errors present | `compilation_failed` |
| Non-zero exit code with no other indicators | `compilation_failed` |

---

## Output Formats

### Atoms JSON Format

```json
{
  "display-name": "my_function",
  "scip-name": "curve25519-dalek 4.1.3 module/my_function()",
  "dependencies": ["..."],
  "code-path": "src/lib.rs",
  "code-text": {
    "lines-start": 42,
    "lines-end": 100
  }
}
```

### Analysis JSON Format

The verification analysis output format is aligned with `atoms.json` for consistency:

```json
{
  "status": "success|verification_failed|compilation_failed",
  "summary": {
    "total_functions": 42,
    "verified_functions": 40,
    "failed_functions": 2,
    "compilation_errors": 0,
    "compilation_warnings": 1,
    "verification_errors": 2
  },
  "compilation": {
    "errors": [...],
    "warnings": [...]
  },
  "verification": {
    "verified_functions": [
      {
        "display-name": "my_function",
        "code-path": "src/lib.rs",
        "code-text": { "lines-start": 10, "lines-end": 25 }
      }
    ],
    "failed_functions": [
      {
        "display-name": "failing_function",
        "code-path": "src/other.rs",
        "code-text": { "lines-start": 30, "lines-end": 45 }
      }
    ],
    "errors": [...]
  }
}
```

Note: `verified_functions` and `failed_functions` use the same format as atoms.json entries (`display-name`, `code-path`, `code-text`).
```
