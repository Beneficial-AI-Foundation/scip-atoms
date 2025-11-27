use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    pub relative_path: String,
    pub callees: HashSet<String>,
    pub range: Vec<i32>,
}

/// Output format: Atom with line numbers
#[derive(Debug, Serialize, Deserialize)]
pub struct AtomWithLines {
    #[serde(rename = "display-name")]
    pub display_name: String,
    pub visible: bool,
    pub dependencies: HashMap<String, DependencyInfo>,
    #[serde(rename = "code-path")]
    pub code_path: String,
    #[serde(rename = "code-function")]
    pub code_function: String,
    #[serde(rename = "code-text")]
    pub code_text: CodeTextInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DependencyInfo {
    pub visible: bool,
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

/// Build a call graph from SCIP data
pub fn build_call_graph(scip_data: &ScipIndex) -> HashMap<String, FunctionNode> {
    let mut call_graph: HashMap<String, FunctionNode> = HashMap::new();
    let mut function_symbols: HashSet<String> = HashSet::new();
    let mut symbol_to_display_name: HashMap<String, String> = HashMap::new();

    // Pre-pass: Find where each symbol is DEFINED (symbol_roles == 1)
    // This is the authoritative source for file paths, not the symbols array
    let mut symbol_to_def_path: HashMap<String, String> = HashMap::new();
    for doc in &scip_data.documents {
        let rel_path = doc.relative_path.trim_start_matches('/').to_string();
        for occurrence in &doc.occurrences {
            let is_definition = occurrence.symbol_roles.unwrap_or(0) & 1 == 1;
            if is_definition {
                symbol_to_def_path.insert(occurrence.symbol.clone(), rel_path.clone());
            }
        }
    }

    // First pass: identify all function symbols
    for doc in &scip_data.documents {
        for symbol in &doc.symbols {
            if is_function_like(symbol.kind) {
                function_symbols.insert(symbol.symbol.clone());
                symbol_to_display_name.insert(
                    symbol.symbol.clone(),
                    symbol.display_name.clone().unwrap_or_else(|| "unknown".to_string()),
                );

                // Use the DEFINITION location if available, otherwise fall back to symbols array location
                let rel_path = symbol_to_def_path
                    .get(&symbol.symbol)
                    .cloned()
                    .unwrap_or_else(|| doc.relative_path.trim_start_matches('/').to_string());

                call_graph.insert(
                    symbol.symbol.clone(),
                    FunctionNode {
                        symbol: symbol.symbol.clone(),
                        display_name: symbol
                            .display_name
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        relative_path: rel_path,
                        callees: HashSet::new(),
                        range: Vec::new(),
                    },
                );
            }
        }
    }

    // Second pass: build call relationships and extract ranges
    for doc in &scip_data.documents {
        let mut current_function: Option<String> = None;

        let mut ordered_occurrences = doc.occurrences.clone();
        ordered_occurrences.sort_by(|a, b| {
            let a_start = (a.range[0], a.range[1]);
            let b_start = (b.range[0], b.range[1]);
            a_start.cmp(&b_start)
        });

        for occurrence in &ordered_occurrences {
            let is_definition = occurrence.symbol_roles.unwrap_or(0) & 1 == 1;

            if is_definition && function_symbols.contains(&occurrence.symbol) {
                current_function = Some(occurrence.symbol.clone());
                if let Some(node) = call_graph.get_mut(&occurrence.symbol) {
                    node.range = occurrence.range.clone();
                }
            }

            if !is_definition && function_symbols.contains(&occurrence.symbol) {
                if let Some(caller) = &current_function {
                    if caller != &occurrence.symbol {
                        if let Some(caller_node) = call_graph.get_mut(caller) {
                            caller_node.callees.insert(occurrence.symbol.clone());
                        }
                    }
                }
            }
        }
    }

    call_graph
}

/// Convert symbol to a clean path format
pub fn symbol_to_path(symbol: &str, display_name: &str) -> String {
    let mut s = symbol;
    
    // Skip "rust-analyzer cargo " prefix if present
    if let Some(rest) = symbol.strip_prefix("rust-analyzer cargo ") {
        s = rest;
    }

    // Skip version part if present
    if let Some(pos) = s.find(|c: char| c.is_ascii_digit()) {
        if let Some(space_pos) = s[pos..].find(' ') {
            s = s[(pos + space_pos + 1)..].trim();
        }
    }

    let mut clean_path = s
        .trim_end_matches('.')
        .replace('-', "_")
        .replace(['[', ']', '#'], "/")
        .trim_end_matches('/')
        .replace(&['`', '(', ')', '[', ']'][..], "")
        .replace("//", "/");

    // Remove angle-bracketed generics
    let re = regex::Regex::new(r"<[^>]*>").unwrap_or_else(|_| regex::Regex::new(r"").unwrap());
    clean_path = re.replace_all(&clean_path, "").to_string();

    if !clean_path.ends_with(display_name) {
        clean_path = format!("{clean_path}/{display_name}")
    }

    if clean_path.len() > 128 {
        clean_path.truncate(128);
    }

    clean_path
}

/// Convert call graph to atoms with line numbers format
pub fn convert_to_atoms_with_lines(call_graph: &HashMap<String, FunctionNode>) -> Vec<AtomWithLines> {
    call_graph
        .values()
        .map(|node| {
            let mut dependencies = HashMap::new();
            for callee in &node.callees {
                if let Some(callee_node) = call_graph.get(callee) {
                    let dep_path = symbol_to_path(&callee_node.symbol, &callee_node.display_name);
                    dependencies.insert(dep_path, DependencyInfo { visible: true });
                }
            }

            // Extract line numbers from SCIP range [start_line, start_col, end_line, end_col]
            let (lines_start, lines_end) = if node.range.len() >= 3 {
                let start = node.range[0] as usize + 1; // Convert to 1-based
                let end = node.range[2] as usize + 1;
                (start, end)
            } else {
                (0, 0)
            };

            AtomWithLines {
                display_name: node.display_name.clone(),
                visible: true,
                dependencies,
                code_path: node.relative_path.clone(),
                code_function: symbol_to_path(&node.symbol, &node.display_name),
                code_text: CodeTextInfo {
                    lines_start,
                    lines_end,
                },
            }
        })
        .collect()
}

