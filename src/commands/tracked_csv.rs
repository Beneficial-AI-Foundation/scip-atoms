//! `tracked-csv` command: generate curve25519_functions.csv for the dashboard.
//!
//! This replaces `analyze_verus_specs_proofs.py` with AST-based auto-discovery.
//! It outputs a CSV with the same schema consumed by the dashboard scripts:
//! `function,module,link,has_spec,has_proof`

use probe_verus::verus_parser::{compute_project_prefix, parse_all_functions_ext};
use probe_verus::FunctionMode;
use std::io::Write;
use std::path::PathBuf;

/// Generate the tracked CSV file.
pub fn cmd_tracked_csv(src_path: PathBuf, output: PathBuf, github_base_url: Option<String>) {
    let github_base = github_base_url.unwrap_or_default();

    eprintln!("Parsing source files from: {}", src_path.display());

    let parsed = parse_all_functions_ext(
        &src_path, true, // include verus constructs
        true, // include methods
        true, // show visibility
        true, // show kind
        true, // include spec text
        true, // include extended info
    );

    eprintln!(
        "Parsed {} functions from {} files",
        parsed.summary.total_functions, parsed.summary.total_files
    );

    let project_prefix = compute_project_prefix(&src_path);

    // Collect rows: only exec/proof functions that have specs or external_body
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();

    for func in &parsed.functions {
        // Only track exec-mode functions (the actual Rust implementations).
        // Proof-mode functions are Verus lemmas, and spec functions have no bodies.
        if func.mode != FunctionMode::Exec {
            continue;
        }

        // Only include functions with real specs (requires/ensures).
        // External body functions are excluded -- they have trusted bodies
        // with no proof, so including them dilutes the proof completion rate.
        if !func.specified || func.is_external_body {
            continue;
        }

        let file = func.file.as_deref().unwrap_or("");
        let full_file_path = if let Some(ref prefix) = project_prefix {
            format!("{}/{}", prefix, file)
        } else {
            file.to_string()
        };
        let line = func.spec_text.lines_start;
        let module_path = func.module_path.as_deref().unwrap_or("");

        // function column: display_name (e.g., "FieldElement51::mul")
        let function_name = func
            .display_name
            .as_deref()
            .unwrap_or(&func.name)
            .to_string();

        // module column: "curve25519_dalek::" + module_path
        let module = if module_path.is_empty() {
            "curve25519_dalek".to_string()
        } else {
            format!("curve25519_dalek::{}", module_path)
        };

        // link column
        let link = format!("{}{}#L{}", github_base, full_file_path, line);

        // All functions reaching here have specs (external_body already filtered out)
        let has_spec = "yes".to_string();

        let has_proof = if func.is_proved() {
            "yes".to_string()
        } else {
            String::new()
        };

        rows.push((function_name, module, link, has_spec, has_proof));
    }

    // Sort by function name for deterministic output
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    // Write CSV
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut file = std::fs::File::create(&output).expect("Failed to create output file");
    writeln!(file, "function,module,link,has_spec,has_proof").unwrap();
    for (function, module, link, has_spec, has_proof) in &rows {
        writeln!(
            file,
            "{},{},{},{},{}",
            function, module, link, has_spec, has_proof
        )
        .unwrap();
    }

    // Print summary
    let total = rows.len();
    let proof_count = rows.iter().filter(|r| r.4 == "yes").count();

    eprintln!("\nCSV written: {}", output.display());
    eprintln!("Summary:");
    eprintln!("  Exec functions with specs: {}", total);
    eprintln!(
        "  With complete proofs: {} ({}%)",
        proof_count,
        if total > 0 {
            proof_count * 100 / total
        } else {
            0
        }
    );
    eprintln!("  Without complete proof: {}", total - proof_count);
}
