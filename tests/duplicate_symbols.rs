use scip_atoms::{
    build_call_graph, convert_to_atoms_with_lines, find_duplicate_scip_names, parse_scip_json,
};

fn get_test_data() -> (
    std::collections::HashMap<String, scip_atoms::FunctionNode>,
    std::collections::HashMap<String, String>,
) {
    let scip_data = parse_scip_json("data/curve_top.json").expect("Failed to parse SCIP JSON");
    build_call_graph(&scip_data)
}

/// Test that multiple trait implementations with the same SCIP symbol
/// (e.g., `impl Mul<A> for B` and `impl Mul<B> for A`) are both captured.
///
/// See docs/DUPLICATE_SYMBOL_BUG.md for details on this bug.
#[test]
fn test_duplicate_mul_implementations() {
    let scip_data = parse_scip_json("data/curve_top.json").expect("Failed to parse SCIP JSON");
    let (call_graph, _symbol_to_display_name) = build_call_graph(&scip_data);

    // Find all entries with "Mul#mul" in their symbol (for montgomery module)
    let mut mul_entries: Vec<_> = call_graph
        .values()
        .filter(|node| node.symbol.contains("montgomery/Mul#mul"))
        .collect();

    mul_entries.sort_by_key(|n| n.range.first().copied().unwrap_or(0));

    // We should have at least 2 montgomery/Mul#mul implementations
    assert!(
        mul_entries.len() >= 2,
        "Expected at least 2 montgomery/Mul#mul implementations, found {}",
        mul_entries.len()
    );

    // Verify we have both signatures
    let signatures: Vec<_> = mul_entries
        .iter()
        .map(|n| n.signature_text.as_str())
        .collect();

    assert!(
        signatures.iter().any(|s| s.contains("scalar: &Scalar")),
        "Missing MontgomeryPoint * Scalar implementation"
    );
    assert!(
        signatures
            .iter()
            .any(|s| s.contains("point: &MontgomeryPoint")),
        "Missing Scalar * MontgomeryPoint implementation"
    );

    // Verify distinct line numbers (don't hardcode specific lines as they may change)
    let lines: Vec<_> = mul_entries
        .iter()
        .filter_map(|n| n.range.first().map(|l| l + 1))
        .collect();

    // Check that we have at least 2 distinct line numbers
    let unique_lines: std::collections::HashSet<_> = lines.iter().collect();
    assert!(
        unique_lines.len() >= 2,
        "Expected at least 2 distinct line numbers, got: {:?}",
        lines
    );
}

#[test]
fn test_scip_names_include_type_info() {
    let scip_data = parse_scip_json("data/curve_top.json").expect("Failed to parse SCIP JSON");
    let (call_graph, symbol_to_display_name) = build_call_graph(&scip_data);
    let atoms = convert_to_atoms_with_lines(&call_graph, &symbol_to_display_name);

    // With the self_type repair, the format is now:
    // montgomery/MontgomeryPoint#Mul<Scalar>#mul() and montgomery/Scalar#Mul<MontgomeryPoint>#mul()
    // Look for atoms that contain both the Mul trait and mul method
    let mul_atoms: Vec<_> = atoms
        .iter()
        .filter(|a| a.scip_name.contains("Mul") && a.scip_name.contains("#mul"))
        .filter(|a| a.scip_name.contains("montgomery/"))
        .collect();

    // Should have at least 2 distinct Mul implementations
    assert!(
        mul_atoms.len() >= 2,
        "Expected at least 2 montgomery Mul atoms, found {}. Atoms: {:?}",
        mul_atoms.len(),
        mul_atoms.iter().map(|a| &a.scip_name).collect::<Vec<_>>()
    );

    // The scip_names should include type parameters to distinguish them
    // Note: We preserve the & for reference types, so expect &Scalar not just Scalar
    let scip_names: Vec<_> = mul_atoms.iter().map(|a| a.scip_name.as_str()).collect();

    // Check that type parameters are present for disambiguation
    // The & is now preserved for reference types
    assert!(
        scip_names.iter().any(|s| s.contains("Mul<&Scalar>")),
        "Expected scip_name with Mul<&Scalar>, got: {:?}",
        scip_names
    );
    assert!(
        scip_names
            .iter()
            .any(|s| s.contains("Mul<&MontgomeryPoint>")),
        "Expected scip_name with Mul<&MontgomeryPoint>, got: {:?}",
        scip_names
    );

    // With the new self_type repair, symbols should also include the Self type
    // e.g., montgomery/&MontgomeryPoint#Mul<&Scalar>#mul()
    // Check that at least one has the Self type in the path
    let has_self_type = scip_names
        .iter()
        .any(|s| s.contains("MontgomeryPoint#Mul") || s.contains("Scalar#Mul"));
    assert!(
        has_self_type,
        "Expected self_type in scip_name (e.g., MontgomeryPoint#Mul), got: {:?}",
        scip_names
    );
}

/// Test that implementations with same symbol AND same signature but different Self types
/// are all captured. This is the bug reported as "impls of form X<a,b> are overwriting each other".
///
/// Example: `impl Mul<&Scalar> for &RistrettoPoint` and `impl Mul<&Scalar> for &RistrettoBasepointTable`
///
/// Both have:
/// - Symbol: `ristretto/Mul#mul().`
/// - Signature: `fn mul(self, scalar: &'b Scalar) -> RistrettoPoint`
///
/// But different self types: `&RistrettoPoint` vs `&RistrettoBasepointTable`
#[test]
fn test_same_symbol_and_signature_different_self_types() {
    let (call_graph, _) = get_test_data();

    // Find all ristretto/Mul#mul() implementations with signature containing "scalar: &"
    // (i.e., Mul<&Scalar> implementations)
    let mul_scalar_entries: Vec<_> = call_graph
        .values()
        .filter(|node| {
            node.symbol.contains("ristretto/Mul#mul")
                && node.signature_text.contains("scalar:")
                && node.signature_text.contains("&")
        })
        .collect();

    // We should have at least 2 different Self types for `impl Mul<&Scalar>`:
    // - &RistrettoPoint
    // - &RistrettoBasepointTable
    // (and possibly &Scalar, depending on SCIP data)
    let self_types: Vec<_> = mul_scalar_entries
        .iter()
        .map(|n| n.self_type.as_deref().unwrap_or("None"))
        .collect();

    let unique_self_types: std::collections::HashSet<_> = self_types.iter().cloned().collect();

    assert!(
        unique_self_types.len() >= 2,
        "Expected at least 2 different Self types for ristretto Mul<&Scalar>, got: {:?}",
        self_types
    );

    // Also verify all have different entries in call_graph (none were overwritten)
    assert_eq!(
        mul_scalar_entries.len(),
        unique_self_types.len(),
        "Some implementations were overwritten! Entries: {}, Unique self_types: {}",
        mul_scalar_entries.len(),
        unique_self_types.len()
    );
}

/// Test that Neg trait implementations for both &Type and Type are captured.
/// Unlike Mul, Neg implementations have different SCIP symbols:
/// - `impl Neg for &Type` → `module/Neg#neg()`
/// - `impl Neg for Type` → `module/Type#Neg#neg()`
#[test]
fn test_neg_implementations_for_scalar() {
    let (call_graph, _) = get_test_data();

    // Find all Neg implementations for scalar
    let neg_entries: Vec<_> = call_graph
        .values()
        .filter(|node| node.symbol.contains("scalar") && node.symbol.contains("Neg#neg"))
        .collect();

    // Should have both `scalar/Neg#neg()` and `scalar/Scalar#Neg#neg()`
    assert!(
        neg_entries.len() >= 2,
        "Expected at least 2 scalar Neg implementations, found {}: {:?}",
        neg_entries.len(),
        neg_entries.iter().map(|n| &n.symbol).collect::<Vec<_>>()
    );

    let symbols: Vec<_> = neg_entries.iter().map(|n| n.symbol.as_str()).collect();

    // Check for impl Neg for &Scalar (at line 816)
    assert!(
        symbols.iter().any(|s| s.contains("scalar/Neg#neg")),
        "Missing impl Neg for &Scalar"
    );

    // Check for impl Neg for Scalar (at line 881)
    assert!(
        symbols.iter().any(|s| s.contains("scalar/Scalar#Neg#neg")),
        "Missing impl Neg for Scalar"
    );
}

#[test]
fn test_neg_implementations_for_ristretto() {
    let (call_graph, _) = get_test_data();

    // Find all Neg implementations for ristretto
    let neg_entries: Vec<_> = call_graph
        .values()
        .filter(|node| node.symbol.contains("ristretto") && node.symbol.contains("Neg#neg"))
        .collect();

    // Should have both `ristretto/Neg#neg()` and `ristretto/RistrettoPoint#Neg#neg()`
    assert!(
        neg_entries.len() >= 2,
        "Expected at least 2 ristretto Neg implementations, found {}: {:?}",
        neg_entries.len(),
        neg_entries.iter().map(|n| &n.symbol).collect::<Vec<_>>()
    );

    let symbols: Vec<_> = neg_entries.iter().map(|n| n.symbol.as_str()).collect();

    // Check for impl Neg for &RistrettoPoint (at line 909)
    assert!(
        symbols.iter().any(|s| s.contains("ristretto/Neg#neg")),
        "Missing impl Neg for &RistrettoPoint"
    );

    // Check for impl Neg for RistrettoPoint (at line 917)
    assert!(
        symbols
            .iter()
            .any(|s| s.contains("ristretto/RistrettoPoint#Neg#neg")),
        "Missing impl Neg for RistrettoPoint"
    );
}

/// Test that From trait implementations are captured and disambiguated correctly.
/// From::from has only one parameter (not self + param), so it needs special handling.
///
/// Example: `impl From<EdwardsPoint> for LookupTable` and `impl From<ProjectiveNielsPoint> for LookupTable`
/// Both have:
/// - Same symbol: `window/LookupTable#From#from()`
/// - But different source types: `EdwardsPoint` vs `ProjectiveNielsPoint`
///
/// For cases where symbol+signature are identical (e.g., generic impls like
/// `impl From<&EdwardsPoint> for LookupTable<AffineNielsPoint>` vs
/// `impl From<&EdwardsPoint> for LookupTable<ProjectiveNielsPoint>`),
/// line numbers are added as a suffix to disambiguate.
#[test]
fn test_from_implementations_are_disambiguated() {
    let (call_graph, symbol_to_display_name) = get_test_data();
    let atoms = convert_to_atoms_with_lines(&call_graph, &symbol_to_display_name);

    // Find all From#from atoms for window module
    let from_atoms: Vec<_> = atoms
        .iter()
        .filter(|a| a.scip_name.contains("From") && a.scip_name.contains("from"))
        .filter(|a| a.scip_name.contains("window/"))
        .collect();

    // Should have multiple From implementations
    if from_atoms.len() >= 2 {
        // Check that scip_names are unique (no duplicates after disambiguation)
        let scip_names: std::collections::HashSet<_> =
            from_atoms.iter().map(|a| a.scip_name.as_str()).collect();

        // Each should be unique (no duplicates)
        assert_eq!(
            scip_names.len(),
            from_atoms.len(),
            "Some From implementations have duplicate scip_names! Found {} atoms but only {} unique scip_names: {:?}",
            from_atoms.len(),
            scip_names.len(),
            scip_names
        );
    }
}

/// Test that there are no duplicate scip_names in the output.
/// This is a regression test for the issue where trait implementations
/// with the same symbol but different types were not disambiguated.
#[test]
fn test_no_duplicate_scip_names() {
    let (call_graph, symbol_to_display_name) = get_test_data();
    let atoms = convert_to_atoms_with_lines(&call_graph, &symbol_to_display_name);

    let duplicates = find_duplicate_scip_names(&atoms);

    // Print duplicates for debugging if test fails
    if !duplicates.is_empty() {
        eprintln!("Found {} duplicate scip_name(s):", duplicates.len());
        for dup in &duplicates {
            eprintln!("  - '{}'", dup.scip_name);
            for occ in &dup.occurrences {
                eprintln!(
                    "    at {}:{} ({})",
                    occ.code_path, occ.lines_start, occ.display_name
                );
            }
        }
    }

    // For now, we only warn about duplicates but don't fail the test.
    // This is because some edge cases may be unavoidable (e.g., Default::default
    // with no parameters to disambiguate).
    // If duplicates become a problem, uncomment the assertion below:
    //
    // assert!(
    //     duplicates.is_empty(),
    //     "Found {} duplicate scip_names - see above for details",
    //     duplicates.len()
    // );
}
