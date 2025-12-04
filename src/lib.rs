use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub mod verus_parser;

/// SCIP data structures
#[derive(Debug, Serialize, Deserialize)]
pub struct ScipIndex {
    pub metadata: Metadata,
    pub documents: Vec<Document>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub tool_info: ToolInfo,
    pub project_root: String,
    pub text_document_encoding: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub language: String,
    pub relative_path: String,
    pub occurrences: Vec<Occurrence>,
    #[serde(default)]
    pub symbols: Vec<Symbol>,
    pub position_encoding: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Occurrence {
    pub range: Vec<i32>,
    pub symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_roles: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Symbol {
    pub symbol: String,
    pub kind: i32,
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<Vec<String>>,
    pub signature_documentation: SignatureDocumentation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignatureDocumentation {
    pub language: String,
    pub text: String,
    pub position_encoding: i32,
}

/// Function node in the call graph
#[derive(Debug, Clone)]
pub struct FunctionNode {
    pub symbol: String,
    pub display_name: String,
    pub signature_text: String,
    pub relative_path: String,
    pub callees: HashSet<String>,
    pub range: Vec<i32>,
}

/// Output format: Atom with line numbers
#[derive(Debug, Serialize, Deserialize)]
pub struct AtomWithLines {
    #[serde(rename = "display-name")]
    pub display_name: String,
    #[serde(rename = "scip-name")]
    pub scip_name: String,
    pub dependencies: HashSet<String>,
    #[serde(rename = "code-path")]
    pub code_path: String,
    #[serde(rename = "code-text")]
    pub code_text: CodeTextInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeTextInfo {
    #[serde(rename = "lines-start")]
    pub lines_start: usize,
    #[serde(rename = "lines-end")]
    pub lines_end: usize,
}

/// Parse a SCIP JSON file
pub fn parse_scip_json(file_path: &str) -> Result<ScipIndex, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(file_path)?;
    let index: ScipIndex = serde_json::from_str(&contents)?;
    Ok(index)
}

/// Check if a symbol kind represents a function-like entity
fn is_function_like(kind: i32) -> bool {
    matches!(kind, 6 | 17 | 26 | 80) // Method, Function, etc.
}

/// Create a unique key for a function by combining symbol and signature.
/// This handles cases where multiple trait impls have the same symbol but different signatures.
fn make_unique_key(symbol: &str, signature: &str) -> String {
    format!("{}|{}", symbol, signature)
}

/// Build a call graph from SCIP data.
/// Returns the call graph and a map of all function symbols to their display names.
///
/// Note: Multiple trait implementations (e.g., `impl Mul<A> for B` and `impl Mul<B> for A`)
/// can have the same SCIP symbol string. We use signature_documentation.text to distinguish them.
pub fn build_call_graph(
    scip_data: &ScipIndex,
) -> (HashMap<String, FunctionNode>, HashMap<String, String>) {
    let mut call_graph: HashMap<String, FunctionNode> = HashMap::new();
    let mut project_function_keys: HashSet<String> = HashSet::new();
    let mut all_function_symbols: HashSet<String> = HashSet::new();
    let mut symbol_to_display_name: HashMap<String, String> = HashMap::new();

    // Pre-pass: Find where each symbol is DEFINED (symbol_roles == 1)
    // Collect ALL definition occurrences per symbol (there may be multiple for trait impls)
    // Maps symbol -> Vec<(path, line_number)>
    let mut symbol_to_definitions: HashMap<String, Vec<(String, i32)>> = HashMap::new();
    for doc in &scip_data.documents {
        let rel_path = doc.relative_path.trim_start_matches('/').to_string();
        for occurrence in &doc.occurrences {
            let is_definition = occurrence.symbol_roles.unwrap_or(0) & 1 == 1;
            if is_definition && !occurrence.range.is_empty() {
                let line = occurrence.range[0];
                symbol_to_definitions
                    .entry(occurrence.symbol.clone())
                    .or_default()
                    .push((rel_path.clone(), line));
            }
        }
    }

    // Sort definitions by line number for consistent matching with symbol entries
    for defs in symbol_to_definitions.values_mut() {
        defs.sort_by_key(|(_, line)| *line);
    }

    // First pass: identify all function symbols and handle duplicates
    // Track how many times we've seen each symbol to match with definition order
    let mut symbol_seen_count: HashMap<String, usize> = HashMap::new();

    for doc in &scip_data.documents {
        for symbol in &doc.symbols {
            if is_function_like(symbol.kind) {
                let signature = &symbol.signature_documentation.text;
                let display_name = symbol
                    .display_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());

                // Track ALL function symbols for dependency tracking
                all_function_symbols.insert(symbol.symbol.clone());
                symbol_to_display_name.insert(symbol.symbol.clone(), display_name.clone());

                // Create unique key using signature to handle duplicate symbols
                let unique_key = make_unique_key(&symbol.symbol, signature);

                // Get the nth definition for this symbol (matching symbol entry order with def order)
                let def_index = *symbol_seen_count.get(&symbol.symbol).unwrap_or(&0);
                symbol_seen_count
                    .entry(symbol.symbol.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);

                // Only add to call_graph if DEFINED in this project
                if let Some(defs) = symbol_to_definitions.get(&symbol.symbol) {
                    if let Some((rel_path, _line)) = defs.get(def_index) {
                        project_function_keys.insert(unique_key.clone());

                        call_graph.insert(
                            unique_key,
                            FunctionNode {
                                symbol: symbol.symbol.clone(),
                                display_name,
                                signature_text: signature.clone(),
                                relative_path: rel_path.clone(),
                                callees: HashSet::new(),
                                range: Vec::new(),
                            },
                        );
                    }
                }
            }
        }
    }

    // Build a map from (symbol, line) -> unique_key for occurrence processing
    let mut symbol_line_to_key: HashMap<(String, i32), String> = HashMap::new();
    for (key, node) in &call_graph {
        if let Some(defs) = symbol_to_definitions.get(&node.symbol) {
            // Find the definition line that matches this node's signature
            for (idx, node_in_graph) in call_graph.values().enumerate() {
                if node_in_graph.symbol == node.symbol {
                    if let Some((_, line)) = defs.get(idx) {
                        // This is a bit tricky - we need to match by signature
                        if node_in_graph.signature_text == node.signature_text {
                            symbol_line_to_key.insert((node.symbol.clone(), *line), key.clone());
                            break;
                        }
                    }
                }
            }
        }
    }

    // Rebuild the symbol_line_to_key map more correctly
    symbol_line_to_key.clear();
    let mut symbol_seen_for_lines: HashMap<String, usize> = HashMap::new();
    for doc in &scip_data.documents {
        for symbol in &doc.symbols {
            if is_function_like(symbol.kind) {
                let signature = &symbol.signature_documentation.text;
                let unique_key = make_unique_key(&symbol.symbol, signature);

                if call_graph.contains_key(&unique_key) {
                    let def_index = *symbol_seen_for_lines.get(&symbol.symbol).unwrap_or(&0);
                    symbol_seen_for_lines
                        .entry(symbol.symbol.clone())
                        .and_modify(|c| *c += 1)
                        .or_insert(1);

                    if let Some(defs) = symbol_to_definitions.get(&symbol.symbol) {
                        if let Some((_, line)) = defs.get(def_index) {
                            symbol_line_to_key.insert((symbol.symbol.clone(), *line), unique_key);
                        }
                    }
                }
            }
        }
    }

    // Second pass: build call relationships and extract ranges
    for doc in &scip_data.documents {
        let mut current_function_key: Option<String> = None;

        let mut ordered_occurrences = doc.occurrences.clone();
        ordered_occurrences.sort_by(|a, b| {
            let a_start = (a.range[0], a.range[1]);
            let b_start = (b.range[0], b.range[1]);
            a_start.cmp(&b_start)
        });

        for occurrence in &ordered_occurrences {
            let is_definition = occurrence.symbol_roles.unwrap_or(0) & 1 == 1;
            let line = if !occurrence.range.is_empty() {
                occurrence.range[0]
            } else {
                -1
            };

            // Track when we enter a project function definition
            if is_definition {
                // Look up the unique key for this (symbol, line) pair
                if let Some(key) = symbol_line_to_key.get(&(occurrence.symbol.clone(), line)) {
                    current_function_key = Some(key.clone());
                    if let Some(node) = call_graph.get_mut(key) {
                        node.range = occurrence.range.clone();
                    }
                }
            }

            // Track ALL function calls (including to external functions)
            // Note: References use the base symbol, not the unique key
            if !is_definition && all_function_symbols.contains(&occurrence.symbol) {
                if let Some(caller_key) = &current_function_key {
                    if let Some(caller_node) = call_graph.get_mut(caller_key) {
                        // For callees, we store the base symbol (not unique key)
                        // since references don't have signature info
                        if caller_node.symbol != occurrence.symbol {
                            caller_node.callees.insert(occurrence.symbol.clone());
                        }
                    }
                }
            }
        }
    }

    (call_graph, symbol_to_display_name)
}

/// Extract type parameter info from a signature for trait impls.
/// For example, from "fn mul(self, scalar: &Scalar) -> MontgomeryPoint"
/// extracts the self type and parameter types to help distinguish impls.
fn extract_impl_type_info(signature: &str) -> Option<String> {
    // Try to extract the self type and first param type
    // Pattern: "fn method(self, param: &Type) -> ..."
    // or "fn method(self: &SelfType, param: &Type) -> ..."

    let signature = signature.trim();

    // Look for the parameter list
    let params_start = signature.find('(')?;
    let params_end = signature.find(')')?;
    let params = &signature[params_start + 1..params_end];

    // Split by comma and look for typed self or first param after self
    let parts: Vec<&str> = params.split(',').map(|s| s.trim()).collect();

    if parts.len() >= 2 {
        // Get the type of the second parameter (first after self)
        let second_param = parts[1];
        if let Some(colon_pos) = second_param.find(':') {
            let type_part = second_param[colon_pos + 1..].trim();
            // Clean up the type (remove & and lifetime annotations)
            let clean_type = type_part
                .trim_start_matches('&')
                .trim_start_matches("'a ")
                .trim_start_matches("'_ ")
                .trim();
            return Some(clean_type.to_string());
        }
    }

    None
}

/// Convert symbol to a scip name, optionally including type info for disambiguation
fn symbol_to_scip_name(symbol: &str, display_name: &str, signature: Option<&str>) -> String {
    // Step 1: Strip "rust-analyzer cargo " prefix
    let s = symbol
        .strip_prefix("rust-analyzer cargo ")
        .unwrap_or_else(|| {
            panic!(
                "Symbol does not start with 'rust-analyzer cargo ': {}",
                symbol
            )
        });

    // Step 2 & 3: Check if s ends with "display_name()."
    let expected_suffix = format!("{}().", display_name);

    if !s.ends_with(&expected_suffix) {
        panic!("Symbol does not end with '{}': {}", expected_suffix, symbol);
    }

    // Delete the last character of s
    let mut result = s[..s.len() - 1].to_string();

    // If we have a signature, try to add type info for disambiguation
    // This helps distinguish e.g., Mul<&Scalar>::mul vs Mul<&MontgomeryPoint>::mul
    if let Some(sig) = signature {
        if let Some(type_info) = extract_impl_type_info(sig) {
            // Check if this looks like a trait method (contains #)
            // e.g., "4.1.3 montgomery/Mul#mul()"
            if result.contains('#') {
                // Insert the type parameter before the #
                // "montgomery/Mul#mul()" -> "montgomery/Mul<Scalar>#mul()"
                if let Some(hash_pos) = result.rfind('#') {
                    result = format!(
                        "{}<{}>{}",
                        &result[..hash_pos],
                        type_info,
                        &result[hash_pos..]
                    );
                }
            }
        }
    }

    result
}

/// Convert symbol to a path format with specified separator
fn symbol_to_path_with_sep(symbol: &str, display_name: &str, sep: &str) -> String {
    let mut s = symbol;
    let mut crate_name = String::new();

    // Skip "rust-analyzer cargo " prefix and extract crate name
    if let Some(rest) = symbol.strip_prefix("rust-analyzer cargo ") {
        s = rest;
        // Extract crate name (everything before the first space, which precedes the version)
        if let Some(space_pos) = s.find(' ') {
            crate_name = s[..space_pos].replace('-', "_");
            s = &s[space_pos + 1..]; // Move past crate name
        }
    }

    // Skip version part if present (e.g., "4.1.3 ")
    if let Some(pos) = s.find(|c: char| c.is_ascii_digit()) {
        if let Some(space_pos) = s[pos..].find(' ') {
            s = s[(pos + space_pos + 1)..].trim();
        }
    }

    let sep_char = sep.chars().next().unwrap_or('/');
    let mut clean_path = s
        .trim_end_matches('.')
        .replace('-', "_")
        .replace(['[', ']', '#'], sep)
        .replace('/', sep)
        .trim_end_matches(sep_char)
        .replace(&['`', '(', ')', '[', ']'][..], "");

    // Clean up double separators
    let double_sep = format!("{}{}", sep, sep);
    while clean_path.contains(&double_sep) {
        clean_path = clean_path.replace(&double_sep, sep);
    }

    // Remove angle-bracketed generics
    let re = regex::Regex::new(r"<[^>]*>").unwrap_or_else(|_| regex::Regex::new(r"").unwrap());
    clean_path = re.replace_all(&clean_path, "").to_string();

    // Clean up leading/trailing separators
    clean_path = clean_path
        .trim_matches(&sep.chars().collect::<Vec<_>>()[..])
        .to_string();

    // Add crate name prefix if we have one and it's not already there
    if !crate_name.is_empty() && !clean_path.starts_with(&crate_name) {
        clean_path = format!("{}{}{}", crate_name, sep, clean_path);
    }

    // Ensure the path ends with the display name
    if !clean_path.ends_with(display_name) {
        clean_path = format!("{}{}{}", clean_path, sep, display_name)
    }

    // Truncate if too long
    if clean_path.len() > 200 {
        clean_path.truncate(200);
    }

    clean_path
}

/// Convert symbol to Rust-style path with :: separators (for code-function field)
pub fn symbol_to_rust_path(symbol: &str, display_name: &str) -> String {
    symbol_to_path_with_sep(symbol, display_name, "::")
}

/// Convert symbol to slash-separated path (for dependencies)
pub fn symbol_to_path(symbol: &str, display_name: &str) -> String {
    symbol_to_path_with_sep(symbol, display_name, "/")
}

/// Convert call graph to atoms with line numbers format.
///
/// This version uses only SCIP data, which only provides the function NAME location,
/// so lines_start and lines_end will be the same (or close for multi-line spans).
/// For accurate function body spans, use `convert_to_atoms_with_parsed_spans` instead.
pub fn convert_to_atoms_with_lines(
    call_graph: &HashMap<String, FunctionNode>,
    symbol_to_display_name: &HashMap<String, String>,
) -> Vec<AtomWithLines> {
    convert_to_atoms_with_lines_internal(call_graph, symbol_to_display_name, None)
}

/// Convert call graph to atoms with accurate line numbers by parsing source files.
///
/// This version uses verus_syn to parse source files and get accurate function body spans.
pub fn convert_to_atoms_with_parsed_spans(
    call_graph: &HashMap<String, FunctionNode>,
    symbol_to_display_name: &HashMap<String, String>,
    project_root: &Path,
) -> Vec<AtomWithLines> {
    // Collect all unique relative paths
    let relative_paths: Vec<String> = call_graph
        .values()
        .map(|node| node.relative_path.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Build the span map by parsing all source files
    let span_map = verus_parser::build_function_span_map(project_root, &relative_paths);

    convert_to_atoms_with_lines_internal(call_graph, symbol_to_display_name, Some(&span_map))
}

/// Internal function that does the actual conversion.
fn convert_to_atoms_with_lines_internal(
    call_graph: &HashMap<String, FunctionNode>,
    symbol_to_display_name: &HashMap<String, String>,
    span_map: Option<&HashMap<(String, String, usize), usize>>,
) -> Vec<AtomWithLines> {
    call_graph
        .values()
        .map(|node| {
            let mut dependencies = HashSet::new();
            for callee in &node.callees {
                // Get display name from call_graph (project functions) or symbol map (all functions)
                let display_name = call_graph
                    .get(callee)
                    .map(|n| n.display_name.clone())
                    .or_else(|| symbol_to_display_name.get(callee).cloned())
                    .unwrap_or_else(|| "unknown".to_string());

                // For dependencies, we don't have signature info (they're just references)
                let dep_path = symbol_to_scip_name(callee, &display_name, None);
                dependencies.insert(dep_path);
            }

            // Get start line from SCIP range (0-based, convert to 1-based)
            let lines_start = if !node.range.is_empty() {
                node.range[0] as usize + 1
            } else {
                0
            };

            // Try to get accurate end line from parsed spans
            let lines_end = if let Some(map) = span_map {
                verus_parser::get_function_end_line(
                    map,
                    &node.relative_path,
                    &node.display_name,
                    lines_start,
                )
                .unwrap_or(lines_start) // Fallback to start line if not found
            } else {
                // Fallback: use SCIP range (which only covers the function name)
                match node.range.len() {
                    4 => node.range[2] as usize + 1,
                    _ => lines_start,
                }
            };

            AtomWithLines {
                display_name: node.display_name.clone(),
                // Include signature info for the scip_name to disambiguate trait impls
                scip_name: symbol_to_scip_name(
                    &node.symbol,
                    &node.display_name,
                    Some(&node.signature_text),
                ),
                dependencies,
                code_path: node.relative_path.clone(),
                code_text: CodeTextInfo {
                    lines_start,
                    lines_end,
                },
            }
        })
        .collect()
}
