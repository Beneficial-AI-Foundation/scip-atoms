# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

scip-atoms is a Rust CLI tool that generates compact function call graph data from SCIP (Source Code Index Protocol) indexes and analyzes Verus verification results. It has four subcommands:
- **atoms**: Generate SCIP-based call graphs with accurate line numbers
- **functions**: List all functions in a Rust/Verus project (no external tools needed)
- **verify**: Run Verus verification and analyze results
- **specify**: Extract function specifications from atoms.json

## Build and Test Commands

```bash
# Build
cargo build                    # Debug build
cargo build --release          # Optimized release build
cargo install --path .         # Install locally

# Test
cargo test                     # All tests
cargo test --lib --verbose     # Unit tests only
cargo test --test duplicate_symbols --verbose    # Integration test
cargo test --test function_coverage --verbose -- --nocapture

# Code quality (all enforced in CI)
cargo fmt --all                # Format code
cargo clippy --all-targets -- -D warnings  # Lint (no warnings allowed)

# Development workflow
cargo fmt && cargo clippy --all-targets && cargo test
```

## Project Structure

```
src/
├── main.rs           # CLI entry point with subcommand routing
├── lib.rs            # Core data structures and SCIP JSON parsing
├── verification.rs   # Verification output parsing & analysis
└── verus_parser.rs   # AST parsing using verus_syn for function spans
```

## Architecture

### Three Main Pipelines

1. **Atoms Pipeline** (`atoms` command): SCIP JSON → call graph parsing → spans via verus_syn → JSON output
2. **Functions Pipeline** (`functions` command): Source files → AST visitor → function list
3. **Verification Pipeline** (`verify` command): Cargo verus output → error parsing → function mapping → analysis

### Key Architectural Patterns

**Accurate Line Spans**: SCIP only provides function name locations. Uses `verus_syn` AST visitor to parse actual function body spans (~95% accuracy). Handles Verus-specific syntax (`verus!{}` blocks, `spec fn`, `proof fn`).

**Interval Trees for Performance**: Error-to-function mapping uses `rust-lapper` for O(log n) lookups instead of linear scans.

**Trait Implementation Disambiguation**: Multiple strategies to resolve SCIP symbol conflicts for trait impls: signature text extraction, self type from parameters, definition type context, line number fallback.

**SCIP Data Caching**: Generated SCIP data is cached in `<project>/data/` to avoid re-running slow external tools.

### Key Types

- `FunctionNode`: Call graph node with callees and type context
- `AtomWithLines`: Output format with line ranges
- `FunctionInterval`: Interval tree entry for error→function mapping
- `CompilationError`, `VerificationFailure`: Error types for verification analysis

## External Tool Dependencies

- **atoms command**: Requires `verus-analyzer` and `scip` CLI
- **functions command**: None (uses verus_syn only)
- **verify command**: Requires `cargo verus`

## Before Committing

Always run fmt and clippy before committing and pushing:

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings
```

## Commit Message Style

Use conventional commits: `feat(module):`, `fix(module):`, `perf(module):`, `refactor(module):`

Examples from history:
- `feat(specify): output dictionary keyed by scip-name from atoms.json`
- `fix(verification): update atoms.json reader for new schema`
- `perf(verify): use interval tree for error-to-function mapping`
