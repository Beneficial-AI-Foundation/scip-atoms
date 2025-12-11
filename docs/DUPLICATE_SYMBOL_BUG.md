# Duplicate SCIP Symbol Bug

## Summary

When multiple trait implementations define the same method name (e.g., `Mul::mul`) for different type combinations, rust-analyzer generates **identical SCIP symbols** for both. This causes scip-atoms to lose one of the implementations when building the call graph, as the second entry overwrites the first in a HashMap.

## Example: Montgomery Point Multiplication

In `curve25519-dalek`, there are two `Mul` trait implementations in `montgomery.rs`:

1. **Line 775**: `impl Mul<&Scalar> for &MontgomeryPoint` - multiply a point by a scalar
2. **Line 844**: `impl Mul<&MontgomeryPoint> for &Scalar` - multiply a scalar by a point

Both implementations have a `mul` method, but they operate on different types.

### Raw SCIP Data

In the SCIP index, both definitions appear as **occurrences** with `symbol_roles: 1` (definition):

```json
// First definition at line 774 (0-based)
{
    "range": [774, 7, 10],
    "symbol": "rust-analyzer cargo curve25519-dalek 4.1.3 montgomery/Mul#mul().",
    "symbol_roles": 1
}

// Second definition at line 843 (0-based)  
{
    "range": [843, 7, 10],
    "symbol": "rust-analyzer cargo curve25519-dalek 4.1.3 montgomery/Mul#mul().",
    "symbol_roles": 1
}
```

**Note:** Both have the **exact same symbol string**.

However, the **symbols array** contains both with distinguishing signature information:

```json
// First symbol entry
{
    "symbol": "rust-analyzer cargo curve25519-dalek 4.1.3 montgomery/Mul#mul().",
    "documentation": ["Given `self` = u_0(P), and a `Scalar` n, return u_0([n]P)"],
    "signature_documentation": {
        "text": "fn mul(self, scalar: &Scalar) -> MontgomeryPoint"
    }
}

// Second symbol entry (same symbol string!)
{
    "symbol": "rust-analyzer cargo curve25519-dalek 4.1.3 montgomery/Mul#mul().",
    "documentation": ["Performs the `*` operation..."],
    "signature_documentation": {
        "text": "fn mul(self, point: &MontgomeryPoint) -> MontgomeryPoint"
    }
}
```

### Current Behavior (Bug)

In `build_call_graph()`, we use `symbol.symbol` as the HashMap key:

```rust
call_graph.insert(
    symbol.symbol.clone(),  // Same key for both!
    FunctionNode { ... },
);
```

Result: Only the second implementation (`Scalar * MontgomeryPoint`) appears in the output. The first (`MontgomeryPoint * Scalar`) is silently lost.

### Evidence in Output

The `curve_final_v2.json` output shows only ONE `Mul::mul` for montgomery.rs:

```json
{
    "code-path": "src/montgomery.rs",
    "code-function": "curve25519_dalek::montgomery::Mul::mul",
    "code-text": {
        "lines-start": 835,
        "lines-end": 847
    }
}
```

The implementation at line 775 is missing entirely.

## Proposed Fix

Use the **signature text** from `signature_documentation.text` to create unique keys and better display names.

### Key Changes

1. **Create unique internal keys** by combining symbol + signature:
   - `montgomery/Mul#mul().|fn mul(self, scalar: &Scalar) -> MontgomeryPoint`
   - `montgomery/Mul#mul().|fn mul(self, point: &MontgomeryPoint) -> MontgomeryPoint`

2. **Generate descriptive code-function names** that include type info:
   - `curve25519_dalek::montgomery::Mul<&Scalar>::mul` (for MontgomeryPoint * Scalar)
   - `curve25519_dalek::montgomery::Mul<&MontgomeryPoint>::mul` (for Scalar * MontgomeryPoint)

### Implementation Strategy

1. Modify `build_call_graph()` to:
   - Track signature text for each symbol
   - Use composite keys that include signature info
   - Store signature text in `FunctionNode` for later use

2. Modify `symbol_to_rust_path()` to:
   - Accept optional signature info
   - Extract type parameters from signature to generate better names

### Expected Output After Fix

```json
{
    "code-path": "src/montgomery.rs", 
    "code-function": "curve25519_dalek::montgomery::Mul<&Scalar>::mul",
    "code-text": {
        "lines-start": 775,
        "lines-end": 807
    }
},
{
    "code-path": "src/montgomery.rs",
    "code-function": "curve25519_dalek::montgomery::Mul<&MontgomeryPoint>::mul", 
    "code-text": {
        "lines-start": 844,
        "lines-end": 856
    }
}
```

## Scope of Impact

This bug affects any crate with multiple trait implementations of the same method:
- `Mul`, `Add`, `Sub`, etc. for different type combinations
- `From`/`Into` implementations
- Any generic trait impl with the same method name

The fix ensures all such implementations are captured in the output.

## Implementation Details

### Level 1: Type Parameter Extraction

For binary ops like `Mul<T>`, extract the type parameter from the second argument:
- `fn mul(self, scalar: &Scalar)` → adds `<&Scalar>` to the trait name

For `From<T>`, extract from the first (and only) parameter:
- `fn from(value: EdwardsPoint)` → adds `<EdwardsPoint>` to the trait name

The `&` is preserved to distinguish `impl From<&T>` from `impl From<T>`.

### Level 2: Self Type Insertion

When rust-analyzer omits the Self type from the symbol path (common for reference Self types),
we extract it from the `self` parameter's signature and insert it:
- `montgomery/Mul#mul()` → `montgomery/&MontgomeryPoint#Mul<&Scalar>#mul()`

### Level 3: Line Number Suffix (Last Resort)

For cases where symbol+signature are identical (e.g., generic impls that differ only in
type parameters not visible in the signature), we add a line number suffix:
- `window/LookupTable#From<&EdwardsPoint>#from()` becomes
- `window/LookupTable#From<&EdwardsPoint>#from()@345` and
- `window/LookupTable#From<&EdwardsPoint>#from()@436`

This handles edge cases like:
```rust
impl<'a> From<&'a EdwardsPoint> for LookupTable<AffineNielsPoint> { ... }  // line 345
impl<'a> From<&'a EdwardsPoint> for LookupTable<ProjectiveNielsPoint> { ... }  // line 436
```

### Duplicate Detection

The tool now includes a `find_duplicate_scip_names()` function that can detect any
remaining duplicates in the output. This is used in `main.rs` to print warnings when
duplicates are detected, helping identify edge cases that may need additional handling.








