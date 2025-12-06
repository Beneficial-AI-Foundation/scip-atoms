# SCIP Symbol Format Comparison: rust-analyzer vs verus-analyzer

This document compares how rust-analyzer and verus-analyzer generate SCIP symbols for trait implementations, specifically focusing on the handling of owned vs reference `Self` types.

## Summary

| Tool | Symbol Format | Self Type Handling | Uniqueness |
|------|--------------|-------------------|------------|
| **rust-analyzer** | `impl#[SelfType][Trait]method()` | ✅ Always included | ✅ Unique symbols |
| **verus-analyzer** | `Type#Trait#method()` or `Trait#method()` | ❌ Missing for references | ❌ Can have duplicates |

## rust-analyzer Symbol Format (Consistent ✅)

rust-analyzer uses a structured format that **always includes the Self type**:

```
module/impl#[SelfType][Trait]method()
```

### Examples from curve25519-dalek (rust-analyzer)

Data source: `data/curve_ra.json`

| Implementation | SCIP Symbol | Line in curve_ra.json |
|---------------|-------------|----------------------|
| `impl Neg for Scalar` | `scalar/impl#[Scalar][Neg]neg().` | 274983, 296384 |
| `impl<'a> Neg for &'a Scalar` | `` scalar/impl#[`&'a Scalar`][Neg]neg(). `` | 274691, 296325 |
| `impl ConstantTimeEq for MontgomeryPoint` | `montgomery/impl#[MontgomeryPoint][ConstantTimeEq]ct_eq().` | 260678, 269316 |
| `impl Mul<&Scalar> for &MontgomeryPoint` | `` montgomery/impl#[`&MontgomeryPoint`][`Mul<&Scalar>`]mul(). `` | 265512, 270421 |
| `impl Mul<&MontgomeryPoint> for &Scalar` | `` montgomery/impl#[`&Scalar`][`Mul<&MontgomeryPoint>`]mul(). `` | 265833, 270504 |

**Key observations:**
- Self type is always in the first `[]` bracket
- Trait (with type parameters) is in the second `[]` bracket
- Reference types are preserved with backticks (e.g., `` `&'a Scalar` ``, `` `&MontgomeryPoint` ``)
- All symbols are unique, even for multiple `Mul` implementations

## verus-analyzer Symbol Format (Inconsistent ❌)

verus-analyzer uses a different format that **varies based on whether Self is owned or a reference**:

| Self Type | Symbol Format |
|-----------|--------------|
| **Owned** (`impl Trait for Type`) | `module/Type#Trait#method()` |
| **Reference** (`impl Trait for &Type`) | `module/Trait#method()` |

### Examples from curve25519-dalek (verus-analyzer)

Data source: `data/curve_top.json`

| Implementation | SCIP Symbol | Line in curve_top.json | Issue |
|---------------|-------------|------------------------|-------|
| `impl Neg for Scalar` | `scalar/Scalar#Neg#neg().` | 27082, 44953 | ✅ OK |
| `impl<'a> Neg for &'a Scalar` | `scalar/Neg#neg().` | 26556, 44810 | ❌ Self type missing |
| `impl ConstantTimeEq for MontgomeryPoint` | `montgomery/MontgomeryPoint#ConstantTimeEq#ct_eq().` | 516620, 523357 | ✅ OK |
| `impl Mul<&Scalar> for &MontgomeryPoint` | `montgomery/Mul#mul().` | 522639, 524833 | ❌ Self type missing |
| `impl Mul<&MontgomeryPoint> for &Scalar` | `montgomery/Mul#mul().` | 523205 | ❌ DUPLICATE! |

**Note:** Lines 522639 and 523205 show that two different `Mul` implementations produce the **identical symbol** `montgomery/Mul#mul().`

## Impact of verus-analyzer's Inconsistency

### 1. Lost Semantic Information

When `Self` is a reference type, the implementor type is omitted from the symbol path. This makes it impossible to determine which type implements the trait from the symbol alone.

### 2. Duplicate Symbols

Multiple implementations can produce **identical symbols**:

```
impl Mul<&Scalar> for &MontgomeryPoint  →  montgomery/Mul#mul()  (line 522639)
impl Mul<&MontgomeryPoint> for &Scalar  →  montgomery/Mul#mul()  (line 523205) SAME!
```

This breaks tools that expect unique symbols for distinct implementations.

### 3. Unpredictable Paths

SCIP consumers cannot reliably construct or predict symbol paths without knowing whether the implementation uses an owned or reference Self type.

## Fix in scip-atoms

The scip-atoms tool **repairs** verus-analyzer's inconsistent symbols by:

1. **Disambiguation**: Using `signature_documentation.text` as a secondary key to distinguish implementations with identical symbols
2. **Self type insertion**: Extracting the Self type from `method().(self)` parameter symbols and inserting it into the symbol path

```rust
// Create unique key using signature to handle duplicate symbols
let unique_key = make_unique_key(&symbol.symbol, signature);

// Extract self_type from the self parameter's signature (preserving &)
let self_type = extract_self_type("self: &MontgomeryPoint"); // -> "&MontgomeryPoint"
let self_type = extract_self_type("self: Scalar");           // -> "Scalar"

// Insert self_type into symbols missing it
// "montgomery/Mul<Scalar>#mul()" -> "montgomery/&MontgomeryPoint#Mul<Scalar>#mul()"
```

### Result after repair

| Original verus-analyzer | Repaired scip-atoms | rust-analyzer equivalent |
|------------------------|---------------------|-------------------------|
| `montgomery/Mul#mul().` | `montgomery/&MontgomeryPoint#Mul<Scalar>#mul()` | `` montgomery/impl#[`&MontgomeryPoint`][`Mul<&Scalar>`]mul(). `` |
| `montgomery/Mul#mul().` (duplicate!) | `montgomery/&Scalar#Mul<MontgomeryPoint>#mul()` | `` montgomery/impl#[`&Scalar`][`Mul<&MontgomeryPoint>`]mul(). `` |
| `scalar/Neg#neg().` | `scalar/&Scalar#Neg#neg()` | `` scalar/impl#[`&'a Scalar`][Neg]neg(). `` |
| `scalar/Scalar#Neg#neg().` | `scalar/Scalar#Neg#neg()` | `scalar/impl#[Scalar][Neg]neg().` |

This produces **unique symbols** that distinguish owned vs reference implementations, consistent with rust-analyzer's approach!

## Data Sources

- **rust-analyzer data**: `data/curve_ra.json` - generated from upstream [curve25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek)
- **verus-analyzer data**: `data/curve_top.json` - generated from curve25519-dalek with Verus annotations

## Conclusion

The symbol naming inconsistency is specific to **verus-analyzer**, not rust-analyzer. rust-analyzer's newer format (`impl#[Type][Trait]method()`) is consistent and produces unique symbols for all trait implementations.

**scip-atoms now repairs verus-analyzer's symbols** to be consistent with rust-analyzer's format by:
1. Extracting Self type from the `method().(self)` parameter symbol
2. Inserting the Self type into symbols that are missing it
3. Adding trait type parameters for disambiguation

This produces output like `montgomery/MontgomeryPoint#Mul<Scalar>#mul()` which is unique and includes all semantic information.

### Alternative approaches
1. Using rust-analyzer instead of verus-analyzer for SCIP generation
2. Filing an issue with verus-analyzer to adopt the newer symbol format (see `docs/VERUS_ANALYZER_ISSUE_DRAFT.md`)

## Related

- [rust-analyzer issue #18772](https://github.com/rust-lang/rust-analyzer/issues/18772) - SCIP symbols for inherent `impl` declarations are ambiguous
- `docs/TRAIT_IMPL_SYMBOL_PATTERNS.md` - Detailed patterns for trait impl symbols in verus-analyzer
- `docs/DUPLICATE_SYMBOL_BUG.md` - Documentation of the duplicate symbol issue
- `docs/VERUS_ANALYZER_ISSUE_DRAFT.md` - Draft issue for verus-analyzer
