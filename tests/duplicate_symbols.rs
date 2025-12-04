use scip_atoms::{build_call_graph, convert_to_atoms_with_lines, parse_scip_json};

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

    let mul_atoms: Vec<_> = atoms
        .iter()
        .filter(|a| a.scip_name.contains("montgomery/Mul") && a.scip_name.contains("#mul"))
        .collect();

    // Should have at least 2 distinct Mul implementations
    assert!(
        mul_atoms.len() >= 2,
        "Expected at least 2 montgomery/Mul atoms, found {}",
        mul_atoms.len()
    );

    // The scip_names should include type parameters to distinguish them
    let scip_names: Vec<_> = mul_atoms.iter().map(|a| a.scip_name.as_str()).collect();

    assert!(
        scip_names.iter().any(|s| s.contains("Mul<Scalar>")),
        "Expected scip_name with Mul<Scalar>, got: {:?}",
        scip_names
    );
    assert!(
        scip_names
            .iter()
            .any(|s| s.contains("Mul<MontgomeryPoint>")),
        "Expected scip_name with Mul<MontgomeryPoint>, got: {:?}",
        scip_names
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
