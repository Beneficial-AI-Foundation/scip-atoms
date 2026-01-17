use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

pub mod constants;
pub mod error;
pub mod path_utils;
pub mod scip_cache;
pub mod verification;
pub mod verus_parser;

pub use error::{ProbeError, ProbeResult};

use constants::{
    is_definition, is_function_like_kind, PROBE_URI_PREFIX, SCIP_SYMBOL_PREFIX,
    TYPE_CONTEXT_LOOKBACK_LINES,
};

// =============================================================================
// Function Mode Enum
// =============================================================================

/// Verus function mode - indicates what kind of verification is performed.
///
/// - `Exec`: Executable code, compiled to native code and verified
/// - `Proof`: Proof code, verified but not compiled (erased at runtime)
/// - `Spec`: Specification code, defines logical properties (erased at runtime)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FunctionMode {
    #[default]
    Exec,
    Proof,
    Spec,
}

impl FunctionMode {
    /// Parse a function mode from a string.
    ///
    /// Accepts: "exec", "proof", "spec" (case-insensitive)
    /// Returns `Exec` for unrecognized values (the default mode).
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "proof" => FunctionMode::Proof,
            "spec" => FunctionMode::Spec,
            _ => FunctionMode::Exec,
        }
    }

    /// Convert to a string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            FunctionMode::Exec => "exec",
            FunctionMode::Proof => "proof",
            FunctionMode::Spec => "spec",
        }
    }
}

impl fmt::Display for FunctionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

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

/// A call from one function to another, with optional type context for disambiguation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CalleeInfo {
    /// The raw SCIP symbol of the callee
    pub symbol: String,
    /// Type hints found on the same line as the call (e.g., turbofish type parameters)
    /// Used to disambiguate calls to generic trait implementations
    pub type_hints: Vec<String>,
    /// Line number where the call occurs (0-based from SCIP)
    pub line: i32,
}

/// Location where a function call occurs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallLocation {
    /// Call in requires clause (precondition)
    Precondition,
    /// Call in ensures clause (postcondition)
    Postcondition,
    /// Call in function body
    Inner,
}

/// A dependency with its call location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyWithLocation {
    #[serde(rename = "code-name")]
    pub code_name: String,
    pub location: CallLocation,
    pub line: usize,
}

/// Function node in the call graph
#[derive(Debug, Clone)]
pub struct FunctionNode {
    pub symbol: String,
    pub display_name: String,
    pub signature_text: String,
    pub relative_path: String,
    /// Callees with their type context for disambiguation
    pub callees: HashSet<CalleeInfo>,
    pub range: Vec<i32>,
    /// The Self type for trait implementations, extracted from the `method().(self)` symbol.
    /// Used to repair verus-analyzer's inconsistent symbol format.
    /// e.g., "MontgomeryPoint" from "self: &MontgomeryPoint"
    pub self_type: Option<String>,
    /// Type context from the definition site (nearby type references).
    /// Used to disambiguate trait impls like `impl From<T> for Container<X>` vs `Container<Y>`.
    pub definition_type_context: Vec<String>,
}

/// Output format: Atom with line numbers
#[derive(Debug, Serialize, Deserialize)]
pub struct AtomWithLines {
    #[serde(rename = "display-name")]
    pub display_name: String,
    #[serde(skip_serializing)]
    pub code_name: String,
    /// Set of dependency code_names (for backward compatibility)
    pub dependencies: HashSet<String>,
    /// Dependencies with call location information (only included with --with-locations flag)
    #[serde(
        rename = "dependencies-with-locations",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub dependencies_with_locations: Vec<DependencyWithLocation>,
    #[serde(rename = "code-module")]
    pub code_module: String,
    #[serde(rename = "code-path")]
    pub code_path: String,
    #[serde(rename = "code-text")]
    pub code_text: CodeTextInfo,
    /// Verus function mode: exec, proof, or spec
    pub mode: FunctionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    is_function_like_kind(kind)
}

/// Create a unique key for a function by combining symbol, signature, self_type, and line number.
///
/// This handles multiple levels of potential collisions:
/// 1. Same symbol, different signature → distinguished by signature
/// 2. Same symbol & signature, different Self type → distinguished by self_type
/// 3. Same symbol, signature & self_type, different line → distinguished by line (fallback)
///
/// The line number fallback handles edge cases like:
/// ```text
/// impl<T> Marker<A> for X { fn mark(self) {} }  // line 10
/// impl<T> Marker<B> for X { fn mark(self) {} }  // line 20
/// ```
/// Where the trait type parameter doesn't appear in the method signature.
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
            if is_definition(occurrence.symbol_roles) && !occurrence.range.is_empty() {
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

    // Pre-pass: Collect type context for definitions (types near each definition line)
    // This helps disambiguate trait impls like `impl From<T> for Container<X>` vs `Container<Y>`
    // Maps (file_path, line) -> Vec<type_name>
    let mut definition_type_contexts: HashMap<(String, i32), Vec<String>> = HashMap::new();
    for doc in &scip_data.documents {
        let rel_path = doc.relative_path.trim_start_matches('/').to_string();

        // Collect all type references in this document
        let mut type_refs_by_line: HashMap<i32, Vec<String>> = HashMap::new();
        for occ in &doc.occurrences {
            if !is_definition(occ.symbol_roles)
                && !occ.range.is_empty()
                && occ.symbol.ends_with('#')
            {
                let line = occ.range[0];
                if let Some(type_name) = extract_type_name_from_symbol(&occ.symbol) {
                    type_refs_by_line.entry(line).or_default().push(type_name);
                }
            }
        }

        // For each definition line, collect types from nearby lines (within 5 lines before)
        for occ in &doc.occurrences {
            if is_definition(occ.symbol_roles) && !occ.range.is_empty() {
                let def_line = occ.range[0];
                let mut nearby_types = Vec::new();

                // Look at lines from def_line-N to def_line for type context
                for offset in 0..=TYPE_CONTEXT_LOOKBACK_LINES {
                    let check_line = def_line - offset;
                    if check_line >= 0 {
                        if let Some(types) = type_refs_by_line.get(&check_line) {
                            for t in types {
                                if !nearby_types.contains(t) {
                                    nearby_types.push(t.clone());
                                }
                            }
                        }
                    }
                }

                if !nearby_types.is_empty() {
                    definition_type_contexts.insert((rel_path.clone(), def_line), nearby_types);
                }
            }
        }
    }

    // Pre-pass: Collect self_type from `method().(self)` symbols
    // These have enclosing_symbol set and display_name == "self"
    // Since multiple trait impls can have the same symbol (verus-analyzer bug),
    // we collect all self_types per enclosing_symbol in order.
    // Maps enclosing_symbol -> Vec<self_type>
    let mut enclosing_to_self_types: HashMap<String, Vec<String>> = HashMap::new();
    for doc in &scip_data.documents {
        for symbol in &doc.symbols {
            // Look for self parameter symbols (display_name == "self" and has enclosing_symbol)
            if let Some(ref display_name) = symbol.display_name {
                if display_name == "self" {
                    if let Some(ref enclosing) = symbol.enclosing_symbol {
                        let self_sig = &symbol.signature_documentation.text;
                        if let Some(self_type) = extract_self_type(self_sig) {
                            enclosing_to_self_types
                                .entry(enclosing.clone())
                                .or_default()
                                .push(self_type);
                        }
                    }
                }
            }
        }
    }

    // Track how many times we've seen each symbol to pick the right self_type
    let mut symbol_self_type_idx: HashMap<String, usize> = HashMap::new();

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

                // Get the nth definition for this symbol (matching symbol entry order with def order)
                let def_index = *symbol_seen_count.get(&symbol.symbol).unwrap_or(&0);
                symbol_seen_count
                    .entry(symbol.symbol.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);

                // Look up self_type from the pre-collected map BEFORE creating unique key
                // Use the index to handle multiple impls with the same symbol
                let self_type =
                    if let Some(self_types) = enclosing_to_self_types.get(&symbol.symbol) {
                        let idx = *symbol_self_type_idx.get(&symbol.symbol).unwrap_or(&0);
                        symbol_self_type_idx
                            .entry(symbol.symbol.clone())
                            .and_modify(|i| *i += 1)
                            .or_insert(1);
                        self_types.get(idx).cloned()
                    } else {
                        None
                    };

                // Only add to call_graph if DEFINED in this project
                if let Some(defs) = symbol_to_definitions.get(&symbol.symbol) {
                    if let Some((rel_path, line)) = defs.get(def_index) {
                        // Create unique key using signature, self_type, AND line number
                        // This handles all collision cases:
                        // - Same symbol, different signature → distinguished by signature
                        // - Same symbol & signature, different Self type → distinguished by self_type
                        // - Same symbol, signature & self_type → distinguished by line (fallback)
                        let unique_key = make_unique_key(
                            &symbol.symbol,
                            signature,
                            self_type.as_deref(),
                            Some(*line),
                        );

                        project_function_keys.insert(unique_key.clone());

                        // Look up definition type context (types near this definition line)
                        let def_type_context = definition_type_contexts
                            .get(&(rel_path.clone(), *line))
                            .cloned()
                            .unwrap_or_default();

                        call_graph.insert(
                            unique_key,
                            FunctionNode {
                                symbol: symbol.symbol.clone(),
                                display_name,
                                signature_text: signature.clone(),
                                relative_path: rel_path.clone(),
                                callees: HashSet::new(),
                                range: Vec::new(),
                                self_type,
                                definition_type_context: def_type_context,
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
    let mut symbol_self_type_idx_for_lines: HashMap<String, usize> = HashMap::new();
    for doc in &scip_data.documents {
        for symbol in &doc.symbols {
            if is_function_like(symbol.kind) {
                let signature = &symbol.signature_documentation.text;

                // Get the definition index first so we can look up the line number
                let def_index = *symbol_seen_for_lines.get(&symbol.symbol).unwrap_or(&0);
                symbol_seen_for_lines
                    .entry(symbol.symbol.clone())
                    .and_modify(|c| *c += 1)
                    .or_insert(1);

                // Look up self_type (must match the same logic as the first pass)
                let self_type =
                    if let Some(self_types) = enclosing_to_self_types.get(&symbol.symbol) {
                        let idx = *symbol_self_type_idx_for_lines
                            .get(&symbol.symbol)
                            .unwrap_or(&0);
                        symbol_self_type_idx_for_lines
                            .entry(symbol.symbol.clone())
                            .and_modify(|i| *i += 1)
                            .or_insert(1);
                        self_types.get(idx).cloned()
                    } else {
                        None
                    };

                // Get line number from definitions
                if let Some(defs) = symbol_to_definitions.get(&symbol.symbol) {
                    if let Some((_, line)) = defs.get(def_index) {
                        let unique_key = make_unique_key(
                            &symbol.symbol,
                            signature,
                            self_type.as_deref(),
                            Some(*line),
                        );

                        if call_graph.contains_key(&unique_key) {
                            symbol_line_to_key.insert((symbol.symbol.clone(), *line), unique_key);
                        }
                    }
                }
            }
        }
    }

    // Second pass: build call relationships and extract ranges
    // Also collect type hints (symbols ending with #) for disambiguation
    for doc in &scip_data.documents {
        let mut current_function_key: Option<String> = None;

        let mut ordered_occurrences = doc.occurrences.clone();
        ordered_occurrences.sort_by(|a, b| {
            let a_start = (a.range[0], a.range[1]);
            let b_start = (b.range[0], b.range[1]);
            a_start.cmp(&b_start)
        });

        // Pre-collect type symbols per line for disambiguation
        // Type symbols are those ending with # (struct/type references)
        let mut line_to_type_hints: HashMap<i32, Vec<String>> = HashMap::new();
        for occ in &ordered_occurrences {
            if !is_definition(occ.symbol_roles) && !occ.range.is_empty() {
                let line = occ.range[0];
                // Check if this is a type reference (symbol ends with #)
                if occ.symbol.ends_with('#') {
                    // Extract just the type name from the symbol
                    // e.g., "rust-analyzer cargo ... curve_models/serial/backend/ProjectiveNielsPoint#"
                    // → "ProjectiveNielsPoint"
                    if let Some(type_name) = extract_type_name_from_symbol(&occ.symbol) {
                        line_to_type_hints.entry(line).or_default().push(type_name);
                    }
                }
            }
        }

        for occurrence in &ordered_occurrences {
            let is_def = is_definition(occurrence.symbol_roles);
            let line = if !occurrence.range.is_empty() {
                occurrence.range[0]
            } else {
                -1
            };

            // Track when we enter a project function definition
            if is_def {
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
            if !is_def && all_function_symbols.contains(&occurrence.symbol) {
                if let Some(caller_key) = &current_function_key {
                    if let Some(caller_node) = call_graph.get_mut(caller_key) {
                        // For callees, we store the base symbol with type hints
                        if caller_node.symbol != occurrence.symbol {
                            let type_hints =
                                line_to_type_hints.get(&line).cloned().unwrap_or_default();
                            caller_node.callees.insert(CalleeInfo {
                                symbol: occurrence.symbol.clone(),
                                type_hints,
                                line,
                            });
                        }
                    }
                }
            }
        }
    }

    (call_graph, symbol_to_display_name)
}

/// Extract the type name from a SCIP symbol ending with #
/// e.g., "rust-analyzer cargo curve25519-dalek 4.1.3 curve_models/serial/backend/ProjectiveNielsPoint#"
/// → "ProjectiveNielsPoint"
fn extract_type_name_from_symbol(symbol: &str) -> Option<String> {
    // Strip the trailing #
    let without_hash = symbol.trim_end_matches('#');
    // Get the last path component
    if let Some(last_slash) = without_hash.rfind('/') {
        let name = &without_hash[last_slash + 1..];
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Extract type parameter info from a signature for trait impls.
/// For example, from "fn mul(self, scalar: &Scalar) -> MontgomeryPoint"
/// extracts the self type and parameter types to help distinguish impls.
///
/// This function handles several patterns:
/// 1. Binary ops: `fn mul(self, rhs: &Scalar) -> ...` - extracts "Scalar" from second param
/// 2. From trait: `fn from(value: EdwardsPoint) -> ...` - extracts "EdwardsPoint" from first param
/// 3. Into trait: `fn into(self) -> RistrettoPoint` - extracts "RistrettoPoint" from return type
fn extract_impl_type_info(signature: &str) -> Option<String> {
    let signature = signature.trim();

    // Look for the parameter list
    let params_start = signature.find('(')?;
    let params_end = signature.find(')')?;
    let params = &signature[params_start + 1..params_end];

    // Split by comma and look for typed self or first param after self
    let parts: Vec<&str> = params.split(',').map(|s| s.trim()).collect();

    // Case 1: Two or more parameters (e.g., binary ops like Mul, Add)
    // Pattern: "fn method(self, param: &Type) -> ..."
    if parts.len() >= 2 {
        // Get the type of the second parameter (first after self)
        let second_param = parts[1];
        if let Some(type_str) = extract_type_from_param(second_param) {
            return Some(type_str);
        }
    }

    // Case 2: Single parameter that is NOT self (e.g., From::from)
    // Pattern: "fn from(value: SourceType) -> ..."
    if parts.len() == 1 {
        let first_param = parts[0].trim();
        // Skip if it's just "self" or "self: Type" (not a From-like method)
        if !first_param.is_empty() && !first_param.starts_with("self") && first_param.contains(':')
        {
            if let Some(type_str) = extract_type_from_param(first_param) {
                return Some(type_str);
            }
        }
    }

    // Case 3: No parameters or just self - try to extract from return type (e.g., Into::into)
    // Pattern: "fn into(self) -> TargetType"
    if let Some(arrow_pos) = signature.find("->") {
        let return_type = signature[arrow_pos + 2..].trim();
        // Clean up the return type
        let clean_return = clean_type_string(return_type);
        // Only use return type for disambiguation if it's a concrete type (not Self)
        if !clean_return.is_empty() && clean_return != "Self" {
            return Some(clean_return);
        }
    }

    None
}

/// Extract and clean a type from a parameter declaration like "param: &Type" or "param: Type"
/// Preserves the `&` to distinguish reference vs owned types.
fn extract_type_from_param(param: &str) -> Option<String> {
    let colon_pos = param.find(':')?;
    let type_part = param[colon_pos + 1..].trim();
    let clean = clean_type_string_preserve_ref(type_part);
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

/// Clean up a type string by removing lifetimes but PRESERVING the reference marker (&).
/// This is important for distinguishing `impl From<&T>` from `impl From<T>`.
fn clean_type_string_preserve_ref(type_str: &str) -> String {
    let type_str = type_str.trim();

    // Check if it's a reference type
    let is_ref = type_str.starts_with('&');

    // Remove the & temporarily to clean up lifetimes
    let without_ref = type_str.trim_start_matches('&').trim();

    // Remove lifetime annotations
    let clean = without_ref
        .trim_start_matches("'a ")
        .trim_start_matches("'b ")
        .trim_start_matches("'_ ")
        .trim_start_matches("mut ")
        .trim();

    if clean.is_empty() {
        String::new()
    } else if is_ref {
        // Re-add the & for reference types
        format!("&{}", clean)
    } else {
        clean.to_string()
    }
}

/// Clean up a type string by removing references, lifetimes, and whitespace
/// Used for return types where we don't care about reference distinction.
fn clean_type_string(type_str: &str) -> String {
    type_str
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("'a ")
        .trim_start_matches("'b ")
        .trim_start_matches("'_ ")
        .trim_start_matches("mut ")
        .trim()
        .to_string()
}

/// Extract the Self type from a self parameter signature.
/// For example, from "self: &MontgomeryPoint" extracts "&MontgomeryPoint".
/// From "self: Scalar" extracts "Scalar".
/// Preserves the `&` to distinguish owned vs reference implementations,
/// matching rust-analyzer's behavior.
fn extract_self_type(self_signature: &str) -> Option<String> {
    // Pattern: "self: &Type" or "self: &'a Type" or "self: Type"
    let self_signature = self_signature.trim();

    if let Some(colon_pos) = self_signature.find(':') {
        let type_part = self_signature[colon_pos + 1..].trim();

        // Check if it's a reference type
        let is_ref = type_part.starts_with('&');

        // Remove lifetime annotations but preserve the & if present
        let clean_type = type_part
            .trim_start_matches('&')
            .trim_start_matches("'a ")
            .trim_start_matches("'b ")
            .trim_start_matches("'_ ")
            .trim();

        if !clean_type.is_empty() {
            // Re-add the & if it was a reference type
            if is_ref {
                return Some(format!("&{}", clean_type));
            } else {
                return Some(clean_type.to_string());
            }
        }
    }

    None
}

/// Check if a symbol path is missing the Self type (verus-analyzer inconsistency).
/// verus-analyzer produces "module/Trait#method()" for reference Self types,
/// but "module/Type#Trait#method()" for owned Self types.
/// This function detects the former pattern.
fn is_missing_self_type(symbol: &str) -> bool {
    // Pattern for missing Self type: "module/Trait#method()" where Trait is capitalized
    // Pattern for present Self type: "module/Type#Trait#method()" has two # separators

    // Count the number of # in the symbol
    let hash_count = symbol.matches('#').count();

    // If there's only one #, and it's followed by a method name, Self type is likely missing
    // e.g., "montgomery/Mul#mul()" vs "montgomery/MontgomeryPoint#Mul#mul()"
    hash_count == 1
}

/// Extract the module path from a probe_name.
///
/// Given a probe_name like "probe:curve25519-dalek/4.1.3/montgomery/MontgomeryPoint#ct_eq()",
/// extracts the module path (everything between version and the type name).
///
/// Example: "probe:curve25519-dalek/4.1.3/montgomery/MontgomeryPoint#ct_eq()" -> "montgomery"
/// Example: "probe:crate/0.1.0/foo/bar/Baz#method()" -> "foo/bar"
/// Example: "probe:crate/0.1.0/TopLevel#method()" -> "" (no module path)
fn extract_code_module(probe_name: &str) -> String {
    // Strip "probe:" prefix
    let s = probe_name
        .strip_prefix(PROBE_URI_PREFIX)
        .unwrap_or(probe_name);

    // Find the position of "#" which marks the type/method boundary
    let hash_pos = s.find('#').unwrap_or(s.len());
    let before_hash = &s[..hash_pos];

    // Find positions of "/" to skip crate and version
    let slashes: Vec<usize> = before_hash.match_indices('/').map(|(i, _)| i).collect();

    // Need at least 2 slashes (after crate, after version)
    // If there's a 3rd slash, there's a module path
    if slashes.len() < 3 {
        return String::new();
    }

    // Module path is between second slash (after version) and last slash (before type)
    let start = slashes[1] + 1;
    let end = slashes[slashes.len() - 1];

    if start < end {
        before_hash[start..end].to_string()
    } else {
        String::new()
    }
}

/// Convert symbol to a scip name, optionally including type info for disambiguation.
///
/// Parameters:
/// - `symbol`: The raw SCIP symbol string
/// - `display_name`: The function/method name
/// - `signature`: Optional function signature (e.g., "fn mul(self, scalar: &Scalar) -> MontgomeryPoint")
/// - `self_type`: Optional Self type extracted from the self parameter (e.g., "MontgomeryPoint")
///
/// This function repairs verus-analyzer's inconsistent symbol format by:
/// 1. Adding trait type parameters (e.g., `Mul` -> `Mul<Scalar>`) for disambiguation
/// 2. Adding the Self type when missing (e.g., `montgomery/Mul#mul` -> `montgomery/MontgomeryPoint#Mul#mul`)
/// 3. Adding line number suffix when type info alone can't disambiguate (e.g., generic impls)
fn symbol_to_code_name(
    symbol: &str,
    display_name: &str,
    signature: Option<&str>,
    self_type: Option<&str>,
) -> String {
    symbol_to_code_name_with_line(symbol, display_name, signature, self_type, None)
}

/// Convert symbol to scip name, with optional line number for disambiguation.
fn symbol_to_code_name_with_line(
    symbol: &str,
    display_name: &str,
    signature: Option<&str>,
    self_type: Option<&str>,
    line_number: Option<usize>,
) -> String {
    symbol_to_code_name_full(
        symbol,
        display_name,
        signature,
        self_type,
        line_number,
        None,
    )
    .unwrap_or_else(|e| {
        // Log warning and return a fallback name using the raw symbol
        eprintln!("Warning: {}", e);
        format!(
            "{}{}",
            PROBE_URI_PREFIX,
            symbol.replace("rust-analyzer cargo ", "").replace(' ', "/")
        )
    })
}

/// Convert symbol to scip name with full disambiguation options.
///
/// # Arguments
/// * `symbol` - The raw SCIP symbol
/// * `display_name` - The function's display name
/// * `signature` - Optional signature text for type extraction
/// * `self_type` - Optional Self type for trait impls
/// * `line_number` - Optional line number (last resort disambiguation)
/// * `target_type` - Optional target type parameter for generic impls (e.g., "ProjectiveNielsPoint")
///
/// # Returns
/// Returns `Ok(String)` with the formatted scip name, or `Err(ProbeError)` if the symbol
/// format is invalid.
fn symbol_to_code_name_full(
    symbol: &str,
    display_name: &str,
    signature: Option<&str>,
    self_type: Option<&str>,
    line_number: Option<usize>,
    target_type: Option<&str>,
) -> Result<String, ProbeError> {
    // Step 1: Strip "rust-analyzer cargo " prefix
    let s = symbol.strip_prefix(SCIP_SYMBOL_PREFIX).ok_or_else(|| {
        ProbeError::invalid_symbol(
            format!("Symbol does not start with '{}'", SCIP_SYMBOL_PREFIX),
            symbol,
        )
    })?;

    // Step 2 & 3: Check if s ends with "display_name()."
    let expected_suffix = format!("{}().", display_name);

    if !s.ends_with(&expected_suffix) {
        return Err(ProbeError::invalid_symbol(
            format!("Symbol does not end with '{}'", expected_suffix),
            symbol,
        ));
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

    // If Self type is provided and the symbol is missing it (verus-analyzer inconsistency),
    // insert the Self type to make it consistent with rust-analyzer format.
    // e.g., "montgomery/Mul<Scalar>#mul()" -> "montgomery/MontgomeryPoint#Mul<Scalar>#mul()"
    if let Some(self_t) = self_type {
        if is_missing_self_type(&result) {
            // Find the position after "module/" to insert the Self type
            // Pattern: "version module/Trait#method()" or "version module/Trait<T>#method()"
            if let Some(slash_pos) = result.rfind('/') {
                // Insert Self type after the slash, before the trait
                let before_slash = &result[..=slash_pos];
                let after_slash = &result[slash_pos + 1..];
                result = format!("{}{}#{}", before_slash, self_t, after_slash);
            }
        }
    }

    // If target_type is provided, add it as a type parameter to the struct name.
    // This enriches the symbol to be more like rust-analyzer's format.
    // e.g., "window/NafLookupTable5#From<&EdwardsPoint>#from()"
    //    -> "window/NafLookupTable5<ProjectiveNielsPoint>#From<&EdwardsPoint>#from()"
    if let Some(target_t) = target_type {
        // Find the struct name (first # after the module path)
        // Pattern: "version module/StructName#Trait..." or "version module/StructName#Trait<T>#..."
        if let Some(first_hash) = result.find('#') {
            // Check if there's already a type parameter before this #
            let before_hash = &result[..first_hash];
            if !before_hash.ends_with('>') {
                // No existing type parameter, add one
                result = format!("{}<{}>{}", before_hash, target_t, &result[first_hash..]);
            }
        }
    }

    // If line number is provided (and no target_type), add it as a suffix for disambiguation.
    // This is a last resort for cases where symbol+signature+self_type are all identical
    // (e.g., generic trait impls that differ only in type parameters not in the signature).
    if let Some(line) = line_number {
        result = format!("{}@{}", result, line);
    }

    // Convert to probe: URI format
    // "curve25519-dalek 4.1.3 montgomery/MontgomeryPoint#ct_eq()"
    // becomes "probe:curve25519-dalek/4.1.3/montgomery/MontgomeryPoint#ct_eq()"
    Ok(format!("{}{}", PROBE_URI_PREFIX, result.replace(' ', "/")))
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
    convert_to_atoms_with_lines_internal(call_graph, symbol_to_display_name, None, false)
}

/// Convert call graph to atoms with accurate line numbers by parsing source files.
///
/// This version uses verus_syn to parse source files and get accurate function body spans.
pub fn convert_to_atoms_with_parsed_spans(
    call_graph: &HashMap<String, FunctionNode>,
    symbol_to_display_name: &HashMap<String, String>,
    project_root: &Path,
    with_locations: bool,
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

    convert_to_atoms_with_lines_internal(
        call_graph,
        symbol_to_display_name,
        Some(&span_map),
        with_locations,
    )
}

/// Internal function that does the actual conversion.
/// Uses a multi-pass approach:
/// 1. Compute final code_names for all atoms (with line numbers for duplicates)
/// 2. Build a map: raw_symbol → list of final_code_names
/// 3. Resolve dependencies using the map (include all matches for ambiguous refs)
fn convert_to_atoms_with_lines_internal(
    call_graph: &HashMap<String, FunctionNode>,
    symbol_to_display_name: &HashMap<String, String>,
    span_map: Option<&HashMap<(String, String, usize), verus_parser::SpanAndMode>>,
    with_locations: bool,
) -> Vec<AtomWithLines> {
    // === Phase 1: Compute line ranges and base code_names for all nodes ===
    struct NodeData<'a> {
        node: &'a FunctionNode,
        lines_start: usize,
        lines_end: usize,
        base_code_name: String,
        mode: FunctionMode,
        /// Line range of requires clause, if present
        requires_range: Option<(usize, usize)>,
        /// Line range of ensures clause, if present
        ensures_range: Option<(usize, usize)>,
    }

    let node_data: Vec<NodeData> = call_graph
        .values()
        .map(|node| {
            let lines_start = if !node.range.is_empty() {
                node.range[0] as usize + 1
            } else {
                0
            };

            let lines_end = if let Some(map) = span_map {
                verus_parser::get_function_end_line(
                    map,
                    &node.relative_path,
                    &node.display_name,
                    lines_start,
                )
                .unwrap_or(lines_start)
            } else {
                match node.range.len() {
                    4 => node.range[2] as usize + 1,
                    _ => lines_start,
                }
            };

            // Get mode from span_map (defaults to Exec if not found)
            let mode = if let Some(map) = span_map {
                verus_parser::get_function_mode(
                    map,
                    &node.relative_path,
                    &node.display_name,
                    lines_start,
                )
                .unwrap_or(FunctionMode::Exec)
            } else {
                FunctionMode::Exec
            };

            // Get spec ranges (requires/ensures line ranges)
            let (requires_range, ensures_range) = if let Some(map) = span_map {
                verus_parser::get_function_spec_ranges(
                    map,
                    &node.relative_path,
                    &node.display_name,
                    lines_start,
                )
            } else {
                (None, None)
            };

            // Generate base code_name WITHOUT line number
            let base_code_name = symbol_to_code_name(
                &node.symbol,
                &node.display_name,
                Some(&node.signature_text),
                node.self_type.as_deref(),
            );

            NodeData {
                node,
                lines_start,
                lines_end,
                base_code_name,
                mode,
                requires_range,
                ensures_range,
            }
        })
        .collect();

    // === Phase 2: Detect duplicates and compute final code_names ===
    let mut code_name_count: HashMap<String, usize> = HashMap::new();
    for data in &node_data {
        *code_name_count
            .entry(data.base_code_name.clone())
            .or_insert(0) += 1;
    }

    // For disambiguation, we need to find "discriminating" types that uniquely identify each impl
    // Group nodes by their base_code_name to find duplicates
    let mut code_name_to_nodes: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, data) in node_data.iter().enumerate() {
        code_name_to_nodes
            .entry(&data.base_code_name)
            .or_default()
            .push(idx);
    }

    // For each group of duplicates, find which types are discriminating
    // (appear in some but not all impls of the same base_code_name)
    let mut node_discriminating_type: HashMap<usize, Option<String>> = HashMap::new();
    for indices in code_name_to_nodes.values() {
        if indices.len() <= 1 {
            // Not a duplicate, no disambiguation needed
            for &idx in indices {
                node_discriminating_type.insert(idx, None);
            }
            continue;
        }

        // Collect all type contexts for this group
        let all_contexts: Vec<&Vec<String>> = indices
            .iter()
            .map(|&idx| &node_data[idx].node.definition_type_context)
            .collect();

        // Find types that appear in exactly one context (discriminating)
        let mut type_counts: HashMap<&str, usize> = HashMap::new();
        for ctx in &all_contexts {
            for t in *ctx {
                *type_counts.entry(t.as_str()).or_insert(0) += 1;
            }
        }

        // For each node in this group, find a discriminating type
        for &idx in indices {
            let ctx = &node_data[idx].node.definition_type_context;
            // Find a type that appears only in this node's context
            let discriminating = ctx
                .iter()
                .find(|t| type_counts.get(t.as_str()).copied().unwrap_or(0) == 1);
            node_discriminating_type.insert(idx, discriminating.cloned());
        }
    }

    // Compute final code_name for each node
    let final_code_names: Vec<String> = node_data
        .iter()
        .enumerate()
        .map(|(idx, data)| {
            let is_duplicate = code_name_count
                .get(&data.base_code_name)
                .copied()
                .unwrap_or(0)
                > 1;

            if is_duplicate {
                // Try to use discriminating type first, fall back to line number
                let result = if let Some(Some(target_type)) = node_discriminating_type.get(&idx) {
                    symbol_to_code_name_full(
                        &data.node.symbol,
                        &data.node.display_name,
                        Some(&data.node.signature_text),
                        data.node.self_type.as_deref(),
                        None, // No line number needed
                        Some(target_type),
                    )
                } else if data.lines_start > 0 {
                    // Fall back to line number if no discriminating type found
                    symbol_to_code_name_full(
                        &data.node.symbol,
                        &data.node.display_name,
                        Some(&data.node.signature_text),
                        data.node.self_type.as_deref(),
                        Some(data.lines_start),
                        None,
                    )
                } else {
                    Ok(data.base_code_name.clone())
                };
                result.unwrap_or_else(|e| {
                    eprintln!("Warning: {}", e);
                    data.base_code_name.clone()
                })
            } else {
                data.base_code_name.clone()
            }
        })
        .collect();

    // === Phase 3: Build map from raw symbol → list of (code_name, type_context) ===
    // The type_context helps match call-site type hints to the correct implementation
    struct CodeNameWithContext {
        code_name: String,
        /// Types from definition site (nearby type references) for disambiguation
        type_context: Vec<String>,
    }

    let mut raw_symbol_to_code_names: HashMap<String, Vec<CodeNameWithContext>> = HashMap::new();
    for (data, final_name) in node_data.iter().zip(final_code_names.iter()) {
        // Use definition_type_context from FunctionNode (captured during build_call_graph)
        // This contains types that appeared near the definition, like "ProjectiveNielsPoint"
        let type_context = data.node.definition_type_context.clone();

        raw_symbol_to_code_names
            .entry(data.node.symbol.clone())
            .or_default()
            .push(CodeNameWithContext {
                code_name: final_name.clone(),
                type_context,
            });
    }

    // Helper to classify call location based on line number and spec ranges
    fn classify_call_location(
        call_line: i32,
        requires_range: Option<(usize, usize)>,
        ensures_range: Option<(usize, usize)>,
    ) -> CallLocation {
        // SCIP uses 0-based lines, verus_syn uses 1-based - convert
        let call_line_1based = (call_line + 1) as usize;

        if let Some((start, end)) = requires_range {
            if call_line_1based >= start && call_line_1based <= end {
                return CallLocation::Precondition;
            }
        }

        if let Some((start, end)) = ensures_range {
            if call_line_1based >= start && call_line_1based <= end {
                return CallLocation::Postcondition;
            }
        }

        CallLocation::Inner
    }

    // === Phase 4: Build final atoms with resolved dependencies ===
    node_data
        .into_iter()
        .zip(final_code_names)
        .map(|(data, code_name)| {
            // Resolve dependencies: map raw symbols to their full code_names
            let mut dependencies = HashSet::new();
            let mut dependencies_with_locations: Vec<DependencyWithLocation> = Vec::new();

            for callee in &data.node.callees {
                // Only compute location info if requested (for --with-locations flag)
                let (location, call_line_1based) = if with_locations {
                    let loc = classify_call_location(
                        callee.line,
                        data.requires_range,
                        data.ensures_range,
                    );
                    let line = (callee.line + 1) as usize;
                    (Some(loc), line)
                } else {
                    (None, 0)
                };

                // Check if this callee is a project function with known code_names
                if let Some(code_name_contexts) = raw_symbol_to_code_names.get(&callee.symbol) {
                    if code_name_contexts.len() == 1 {
                        // Only one implementation - use it directly
                        let dep_code_name = code_name_contexts[0].code_name.clone();
                        dependencies.insert(dep_code_name.clone());
                        if let Some(loc) = location.clone() {
                            dependencies_with_locations.push(DependencyWithLocation {
                                code_name: dep_code_name,
                                location: loc,
                                line: call_line_1based,
                            });
                        }
                    } else if !callee.type_hints.is_empty() {
                        // Multiple implementations - try to match using type hints
                        // First, find types in call-site hints that DON'T appear in ALL impl contexts
                        // (i.e., discriminating types like ProjectiveNielsPoint vs AffineNielsPoint)
                        let discriminating_hints: Vec<_> = callee
                            .type_hints
                            .iter()
                            .filter(|hint| {
                                // Count how many impls have this type in their context
                                let matching_count = code_name_contexts
                                    .iter()
                                    .filter(|ctx| ctx.type_context.iter().any(|t| t == *hint))
                                    .count();
                                // Keep hints that match some but not all impls
                                matching_count > 0 && matching_count < code_name_contexts.len()
                            })
                            .collect();

                        let matched: Vec<_> = if !discriminating_hints.is_empty() {
                            // Use discriminating hints to filter
                            code_name_contexts
                                .iter()
                                .filter(|ctx| {
                                    discriminating_hints
                                        .iter()
                                        .any(|hint| ctx.type_context.iter().any(|t| t == *hint))
                                })
                                .collect()
                        } else {
                            // Fallback: use all hints
                            code_name_contexts
                                .iter()
                                .filter(|ctx| {
                                    callee.type_hints.iter().any(|hint| {
                                        ctx.type_context
                                            .iter()
                                            .any(|t| t.contains(hint) || hint.contains(t))
                                    })
                                })
                                .collect()
                        };

                        if matched.len() == 1 {
                            // Found exactly one match - use it
                            let dep_code_name = matched[0].code_name.clone();
                            dependencies.insert(dep_code_name.clone());
                            if let Some(loc) = location.clone() {
                                dependencies_with_locations.push(DependencyWithLocation {
                                    code_name: dep_code_name,
                                    location: loc,
                                    line: call_line_1based,
                                });
                            }
                        } else {
                            // Still ambiguous - include all
                            for ctx in code_name_contexts {
                                dependencies.insert(ctx.code_name.clone());
                                if let Some(loc) = location.clone() {
                                    dependencies_with_locations.push(DependencyWithLocation {
                                        code_name: ctx.code_name.clone(),
                                        location: loc,
                                        line: call_line_1based,
                                    });
                                }
                            }
                        }
                    } else {
                        // No type hints - include all possible implementations
                        for ctx in code_name_contexts {
                            dependencies.insert(ctx.code_name.clone());
                            if let Some(loc) = location.clone() {
                                dependencies_with_locations.push(DependencyWithLocation {
                                    code_name: ctx.code_name.clone(),
                                    location: loc,
                                    line: call_line_1based,
                                });
                            }
                        }
                    }
                } else {
                    // External function - use the raw symbol conversion
                    let display_name = symbol_to_display_name
                        .get(&callee.symbol)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    let dep_path = symbol_to_code_name(&callee.symbol, &display_name, None, None);
                    dependencies.insert(dep_path.clone());
                    if let Some(loc) = location {
                        dependencies_with_locations.push(DependencyWithLocation {
                            code_name: dep_path,
                            location: loc,
                            line: call_line_1based,
                        });
                    }
                }
            }

            let code_module = extract_code_module(&code_name);
            AtomWithLines {
                display_name: data.node.display_name.clone(),
                code_name,
                dependencies,
                dependencies_with_locations,
                code_module,
                code_path: data.node.relative_path.clone(),
                code_text: CodeTextInfo {
                    lines_start: data.lines_start,
                    lines_end: data.lines_end,
                },
                mode: data.mode,
            }
        })
        .collect()
}

/// Information about a duplicate code_name
#[derive(Debug, Clone)]
pub struct DuplicateCodeName {
    pub code_name: String,
    pub occurrences: Vec<DuplicateOccurrence>,
}

#[derive(Debug, Clone)]
pub struct DuplicateOccurrence {
    pub display_name: String,
    pub code_path: String,
    pub lines_start: usize,
}

/// Check for duplicate code_names in the atoms output.
/// Returns a list of code_names that appear more than once.
///
/// This is useful for detecting cases where the disambiguation logic fails,
/// such as trait implementations that can't be distinguished by signature alone.
pub fn find_duplicate_code_names(atoms: &[AtomWithLines]) -> Vec<DuplicateCodeName> {
    let mut code_name_to_atoms: HashMap<String, Vec<&AtomWithLines>> = HashMap::new();

    for atom in atoms {
        code_name_to_atoms
            .entry(atom.code_name.clone())
            .or_default()
            .push(atom);
    }

    code_name_to_atoms
        .into_iter()
        .filter(|(_, atoms)| atoms.len() > 1)
        .map(|(code_name, atoms)| DuplicateCodeName {
            code_name,
            occurrences: atoms
                .into_iter()
                .map(|a| DuplicateOccurrence {
                    display_name: a.display_name.clone(),
                    code_path: a.code_path.clone(),
                    lines_start: a.code_text.lines_start,
                })
                .collect(),
        })
        .collect()
}
