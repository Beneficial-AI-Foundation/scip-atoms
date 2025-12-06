# Trait Implementation Symbol Patterns in SCIP

This document explains how rust-analyzer generates SCIP symbols for different trait implementations, and why some cases require special handling in scip-atoms.

## Summary

| Trait Pattern | SCIP Symbol Includes | Duplicate Symbol Risk |
|---------------|---------------------|----------------------|
| `impl Trait for Type` vs `impl Trait for &Type` | Self type | ❌ No - different symbols |
| `impl Trait<A> for B` vs `impl Trait<B> for A` | Only trait name | ✅ Yes - same symbol |

## Case 1: Neg Trait - Different Symbols (No Special Handling Needed)

For unary traits like `Neg`, rust-analyzer generates **different** symbols based on whether Self is a reference or owned type.

### Example: Scalar

```rust
// impl Neg for &Scalar
impl<'a> Neg for &'a Scalar {
    fn neg(self) -> Scalar { ... }
}

// impl Neg for Scalar  
impl Neg for Scalar {
    fn neg(self) -> Scalar { ... }
}
```

### Generated SCIP Symbols

| Implementation | Line | SCIP Symbol |
|----------------|------|-------------|
| `impl Neg for &Scalar` | 816 | `scalar/Neg#neg()` |
| `impl Neg for Scalar` | 881 | `scalar/Scalar#Neg#neg()` |

**Key observation:** The Self type (`&Scalar` vs `Scalar`) affects the symbol path. When Self is an owned type, it gets included in the path (`Scalar#Neg`).

### Same Pattern for Other Types

| Type | `impl Neg for &T` Symbol | `impl Neg for T` Symbol |
|------|--------------------------|------------------------|
| Scalar | `scalar/Neg#neg()` | `scalar/Scalar#Neg#neg()` |
| RistrettoPoint | `ristretto/Neg#neg()` | `ristretto/RistrettoPoint#Neg#neg()` |
| EdwardsPoint | `edwards/Neg#neg()` | `edwards/EdwardsPoint#Neg#neg()` |

## Case 2: Mul Trait - Same Symbols (Requires Special Handling)

For binary traits like `Mul` with type parameters, rust-analyzer generates the **same** symbol when only the type parameter differs.

### Example: MontgomeryPoint

```rust
// MontgomeryPoint * Scalar
impl<'a, 'b> Mul<&'b Scalar> for &'a MontgomeryPoint {
    fn mul(self, scalar: &Scalar) -> MontgomeryPoint { ... }
}

// Scalar * MontgomeryPoint
impl<'a, 'b> Mul<&'b MontgomeryPoint> for &'a Scalar {
    fn mul(self, point: &MontgomeryPoint) -> MontgomeryPoint { ... }
}
```

### Generated SCIP Symbols (Same!)

| Implementation | Line | SCIP Symbol |
|----------------|------|-------------|
| `impl Mul<&Scalar> for &MontgomeryPoint` | 775 | `montgomery/Mul#mul()` |
| `impl Mul<&MontgomeryPoint> for &Scalar` | 844 | `montgomery/Mul#mul()` |

**Problem:** Both implementations have the **exact same symbol string**. The type parameter (`Mul<&Scalar>` vs `Mul<&MontgomeryPoint>`) is not included in the symbol.

### Why This Happens

- Both implementations have reference Self types (`&MontgomeryPoint` and `&Scalar`)
- The symbol path only includes the module (`montgomery`) and trait (`Mul`)
- Type parameters are stripped from the symbol

## scip-atoms Fix

To handle Case 2, scip-atoms uses the `signature_documentation.text` field to distinguish implementations with identical symbols.

### How It Works

1. **Detect duplicates**: When multiple symbols have the same string, use signature text as a secondary key
2. **Create unique keys**: `symbol + "|" + signature_text`
3. **Enhance output**: Include type info in scip_name for disambiguation

### Result

Before fix:
```
montgomery/Mul#mul()  (only one appears, other overwritten)
```

After fix:
```
montgomery/Mul<Scalar>#mul()        (line 775)
montgomery/Mul<MontgomeryPoint>#mul()  (line 844)
```

## Affected Traits

This pattern affects any binary operator trait where:
- Multiple impls exist with different type parameters
- Self types are structurally similar (both references or both owned)

Common examples:
- `Mul<A> for B` vs `Mul<B> for A`
- `Add<A> for B` vs `Add<B> for A`
- `Sub<A> for B` vs `Sub<B> for A`
- `From<A>` for different source types (potentially)

## Testing

See `tests/duplicate_symbols.rs` for tests that verify both patterns are handled correctly:
- `test_duplicate_mul_implementations` - verifies Mul fix works
- `test_scip_names_include_type_info` - verifies enhanced scip_names
- `test_neg_implementations_for_scalar` - verifies Neg works naturally
- `test_neg_implementations_for_ristretto` - verifies Neg works naturally





