//! Specify command - Extract function specifications to JSON.

use probe_verus::constants::LINE_TOLERANCE;
use probe_verus::path_utils::{extract_src_suffix, paths_match_by_suffix};
use probe_verus::verus_parser::{self, FunctionInfo, ParsedOutput};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// Atom entry from atoms.json for code-name lookup.
#[derive(Deserialize)]
struct AtomEntry {
    #[serde(rename = "display-name")]
    display_name: String,
    #[serde(rename = "code-path")]
    code_path: String,
    #[serde(rename = "code-text")]
    code_text: CodeText,
}

#[derive(Deserialize)]
struct CodeText {
    #[serde(rename = "lines-start")]
    lines_start: usize,
}

/// Execute the specify command.
///
/// Extracts function specifications (requires/ensures) to JSON,
/// keyed by code-name from atoms.json.
pub fn cmd_specify(path: PathBuf, output: PathBuf, atoms_path: PathBuf, with_spec_text: bool) {
    // Validate inputs
    if !path.exists() {
        eprintln!("Error: Path does not exist: {}", path.display());
        std::process::exit(1);
    }

    if !atoms_path.exists() {
        eprintln!("Error: atoms.json not found at {}", atoms_path.display());
        std::process::exit(1);
    }

    // Load atoms.json to get code-name mappings
    let atoms = load_atoms(&atoms_path);

    // Parse all functions with spec info (requires/ensures)
    let parsed: ParsedOutput = verus_parser::parse_all_functions(
        &path,
        true,           // include_verus_constructs
        true,           // include_methods
        false,          // show_visibility
        false,          // show_kind
        with_spec_text, // include_spec_text
    );

    // Match functions to code-names and build output dictionary
    let (output_map, matched_count, unmatched_count) = match_functions_to_atoms(parsed, &atoms);

    // Write JSON output
    let json = serde_json::to_string_pretty(&output_map).expect("Failed to serialize JSON");
    std::fs::write(&output, &json).expect("Failed to write JSON output");

    println!(
        "Wrote {} functions to {} ({} unmatched)",
        matched_count,
        output.display(),
        unmatched_count
    );
}

/// Load atoms from a JSON file.
fn load_atoms(atoms_path: &PathBuf) -> HashMap<String, AtomEntry> {
    let atoms_content = std::fs::read_to_string(atoms_path).expect("Failed to read atoms.json");
    serde_json::from_str(&atoms_content).expect("Failed to parse atoms.json")
}

/// Match parsed functions to atoms by path and line number.
fn match_functions_to_atoms(
    parsed: ParsedOutput,
    atoms: &HashMap<String, AtomEntry>,
) -> (HashMap<String, FunctionInfo>, usize, usize) {
    let mut output_map: HashMap<String, FunctionInfo> = HashMap::new();
    let mut matched_count = 0;
    let mut unmatched_count = 0;

    for func in parsed.functions {
        if let Some(code_name) = find_matching_atom(&func, atoms) {
            output_map.insert(code_name, func);
            matched_count += 1;
        } else {
            unmatched_count += 1;
        }
    }

    (output_map, matched_count, unmatched_count)
}

/// Find the best matching atom for a function.
///
/// Matching strategy:
/// 1. Path must match (by suffix comparison)
/// 2. Display name must match
/// 3. SCIP line must fall within the function's span [start_line, end_line]
///    OR be within LINE_TOLERANCE of start_line
///
/// This handles the case where verus_syn includes doc comments in the span
/// (reporting an earlier start_line) while verus-analyzer reports the actual
/// function declaration line.
fn find_matching_atom(func: &FunctionInfo, atoms: &HashMap<String, AtomEntry>) -> Option<String> {
    let func_path = func.file.as_deref().unwrap_or("");
    let func_suffix = extract_src_suffix(func_path);

    let mut best_match: Option<&str> = None;
    let mut best_line_diff = usize::MAX;

    for (code_name, atom) in atoms {
        let atom_suffix = extract_src_suffix(&atom.code_path);

        let path_matches =
            paths_match_by_suffix(func_path, &atom.code_path) || func_suffix == atom_suffix;

        if path_matches && func.name == atom.display_name {
            let atom_line = atom.code_text.lines_start;

            // Check if SCIP line falls within the function span [start_line, end_line]
            // This handles doc comments being included in verus_syn's span
            let within_span =
                atom_line >= func.spec_text.lines_start && atom_line <= func.spec_text.lines_end;

            // Also check traditional tolerance for cases without doc comments
            let line_diff =
                (func.spec_text.lines_start as isize - atom_line as isize).unsigned_abs();
            let within_tolerance = line_diff <= LINE_TOLERANCE;

            if within_span || within_tolerance {
                // Prefer matches closer to start_line
                let effective_diff = if within_span && !within_tolerance {
                    // SCIP line is within span but after tolerance - use distance from start
                    atom_line - func.spec_text.lines_start
                } else {
                    line_diff
                };

                if effective_diff < best_line_diff {
                    best_match = Some(code_name);
                    best_line_diff = effective_diff;

                    // Exact match - can't do better
                    if effective_diff == 0 {
                        break;
                    }
                }
            }
        }
    }

    best_match.map(|s| s.to_string())
}
