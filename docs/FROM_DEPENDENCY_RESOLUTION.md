# From Trait Dependency Resolution

This document explains how `scip-atoms` resolves calls to `From::from()` to the correct trait implementation.

## The Challenge

The `From` trait is commonly implemented multiple times for the same struct with different type parameters:

```rust
impl<'a> From<&'a EdwardsPoint> for NafLookupTable5<ProjectiveNielsPoint> { ... }
impl<'a> From<&'a EdwardsPoint> for NafLookupTable5<AffineNielsPoint> { ... }
```

In verus-analyzer's SCIP output, both implementations have **identical symbols**:
```
window/NafLookupTable5#From#from().
window/NafLookupTable5#From#from().
```

When code calls `NafLookupTable5::from(point)`, which implementation should appear in the dependencies?

## The Solution

We leverage two pieces of information from the SCIP index:

### 1. Definition-Site Type Context

Near each `fn from()` definition, SCIP records type references that tell us which impl it belongs to:

```
Line 537 (definition): ProjectiveNielsPoint, EdwardsPoint, NafLookupTable5
Line 549 (definition): AffineNielsPoint, EdwardsPoint, NafLookupTable5
```

We capture types within 5 lines before each definition as `definition_type_context`.

### 2. Call-Site Type Hints (Turbofish)

When the source code uses explicit type syntax:
```rust
let table = NafLookupTable5::<ProjectiveNielsPoint>::from(A);
```

SCIP records the turbofish type as a **separate occurrence** on the same line:
```
Line 40: [REF] NafLookupTable5#
Line 40: [REF] ProjectiveNielsPoint#    ← Type hint!
Line 40: [REF] NafLookupTable5#From#from().
```

We capture these as `type_hints` for each callee reference.

### 3. Matching Algorithm

When resolving dependencies:

1. **Find discriminating types**: Types that appear in SOME but not ALL implementations
   - `ProjectiveNielsPoint` appears only in the first impl
   - `AffineNielsPoint` appears only in the second impl
   - `EdwardsPoint` appears in both (not discriminating)

2. **Match call-site hints to definitions**: If a call-site has `ProjectiveNielsPoint`, match it to the impl with `ProjectiveNielsPoint` in its context

3. **Generate enriched scip_names**: Include the target type in the name
   - `NafLookupTable5<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()`

## Results

### Without Disambiguation (ambiguous)

verus-analyzer produces identical symbols for different implementations:
```json
{
  "scip-name": "vartime_double_base/.../mul()",
  "dependencies": [
    "window/NafLookupTable5#From#from()"
  ]
}
```

The dependency is ambiguous — it could refer to either the `ProjectiveNielsPoint` or `AffineNielsPoint` implementation.

### With Disambiguation (resolved)

scip-atoms enriches the symbols with target type information:
```json
{
  "scip-name": "vartime_double_base/.../mul()",
  "dependencies": [
    "window/NafLookupTable5<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()"
  ]
}
```

The dependency now points to exactly the correct implementation.

## Verified Examples

| Caller | Type Hint | Resolved Implementation |
|--------|-----------|------------------------|
| `precomputed_straus/.../new()` | `AffineNielsPoint` | `NafLookupTable8<AffineNielsPoint>#From<&EdwardsPoint>#from()` |
| `variable_base/.../mul()` | `ProjectiveNielsPoint` | `LookupTable<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()` |
| `vartime_double_base/.../mul()` | `ProjectiveNielsPoint` | `NafLookupTable5<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()` |

## All From Implementations

After disambiguation, each From implementation has a unique, descriptive scip_name:

```
Scalar#From<u8>#from()
Scalar#From<u16>#from()
Scalar#From<u32>#from()
Scalar#From<u64>#from()
Scalar#From<u128>#from()
BatchCompressState#From<&RistrettoPoint>#from()
LookupTable<AffineNielsPoint>#From<&EdwardsPoint>#from()
LookupTable<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()
NafLookupTable5<AffineNielsPoint>#From<&EdwardsPoint>#from()
NafLookupTable5<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()
NafLookupTable8<AffineNielsPoint>#From<&EdwardsPoint>#from()
NafLookupTable8<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()
```

## Limitations

This disambiguation works when:
- The call site uses **explicit type syntax** (turbofish `::< >` or type annotations)
- SCIP records the type as a separate occurrence

Cases relying purely on type inference (no explicit type in source) cannot be disambiguated from SCIP data alone.
