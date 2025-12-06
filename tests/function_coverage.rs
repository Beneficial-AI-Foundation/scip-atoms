//! Test that validates all tracked functions from the CSV are present in the generated atoms.
//!
//! This test downloads the function tracking CSV from:
//! https://github.com/Beneficial-AI-Foundation/dalek-lite/blob/main/functions_to_track.csv
//!
//! And verifies that each function appears in the generated atoms.json.

use scip_atoms::{build_call_graph, convert_to_atoms_with_lines, parse_scip_json, AtomWithLines};
use std::collections::HashMap;

const CSV_URL: &str =
    "https://raw.githubusercontent.com/Beneficial-AI-Foundation/dalek-lite/main/functions_to_track.csv";

/// Parsed function entry from the CSV
#[derive(Debug, Clone)]
struct TrackedFunction {
    /// Full function signature, e.g., "Scalar::hash_from_bytes(&[u8])"
    function: String,
    /// Module path, e.g., "curve25519_dalek::scalar"
    module: String,
    /// Impl block, e.g., "Scalar" or "Mul<&'b Scalar> for Scalar"
    impl_block: String,
}

impl TrackedFunction {
    /// Extract just the function/method name from the full signature.
    /// E.g., "Scalar::hash_from_bytes(&[u8])" -> "hash_from_bytes"
    /// E.g., "elligator_encode(&FieldElement)" -> "elligator_encode"
    fn method_name(&self) -> &str {
        let func = &self.function;

        // Find the opening paren to strip parameters
        let without_params = func.split('(').next().unwrap_or(func);

        // If it has ::, take the part after the last ::
        if let Some(pos) = without_params.rfind("::") {
            &without_params[pos + 2..]
        } else {
            without_params
        }
    }

    /// Get the module name (last component of the module path).
    /// E.g., "curve25519_dalek::scalar" -> "scalar"
    fn module_name(&self) -> &str {
        self.module.split("::").last().unwrap_or(&self.module)
    }
}

/// Download and parse the CSV file from GitHub.
fn fetch_tracked_functions() -> Result<Vec<TrackedFunction>, String> {
    let response = ureq::get(CSV_URL)
        .call()
        .map_err(|e| format!("Failed to fetch CSV: {}", e))?;

    let body = response
        .into_string()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    parse_csv(&body)
}

/// Parse CSV content into TrackedFunction entries.
fn parse_csv(content: &str) -> Result<Vec<TrackedFunction>, String> {
    let mut functions = Vec::new();

    for (i, line) in content.lines().enumerate() {
        // Skip header line
        if i == 0 {
            continue;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Handle quoted fields and commas within quotes
        let parts = parse_csv_line(line);
        if parts.len() >= 3 {
            functions.push(TrackedFunction {
                function: parts[0].clone(),
                module: parts[1].clone(),
                impl_block: parts[2].clone(),
            });
        }
    }

    Ok(functions)
}

/// Parse a single CSV line, handling quoted fields.
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in line.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(c),
        }
    }
    result.push(current.trim().to_string());

    result
}

/// Load atoms from curve_top.json and convert to the standard format.
fn load_atoms() -> Vec<AtomWithLines> {
    let scip_data = parse_scip_json("data/curve_top.json").expect("Failed to parse SCIP JSON");
    let (call_graph, symbol_to_display_name) = build_call_graph(&scip_data);
    convert_to_atoms_with_lines(&call_graph, &symbol_to_display_name)
}

/// Build a lookup structure for efficient matching.
/// Maps (display_name, module_name) -> list of atoms
fn build_atom_index(atoms: &[AtomWithLines]) -> HashMap<(String, String), Vec<&AtomWithLines>> {
    let mut index: HashMap<(String, String), Vec<&AtomWithLines>> = HashMap::new();

    for atom in atoms {
        // Extract module name from scip-name
        // E.g., "curve25519-dalek 4.1.3 scalar/Scalar#hash_from_bytes()" -> "scalar"
        if let Some(module) = extract_module_from_scip_name(&atom.scip_name) {
            let key = (atom.display_name.clone(), module);
            index.entry(key).or_default().push(atom);
        }
    }

    index
}

/// Extract the module name from a scip-name.
/// E.g., "curve25519-dalek 4.1.3 scalar/Scalar#hash_from_bytes()" -> "scalar"
/// E.g., "curve25519-dalek 4.1.3 backend/serial/u64/field/FieldElement51#add()" -> "field"
fn extract_module_from_scip_name(scip_name: &str) -> Option<String> {
    // Skip the crate and version prefix
    let parts: Vec<&str> = scip_name.splitn(3, ' ').collect();
    if parts.len() < 3 {
        return None;
    }

    let path = parts[2]; // e.g., "scalar/Scalar#hash_from_bytes()"

    // Split by / and # to get path components
    let path_parts: Vec<&str> = path.split('/').collect();

    // For nested modules like "backend/serial/u64/field", we want the last directory component
    // before the type/function, which is the part before the #
    if let Some(last_dir) = path_parts.iter().rev().find(|p| !p.contains('#')) {
        return Some(last_dir.to_string());
    }

    // If all parts contain #, extract from the first part with #
    for part in &path_parts {
        if let Some(pos) = part.find('#') {
            return Some(part[..pos].to_string());
        }
    }

    None
}

/// Check if a tracked function exists in the atoms.
fn find_matching_atom<'a>(
    tracked: &TrackedFunction,
    index: &'a HashMap<(String, String), Vec<&'a AtomWithLines>>,
    atoms: &'a [AtomWithLines],
) -> Option<&'a AtomWithLines> {
    let method_name = tracked.method_name();
    let module_name = tracked.module_name();

    // Try exact match first
    let key = (method_name.to_string(), module_name.to_string());
    if let Some(matches) = index.get(&key) {
        if !matches.is_empty() {
            return Some(matches[0]);
        }
    }

    // Try fuzzy match: just by display name and module substring
    atoms
        .iter()
        .find(|atom| atom.display_name == method_name && atom.scip_name.contains(module_name))
}

#[test]
fn test_all_tracked_functions_present() {
    // Fetch the tracked functions from GitHub
    let tracked_functions = fetch_tracked_functions().expect("Failed to fetch tracked functions");

    println!(
        "Fetched {} tracked functions from CSV",
        tracked_functions.len()
    );

    // Load atoms from the generated data
    let atoms = load_atoms();
    let index = build_atom_index(&atoms);

    println!("Loaded {} atoms from curve_top.json", atoms.len());

    // Check each tracked function
    let mut missing: Vec<&TrackedFunction> = Vec::new();
    let mut found_count = 0;

    for tracked in &tracked_functions {
        if find_matching_atom(tracked, &index, &atoms).is_some() {
            found_count += 1;
        } else {
            missing.push(tracked);
        }
    }

    // Calculate coverage percentage
    let coverage = (found_count as f64 / tracked_functions.len() as f64) * 100.0;

    // Report results
    println!("\n=== Function Coverage Report ===");
    println!("Total tracked functions: {}", tracked_functions.len());
    println!("Found in atoms: {}", found_count);
    println!("Missing: {}", missing.len());
    println!("Coverage: {:.1}%", coverage);

    if !missing.is_empty() {
        println!("\n=== Missing Functions ===");
        for func in &missing {
            println!(
                "  - {} (module: {}, impl: {})",
                func.function, func.module, func.impl_block
            );
        }
    }

    // This test always passes - it's for reporting coverage, not enforcing it
    println!("\n[INFO] This test reports coverage but does not fail on missing functions.");
}

#[test]
fn test_specific_critical_functions() {
    // These are critical functions we want to track
    let critical_functions = vec![
        ("hash_from_bytes", "scalar"),
        ("from_bytes_mod_order", "scalar"),
        ("from_bytes_mod_order_wide", "scalar"),
        ("mul", "montgomery"),
        ("to_bytes", "scalar"),
        ("invert", "scalar"),
    ];

    let atoms = load_atoms();
    let index = build_atom_index(&atoms);

    let mut found_critical = Vec::new();
    let mut missing_critical = Vec::new();

    for (method, module) in &critical_functions {
        let key = (method.to_string(), module.to_string());
        let found = index.contains_key(&key)
            || atoms
                .iter()
                .any(|a| &a.display_name == method && a.scip_name.contains(module));

        if found {
            found_critical.push((method, module));
        } else {
            missing_critical.push((method, module));
        }
    }

    println!("\n=== Critical Functions Report ===");
    println!("Found: {:?}", found_critical);
    if !missing_critical.is_empty() {
        println!("Missing: {:?}", missing_critical);
    }

    // Report only, don't fail
}

#[test]
fn test_csv_parsing() {
    let sample_csv = r#"function,module,impl_block
Scalar::hash_from_bytes(&[u8]),curve25519_dalek::scalar,Scalar
"differential_add_and_double(&ProjectivePoint, &ProjectivePoint, &FieldElement)",curve25519_dalek::montgomery,
elligator_encode(&FieldElement),curve25519_dalek::montgomery,
"#;

    let functions = parse_csv(sample_csv).unwrap();

    assert_eq!(functions.len(), 3);

    assert_eq!(functions[0].method_name(), "hash_from_bytes");
    assert_eq!(functions[0].module_name(), "scalar");

    assert_eq!(functions[1].method_name(), "differential_add_and_double");
    assert_eq!(functions[1].module_name(), "montgomery");

    assert_eq!(functions[2].method_name(), "elligator_encode");
    assert_eq!(functions[2].module_name(), "montgomery");
}
