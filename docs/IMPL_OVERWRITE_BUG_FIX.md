# Fix: Trait Impl Overwrite Bug

## Summary

Multiple trait implementations of the form `impl X<A, B> for Y` were overwriting each other in the call graph because they shared the same SCIP symbol and signature. This fix ensures all implementations are captured by using a 4-component unique key.

## The Bug

When rust-analyzer/verus-analyzer generates SCIP symbols for trait implementations, it often produces **identical symbols** for different implementations. For example:

```rust
// These two impls have the SAME SCIP symbol: `ristretto/Mul#mul().`
impl<'a, 'b> Mul<&'b Scalar> for &'a RistrettoPoint { ... }
impl<'a, 'b> Mul<&'b Scalar> for &'a RistrettoBasepointTable { ... }
```

Both produce:
- **Symbol**: `rust-analyzer cargo curve25519-dalek 4.1.3 ristretto/Mul#mul().`
- **Signature**: `fn mul(self, scalar: &'b Scalar) -> RistrettoPoint`

When building the call graph, we used `symbol|signature` as the HashMap key. Since both entries had the same key, the second one would **overwrite** the first.

## The Fix

We now use a **4-component unique key**:

```
symbol | signature | self_type | line_number
```

### Component Breakdown

| Component | Purpose | Example |
|-----------|---------|---------|
| `symbol` | Base SCIP symbol | `ristretto/Mul#mul().` |
| `signature` | Function signature text | `fn mul(self, scalar: &'b Scalar) -> RistrettoPoint` |
| `self_type` | The Self type from `self` parameter | `&RistrettoPoint` or `&RistrettoBasepointTable` |
| `line_number` | Definition line (fallback) | `932` or `1087` |

### How Each Component Helps

**Level 1: Different Signature**
```rust
impl Mul<&Scalar> for &Point { fn mul(self, s: &Scalar) -> Point }
impl Mul<&Point> for &Scalar { fn mul(self, p: &Point) -> Point }
//                                         ^^^^^^^^^ different param type
```
→ Distinguished by `signature`

**Level 2: Different Self Type**
```rust
impl Mul<&Scalar> for &RistrettoPoint { fn mul(self, s: &Scalar) -> RistrettoPoint }
impl Mul<&Scalar> for &RistrettoBasepointTable { fn mul(self, s: &Scalar) -> RistrettoPoint }
//                    ^^^^^^^^^^^^^^^^^^^^^^^^ different Self type
```
→ Distinguished by `self_type` (extracted from `self: &RistrettoPoint` vs `self: &RistrettoBasepointTable`)

**Level 3: Different Line (Fallback)**
```rust
// Hypothetical edge case: trait type param doesn't appear in method signature
impl<T> Marker<A> for X { fn mark(self) {} }  // line 10
impl<T> Marker<B> for X { fn mark(self) {} }  // line 20
```
→ Distinguished by `line_number`

## Real-World Impact

### Before Fix (atoms_old.json)
```
ristretto Mul#mul entries: 5 total
  Line  923: ristretto/RistrettoPoint#MulAssign<'b Scalar>#mul_assign()
  Line  941: ristretto/&Scalar#Mul<'b RistrettoPoint>#mul()
  Line  980: ristretto/RistrettoPoint#MultiscalarMul<J>#multiscalar_mul()
  Line 1087: ristretto/&RistrettoBasepointTable#Mul<'b Scalar>#mul()
  Line 1096: ristretto/&Scalar#Mul<RistrettoBasepointTable>#mul()
```

### After Fix (atoms.json)
```
ristretto Mul#mul entries: 6 total (+1 recovered!)
  Line  923: ristretto/RistrettoPoint#MulAssign<'b Scalar>#mul_assign()
  Line  932: ristretto/&RistrettoPoint#Mul<'b Scalar>#mul()  ← RECOVERED!
  Line  941: ristretto/&Scalar#Mul<'b RistrettoPoint>#mul()
  Line  980: ristretto/RistrettoPoint#MultiscalarMul<J>#multiscalar_mul()
  Line 1087: ristretto/&RistrettoBasepointTable#Mul<'b Scalar>#mul()
  Line 1096: ristretto/&Scalar#Mul<RistrettoBasepointTable>#mul()
```

### Total Impact
- **OLD**: 986 atoms
- **NEW**: 988 atoms (+2)
- **Recovered entries**:
  - `edwards/&EdwardsPoint#Mul<'b Scalar>#mul()` (line 1806)
  - `ristretto/&RistrettoPoint#Mul<'b Scalar>#mul()` (line 932)

## Code Location

The fix is in `src/lib.rs`:

```rust
/// Create a unique key for a function by combining symbol, signature, self_type, and line number.
fn make_unique_key(
    symbol: &str,
    signature: &str,
    self_type: Option<&str>,
    line: Option<i32>,
) -> String {
    let base = match self_type {
        Some(st) => format!("{}|{}|{}", symbol, signature, st),
        None => format!("{}|{}", symbol, signature),
    };
    match line {
        Some(l) => format!("{}@{}", base, l),
        None => base,
    }
}
```

## Tests

See `tests/duplicate_symbols.rs`:

- `test_duplicate_mul_implementations` - Verifies multiple Mul impls are captured
- `test_same_symbol_and_signature_different_self_types` - Specifically tests the X<a,b> case
- `test_scip_names_include_type_info` - Verifies output includes type info for disambiguation
- `test_neg_implementations_for_scalar` - Tests Neg trait impls (different symbol pattern)
- `test_neg_implementations_for_ristretto` - Tests Neg trait impls for RistrettoPoint

## Related Documentation

- `docs/DUPLICATE_SYMBOL_BUG.md` - Original bug description
- `docs/TRAIT_IMPL_SYMBOL_PATTERNS.md` - SCIP symbol patterns for trait impls
- `docs/SCIP_SYMBOL_FORMAT_COMPARISON.md` - rust-analyzer vs verus-analyzer differences

