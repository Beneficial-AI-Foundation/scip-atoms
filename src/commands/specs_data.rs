//! `specs-data` command: generate specs_data.json for the specs browser.
//!
//! This replaces the Python scripts (extract_specs.py + analyze_verus_specs_proofs.py)
//! with a single AST-based pass using verus_syn. It auto-discovers all functions,
//! categorizes them, computes cross-references, and outputs JSON matching the
//! existing specs_data.json schema consumed by docs/specs.js.

use probe_verus::verus_parser::{compute_project_prefix, parse_all_functions_ext, FunctionInfo};
use probe_verus::FunctionMode;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;

/// Top-level output matching the existing specs_data.json schema.
#[derive(Serialize)]
struct SpecsData {
    spec_functions: Vec<SpecFunctionEntry>,
    verified_functions: Vec<VerifiedFunctionEntry>,
}

/// A spec function or axiom entry (right panel of specs browser).
#[derive(Serialize)]
struct SpecFunctionEntry {
    id: String,
    name: String,
    signature: String,
    body: String,
    file: String,
    line: usize,
    module: String,
    short_module: String,
    visibility: String,
    doc_comment: String,
    math_interpretation: String,
    informal_interpretation: String,
    github_link: String,
    category: String,
    referenced_specs: Vec<String>,
}

/// A verified/tracked function entry (left panel of specs browser).
#[derive(Serialize)]
struct VerifiedFunctionEntry {
    id: String,
    name: String,
    display_name: String,
    impl_type: String,
    contract: String,
    requires: Vec<String>,
    ensures: Vec<String>,
    referenced_specs: Vec<String>,
    file: String,
    line: usize,
    module: String,
    doc_comment: String,
    math_interpretation: String,
    informal_interpretation: String,
    github_link: String,
    category: String,
    is_public: bool,
    is_libsignal: bool,
    has_spec: bool,
    has_proof: bool,
}

/// Derive a short module name from the full module path for grouping in the UI.
///
/// Examples: "specs::field_specs" -> "field", "backend::serial::u64::scalar" -> "scalar",
/// "lemmas::common_lemmas::foo" -> "lemmas"
fn derive_short_module(module_path: &str) -> String {
    if module_path.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = module_path.split("::").collect();
    // Use the first component as the short module (like the Python script does)
    let first = parts[0];
    match first {
        "specs" => {
            if parts.len() > 1 {
                // "specs::field_specs" -> strip "specs" from the second part
                let sub = parts[1];
                sub.strip_suffix("_specs").unwrap_or(sub).to_string()
            } else {
                "specs".to_string()
            }
        }
        "lemmas" => "lemmas".to_string(),
        "backend" => {
            // "backend::serial::u64::scalar" -> "scalar" (leaf module)
            parts.last().unwrap_or(&"backend").to_string()
        }
        other => other.to_string(),
    }
}

/// Extract a math interpretation from a doc comment.
///
/// Looks for lines containing = or equivalence that look like formulas, not prose.
fn extract_math_interpretation(doc_comment: &str) -> String {
    if doc_comment.is_empty() {
        return String::new();
    }

    let prose_re = Regex::new(r"(?i)^(this|the|it|we|for|if|when|note|see|returns|computes|checks|ensures|requires|proves|helper|verify|convert|used|should|must|can)\b").unwrap();
    let word_re = Regex::new(r"[a-zA-Z]{4,}").unwrap();
    let math_words: HashSet<&str> = [
        "sqrt", "mod", "pow", "spec", "nat", "int", "bool", "field", "scalar", "point", "limb",
        "byte", "bits",
    ]
    .into_iter()
    .collect();

    for line in doc_comment.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.contains('=') && !line.contains('\u{2261}') {
            continue;
        }
        if prose_re.is_match(line) {
            continue;
        }
        if line.len() > 100 {
            continue;
        }
        let words: Vec<_> = word_re.find_iter(line).collect();
        let non_math_words = words
            .iter()
            .filter(|w| !math_words.contains(w.as_str().to_lowercase().as_str()))
            .count();
        if non_math_words > 4 {
            continue;
        }
        return line.to_string();
    }
    String::new()
}

/// Compute cross-references: which spec function names appear in a function's
/// ensures/requires calls.
fn compute_referenced_specs(func: &FunctionInfo, spec_names: &HashSet<String>) -> Vec<String> {
    let mut refs: HashSet<String> = HashSet::new();
    for call in func.ensures_calls.iter().chain(func.requires_calls.iter()) {
        if spec_names.contains(call.as_str()) {
            refs.insert(call.clone());
        }
    }

    // Also scan the contract text for spec function references (the Python script does this)
    if let Some(ref req_text) = func.requires_text {
        for name in spec_names {
            if req_text.contains(name.as_str()) {
                refs.insert(name.clone());
            }
        }
    }
    if let Some(ref ens_text) = func.ensures_text {
        for name in spec_names {
            if ens_text.as_str().contains(name.as_str()) {
                refs.insert(name.clone());
            }
        }
    }

    let mut sorted: Vec<String> = refs.into_iter().collect();
    sorted.sort();
    sorted
}

/// Split requires/ensures text into individual clauses.
fn split_clauses(text: &Option<String>) -> Vec<String> {
    match text {
        Some(t) => {
            let trimmed = t.trim();
            // Strip leading "requires" or "ensures" keyword
            let body = if let Some(rest) = trimmed.strip_prefix("requires") {
                rest.trim()
            } else if let Some(rest) = trimmed.strip_prefix("ensures") {
                rest.trim()
            } else {
                trimmed
            };

            if body.is_empty() {
                return Vec::new();
            }

            // Each clause is separated by a comma at the end of a line
            body.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        }
        None => Vec::new(),
    }
}

/// Build a unique ID for a function, matching the Python script's convention.
fn make_id(module_path: &str, name: &str, display_name: &str, _line: usize) -> String {
    let base = if module_path.is_empty() {
        name.to_string()
    } else {
        format!("{}__{}", module_path.replace("::", "__"), name)
    };

    // For methods with generic impl types, use display_name to disambiguate
    if display_name.contains('<') {
        display_name
            .to_lowercase()
            .replace("::", "__")
            .replace('<', "_")
            .replace(['>', ' '], "")
    } else if display_name.contains("::") && !base.contains(&name.to_lowercase()) {
        // Impl method: use display_name
        display_name.to_lowercase().replace("::", "__")
    } else {
        base.to_lowercase()
    }
}

/// Generate specs_data.json from a source directory.
pub fn cmd_specs_data(src_path: PathBuf, output: PathBuf, github_base_url: Option<String>) {
    let github_base = github_base_url.unwrap_or_default();

    eprintln!("Parsing source files from: {}", src_path.display());

    // Parse all functions with extended info enabled
    let parsed = parse_all_functions_ext(
        &src_path, true, // include verus constructs (spec, proof, exec)
        true, // include methods
        true, // show visibility
        true, // show kind
        true, // include spec text
        true, // include extended info (doc comments, signatures, bodies)
    );

    eprintln!(
        "Parsed {} functions from {} files",
        parsed.summary.total_functions, parsed.summary.total_files
    );

    // Build the set of spec function names for cross-referencing
    let spec_names: HashSet<String> = parsed
        .functions
        .iter()
        .filter(|f| f.mode == FunctionMode::Spec)
        .map(|f| f.name.clone())
        .collect();

    let mut spec_functions = Vec::new();
    let mut verified_functions = Vec::new();

    // We need to figure out the file path prefix for github links.
    // The relative paths from parse_all_functions use src_path as base.
    // We need paths like "curve25519-dalek/src/specs/field_specs.rs".
    // If src_path points to e.g. /path/to/curve25519-dalek/src, we want
    // relative paths from the grandparent.
    let project_prefix = compute_project_prefix(&src_path);

    for func in &parsed.functions {
        let file = func.file.as_deref().unwrap_or("");
        let full_file_path = if let Some(ref prefix) = project_prefix {
            format!("{}/{}", prefix, file)
        } else {
            file.to_string()
        };
        let line = func.spec_text.lines_start;
        let module_path = func.module_path.as_deref().unwrap_or("");
        let display_name = func.display_name.as_deref().unwrap_or(&func.name);
        let doc_comment = func.doc_comment.as_deref().unwrap_or("");
        let math_interp = extract_math_interpretation(doc_comment);
        let github_link = format!("{}{}#L{}", github_base, full_file_path, line);
        let refs = compute_referenced_specs(func, &spec_names);
        let is_public = func
            .visibility
            .as_deref()
            .map(|v: &str| v.starts_with("pub"))
            .unwrap_or(false);

        match func.mode {
            FunctionMode::Spec => {
                let signature = func.signature_text.as_deref().unwrap_or("").to_string();
                let body = func.body_text.as_deref().unwrap_or("").to_string();
                let vis = func
                    .kind
                    .as_deref()
                    .map(|k: &str| {
                        if is_public {
                            format!("pub {}", k)
                        } else {
                            k.to_string()
                        }
                    })
                    .unwrap_or_default();
                let short_module = derive_short_module(module_path);
                let fn_id = make_id(module_path, &func.name, display_name, line);

                // Compute spec-to-spec references (which spec fns does this spec fn call?)
                let spec_refs = if let Some(ref body_text) = func.body_text {
                    let mut body_refs: Vec<String> = spec_names
                        .iter()
                        .filter(|sn| {
                            *sn != &func.name && body_text.as_str().contains(&format!("{}(", sn))
                        })
                        .cloned()
                        .collect();
                    body_refs.sort();
                    body_refs
                } else {
                    Vec::new()
                };

                spec_functions.push(SpecFunctionEntry {
                    id: fn_id,
                    name: func.name.clone(),
                    signature,
                    body,
                    file: full_file_path,
                    line,
                    module: module_path.to_string(),
                    short_module,
                    visibility: vis,
                    doc_comment: doc_comment.to_string(),
                    math_interpretation: math_interp,
                    informal_interpretation: doc_comment.to_string(),
                    github_link,
                    category: "spec".to_string(),
                    referenced_specs: spec_refs,
                });
            }
            FunctionMode::Proof if func.name.starts_with("axiom_") => {
                let signature = func.signature_text.as_deref().unwrap_or("").to_string();
                let body = func.body_text.as_deref().unwrap_or("").to_string();
                let short_module = derive_short_module(module_path);
                let fn_id = make_id(module_path, &func.name, display_name, line);

                spec_functions.push(SpecFunctionEntry {
                    id: fn_id,
                    name: func.name.clone(),
                    signature,
                    body,
                    file: full_file_path,
                    line,
                    module: module_path.to_string(),
                    short_module,
                    visibility: "proof fn".to_string(),
                    doc_comment: doc_comment.to_string(),
                    math_interpretation: math_interp,
                    informal_interpretation: doc_comment.to_string(),
                    github_link,
                    category: "axiom".to_string(),
                    referenced_specs: refs,
                });
            }
            FunctionMode::Exec | FunctionMode::Proof => {
                // Only include functions that have specs (the "verified" panel)
                if !func.specified {
                    continue;
                }

                let impl_type = func.impl_type.as_deref().unwrap_or("");
                let fn_id = make_id(module_path, &func.name, display_name, line);

                // Build contract text from signature + requires + ensures
                let mut contract_parts: Vec<String> = Vec::new();
                if let Some(ref sig) = func.signature_text {
                    contract_parts.push(sig.clone());
                }
                if let Some(ref req) = func.requires_text {
                    contract_parts.push(req.clone());
                }
                if let Some(ref ens) = func.ensures_text {
                    contract_parts.push(ens.clone());
                }
                let contract = contract_parts.join("\n");

                let requires = split_clauses(&func.requires_text);
                let ensures = split_clauses(&func.ensures_text);

                let has_spec = func.has_requires || func.has_ensures || func.is_external_body;
                let has_proof = func.is_proved();

                let short_module = derive_short_module(module_path);

                verified_functions.push(VerifiedFunctionEntry {
                    id: fn_id,
                    name: func.name.clone(),
                    display_name: display_name.to_string(),
                    impl_type: impl_type.to_string(),
                    contract,
                    requires,
                    ensures,
                    referenced_specs: refs,
                    file: full_file_path,
                    line,
                    module: short_module,
                    doc_comment: doc_comment.to_string(),
                    math_interpretation: math_interp,
                    informal_interpretation: String::new(),
                    github_link,
                    category: "tracked".to_string(),
                    is_public,
                    is_libsignal: false,
                    has_spec,
                    has_proof,
                });
            }
        }
    }

    // Sort for deterministic output
    spec_functions.sort_by(|a, b| a.id.cmp(&b.id));
    verified_functions.sort_by(|a, b| a.id.cmp(&b.id));

    let specs_data = SpecsData {
        spec_functions,
        verified_functions,
    };

    let json = serde_json::to_string_pretty(&specs_data).expect("Failed to serialize JSON");

    std::fs::write(&output, &json).expect("Failed to write output file");

    eprintln!(
        "Wrote specs_data.json: {} spec functions, {} verified functions -> {}",
        specs_data.spec_functions.len(),
        specs_data.verified_functions.len(),
        output.display()
    );
}
