# Verification Analysis Architecture

This document describes the architecture of the `scip-atoms verify` command, which analyzes Verus verification output to identify verified and failed functions.

---

## Separation from Call Graph Generation

The `scip-atoms` tool has **two completely independent pipelines**:

| Command | Tool Used | Purpose |
|---------|-----------|---------|
| `scip-atoms atoms` | **verus-analyzer** → SCIP index | Generate call graph (who calls whom) |
| `scip-atoms verify` | **cargo verus** + **verus_syn** | Analyze verification (what passed/failed) |

**verus-analyzer is NOT used in verification analysis.** The verification analysis only:
1. Runs `cargo verus verify` to get verification output
2. Parses source files using `verus_syn` crate (our own `verus_parser` module)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        scip-atoms tool                                   │
├───────────────────────────────┬─────────────────────────────────────────┤
│   atoms command               │   verify command                        │
│   ─────────────               │   ──────────────                        │
│                               │                                         │
│   verus-analyzer              │   cargo verus verify                    │
│        │                      │        │                                │
│        ▼                      │        ▼                                │
│   index.scip                  │   verification output (text)            │
│        │                      │        │                                │
│        ▼                      │        ▼                                │
│   SCIP parsing                │   regex parsing                         │
│   + verus_syn spans           │   + verus_syn spans                     │
│        │                      │        │                                │
│        ▼                      │        ▼                                │
│   atoms.json                  │   results.json                          │
│   (call graph)                │   (verified/failed functions)           │
└───────────────────────────────┴─────────────────────────────────────────┘
```

Both pipelines share `verus_syn` for parsing function spans, but that's the only overlap.

---

## Function Categorization

The verification analysis categorizes functions into three groups:

| Category | Criteria | Meaning |
|----------|----------|---------|
| **verified** | Has `requires`/`ensures` + no errors + no `assume()`/`admit()` | Proven correct by Verus |
| **failed** | Has `requires`/`ensures` + had verification errors | Verification attempted but failed |
| **unverified** | Has `requires`/`ensures` + contains `assume()`/`admit()` | Contains trusted assumptions |

### What's Included

Functions with specifications that have verifiable bodies:
- `fn` with `requires` and/or `ensures`
- `proof fn` with `requires` and/or `ensures`
- `exec fn` with `requires` and/or `ensures`

### What's Excluded

- **`spec fn`**: Pure specifications with no body to verify
- **Functions without specs**: No `requires` or `ensures` (nothing to verify)

### Detection Methods

| Property | How Detected |
|----------|--------------|
| `has_requires` | `sig.spec.requires.is_some()` via `verus_syn` AST |
| `has_ensures` | `sig.spec.ensures.is_some()` via `verus_syn` AST |
| `has_trusted_assumption` | Text search for `assume(` or `admit(` in function body |

---

## Overview (Verification Pipeline Only)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           scip-atoms verify                              │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│   ┌─────────────┐     ┌──────────────────────────────────────────────┐  │
│   │ VerusRunner │────▶│          Verification Output (text)          │  │
│   └─────────────┘     └──────────────────────────────────────────────┘  │
│         OR                              │                                │
│   ┌─────────────┐                       │                                │
│   │ --from-file │───────────────────────┘                                │
│   └─────────────┘                                                        │
│                                         ▼                                │
│                              ┌─────────────────────┐                     │
│                              │ VerificationAnalyzer│                     │
│                              └─────────────────────┘                     │
│                                         │                                │
│              ┌──────────────────────────┼──────────────────────────┐     │
│              ▼                          ▼                          ▼     │
│   ┌─────────────────────┐  ┌─────────────────────┐  ┌──────────────────┐ │
│   │CompilationErrorParser│  │VerificationParser  │  │  verus_parser    │ │
│   └─────────────────────┘  └─────────────────────┘  └──────────────────┘ │
│              │                          │                          │     │
│              ▼                          ▼                          ▼     │
│   ┌─────────────────────┐  ┌─────────────────────┐  ┌──────────────────┐ │
│   │ Compilation Errors  │  │ Error Locations     │  │ All Functions    │ │
│   │ & Warnings          │  │ (file, line)        │  │ with Spans       │ │
│   └─────────────────────┘  └─────────────────────┘  └──────────────────┘ │
│                                         │                          │     │
│                                         └──────────┬───────────────┘     │
│                                                    ▼                     │
│                                         ┌─────────────────────┐          │
│                                         │  Error → Function   │          │
│                                         │     Mapping         │          │
│                                         └─────────────────────┘          │
│                                                    │                     │
│                                                    ▼                     │
│                                         ┌─────────────────────┐          │
│                                         │   Categorize:       │          │
│                                         │   • verified        │          │
│                                         │   • failed          │          │
│                                         │   • unverified      │          │
│                                         └─────────────────────┘          │
│                                                    │                     │
│                                                    ▼                     │
│                                         ┌─────────────────────┐          │
│                                         │   AnalysisResult    │          │
│                                         │  (results.json)     │          │
│                                         └─────────────────────┘          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Components

### 1. VerusRunner

**Purpose:** Run `cargo verus verify` and capture output.

**Location:** `verification.rs`

**Flow:**
```
1. Setup environment variables (BORING_BSSL_PATH, etc.)
2. Build command: cargo verus verify [-p package] [-- --verify-only-module X]
3. Execute and capture stdout + stderr
4. Return (output_text, exit_code)
```

**Potential Simplifications:**
- Environment setup could be optional/configurable
- Could stream output instead of buffering

---

### 2. CompilationErrorParser

**Purpose:** Parse rustc/cargo error output to extract compilation errors and warnings.

**Location:** `verification.rs`

**Patterns matched:**
| Pattern | Example |
|---------|---------|
| `error[E0123]: message` | Standard rustc error |
| `error: could not compile` | Cargo build failure |
| `--> file.rs:10:5` | File location |
| `warning: message` | Compiler warning |

**Data flow:**
```
verification_output (text)
         │
         ▼
┌─────────────────────────┐
│ Line-by-line parsing    │
│ using regex patterns    │
└─────────────────────────┘
         │
         ▼
(Vec<CompilationError>, Vec<CompilationError>)
     errors              warnings
```

**Issues/Observations:**
- ⚠️ **10+ regex patterns** compiled at construction time
- ⚠️ **Two-pass parsing:** First checks for verification results, then parses
- ⚠️ **Verification errors filtered out** to avoid double-counting with VerificationParser

---

### 3. VerificationParser

**Purpose:** Parse Verus-specific verification errors and map them to source locations.

**Location:** `verification.rs`

**Verification error types detected:**
- `assertion failed`
- `postcondition not satisfied`
- `precondition not satisfied`
- `loop invariant not preserved`
- `loop invariant not satisfied on entry`
- `assertion not satisfied`

**Key methods:**

#### `parse_verification_output_from_content`
```
Output: HashMap<String, Vec<i32>>  // file_path → [error_line_numbers]
```
- Scans for `--> file:line:col` patterns
- Looks backward to verify it's a real error (not a timing note)

#### `parse_verification_failures`
```
Output: Vec<VerificationFailure>  // Detailed error info
```
- Extracts full error context (15 lines)
- Cleans ANSI escape codes
- Extracts assertion details

#### `find_function_at_line`
```
Input:  (file_path, line_number, all_functions_with_lines)
Output: Option<String>  // function name
```
- **File matching:** exact → suffix → filename-only (priority order)
- **Function matching:** finds function whose span contains the line

**Issues/Observations:**
- ⚠️ **Duplicated work:** Both `parse_verification_output_from_content` and `parse_verification_failures` parse the same output
- ⚠️ **Backward lookback:** Scans up to 10 lines back to verify error context

---

### 4. verus_parser Module

**Purpose:** Parse Rust/Verus source files to extract function definitions and their spans.

**Location:** `verus_parser.rs`

**Key function:**
```rust
parse_all_functions(path, include_verus_constructs, include_methods, show_visibility, show_kind)
    → ParsedOutput { functions, functions_by_file, summary }
```

**What it parses:**
- Regular `fn` definitions
- `spec fn`, `proof fn`, `exec fn` (Verus-specific)
- Impl methods and trait methods
- Functions inside `verus! { }` macro blocks
- Functions inside `cfg_if! { }` macro blocks

**Uses:** `verus_syn` crate with AST visitor pattern.

**Issues/Observations:**
- ✅ Single-pass AST parsing is efficient
- ⚠️ Parses **all** source files even if only analyzing one function
- ⚠️ No caching between runs

---

### 5. VerificationAnalyzer (Orchestrator)

**Purpose:** Coordinate all parsers and produce final analysis.

**Location:** `verification.rs`

**Data flow in `analyze_output`:**

```
Step 1: Parse compilation output
─────────────────────────────────
verification_output ──▶ CompilationErrorParser
                              │
                              ▼
                    (errors, warnings)


Step 2: Parse all project functions  
────────────────────────────────────
project_path ──▶ verus_parser::parse_all_functions
                              │
                              ▼
                    ParsedOutput { functions: [...] }


Step 3: Build lookup structures
───────────────────────────────
ParsedOutput ──▶ Build 3 data structures:
                    │
                    ├─▶ function_locations: HashMap<name, Vec<FunctionLocation>>
                    │      (for building output)
                    │
                    ├─▶ all_functions_with_lines: HashMap<file, Vec<(name, line)>>
                    │      (for error→function mapping)
                    │
                    └─▶ all_function_names: HashSet<String>
                           (for tracking verified vs failed)


Step 4: Parse verification errors
─────────────────────────────────
verification_output ──▶ VerificationParser
                              │
                              ├─▶ errors_by_file: HashMap<file, Vec<line>>
                              │
                              └─▶ verification_failures: Vec<VerificationFailure>


Step 5: Map errors to functions
───────────────────────────────
For each (file, line) in errors:
    │
    ▼
find_function_at_line(file, line, all_functions_with_lines)
    │
    ▼
Mark function as failed, record specific location


Step 6: Build output
────────────────────
- failed_functions: FunctionLocation objects for failed
- verified_functions: All functions NOT in failed set
- Apply module/function filters if provided
```

---

## Current Inefficiencies

### 1. Redundant Parsing of Verification Output

```
verification_output is parsed 3 times:
  1. CompilationErrorParser.parse_compilation_output()  
  2. VerificationParser.parse_verification_output_from_content()
  3. VerificationParser.parse_verification_failures()
```

**Potential fix:** Single-pass parser that extracts all three outputs.

### 2. Multiple Data Structure Builds

```
ParsedOutput.functions is iterated 3 times to build:
  1. function_locations HashMap
  2. all_functions_with_lines HashMap  
  3. all_function_names HashSet
```

**Potential fix:** Single iteration building all three.

### 3. Duplicate Error→Function Mapping

```
Errors are mapped to functions twice:
  1. From errors_by_file (line 778-803)
  2. From verification_failures (line 807-833)

Both use find_function_at_line and do similar location matching.
```

**Potential fix:** Unify into single loop, dedup by (file, line).

### 4. Full Project Parsing

```
verus_parser::parse_all_functions parses entire project
even when --verify-only-module or --verify-function is specified.
```

**Potential fix:** Filter files before parsing based on module filter.

### 5. Regex Compilation on Every Run

```
CompilationErrorParser and VerificationParser compile 15+ regexes
on every instantiation.
```

**Potential fix:** Use `lazy_static` or `once_cell` for regex patterns.

---

## Proposed Simplified Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Simplified Analysis                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│   ┌─────────────────────┐         ┌─────────────────────┐               │
│   │  Verification       │         │  Source Files       │               │
│   │  Output (text)      │         │  (*.rs)             │               │
│   └─────────────────────┘         └─────────────────────┘               │
│              │                              │                            │
│              ▼                              ▼                            │
│   ┌─────────────────────┐         ┌─────────────────────┐               │
│   │  UnifiedOutputParser│         │  FunctionIndexer    │               │
│   │  (single pass)      │         │  (with caching)     │               │
│   └─────────────────────┘         └─────────────────────┘               │
│              │                              │                            │
│              │  errors: Vec<ErrorInfo>      │  functions: Vec<FnInfo>    │
│              │    - type (compile/verify)   │    - name, file, span      │
│              │    - file, line, message     │                            │
│              │                              │                            │
│              └──────────────┬───────────────┘                            │
│                             ▼                                            │
│                  ┌─────────────────────┐                                 │
│                  │   Error Mapper      │                                 │
│                  │ (binary search by   │                                 │
│                  │  file + line range) │                                 │
│                  └─────────────────────┘                                 │
│                             │                                            │
│                             ▼                                            │
│                  ┌─────────────────────┐                                 │
│                  │   AnalysisResult    │                                 │
│                  └─────────────────────┘                                 │
└─────────────────────────────────────────────────────────────────────────┘
```

### Key Improvements:

1. **UnifiedOutputParser:** Single pass extracts both compilation and verification errors
2. **FunctionIndexer:** 
   - Indexes functions by file for O(log n) lookup
   - Can cache results between runs
   - Lazy parsing (only parse files mentioned in errors)
3. **Error Mapper:** Binary search on sorted function spans instead of linear scan

---

## Metrics to Track

For optimization, we should measure:

| Metric | Current | After Optimization |
|--------|---------|-------------------|
| Time parsing verification output | ? ms | ? ms |
| Time parsing source files | ? ms | ? ms |
| Time mapping errors to functions | ? ms | ? ms |
| Memory for function index | ? MB | ? MB |
| Number of regex compilations | ~15 | 0 (cached) |
| Number of output parsing passes | 3 | 1 |

---

## Next Steps

1. **Measure current performance** on a large project
2. **Profile** to identify actual bottlenecks
3. **Implement quick wins:**
   - Merge the 3 loops over `parsed_output.functions`
   - Cache compiled regexes with `lazy_static`
4. **Consider larger refactors** if needed:
   - Single-pass output parser
   - Lazy/filtered source file parsing
