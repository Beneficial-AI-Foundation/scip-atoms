//! Parser module using verus_syn to extract accurate function spans.
//!
//! SCIP only provides the location of function names, not their full body spans.
//! This module parses the actual source files to get accurate start/end line numbers.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use verus_syn::spanned::Spanned;
use verus_syn::visit::Visit;
use verus_syn::{ImplItemFn, Item, ItemFn, ItemMacro, TraitItemFn};

/// Function span information
#[derive(Debug, Clone)]
pub struct FunctionSpan {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Visitor that collects function spans from an AST
struct FunctionSpanVisitor {
    functions: Vec<FunctionSpan>,
}

impl FunctionSpanVisitor {
    fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for FunctionSpanVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        let start_line = span.start().line;
        let end_line = span.end().line;

        self.functions.push(FunctionSpan {
            name,
            start_line,
            end_line,
        });

        // Continue visiting nested items
        verus_syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        let start_line = span.start().line;
        let end_line = span.end().line;

        self.functions.push(FunctionSpan {
            name,
            start_line,
            end_line,
        });

        // Continue visiting nested items
        verus_syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        let start_line = span.start().line;
        let end_line = span.end().line;

        self.functions.push(FunctionSpan {
            name,
            start_line,
            end_line,
        });

        // Continue visiting nested items
        verus_syn::visit::visit_trait_item_fn(self, node);
    }

    // Ensure we traverse into impl blocks
    fn visit_item_impl(&mut self, node: &'ast verus_syn::ItemImpl) {
        // Visit all items in the impl block
        verus_syn::visit::visit_item_impl(self, node);
    }

    // Ensure we traverse into trait definitions
    fn visit_item_trait(&mut self, node: &'ast verus_syn::ItemTrait) {
        // Visit all items in the trait
        verus_syn::visit::visit_item_trait(self, node);
    }

    // Ensure we traverse into modules
    fn visit_item_mod(&mut self, node: &'ast verus_syn::ItemMod) {
        // Visit all items in the module
        verus_syn::visit::visit_item_mod(self, node);
    }

    // Handle verus! macro blocks by parsing their contents
    fn visit_item_macro(&mut self, node: &'ast ItemMacro) {
        // Check if this is a verus! macro
        if let Some(ident) = &node.mac.path.get_ident() {
            if *ident == "verus" {
                // Try to parse the macro body as items
                if let Ok(items) = verus_syn::parse2::<VerusMacroBody>(node.mac.tokens.clone()) {
                    for item in items.items {
                        self.visit_item(&item);
                    }
                }
            }
        }
        // Continue with default traversal
        verus_syn::visit::visit_item_macro(self, node);
    }
}

/// Helper struct to parse verus! macro body as a list of items
struct VerusMacroBody {
    items: Vec<Item>,
}

impl verus_syn::parse::Parse for VerusMacroBody {
    fn parse(input: verus_syn::parse::ParseStream) -> verus_syn::Result<Self> {
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(input.parse()?);
        }
        Ok(VerusMacroBody { items })
    }
}

/// Parse a single source file and extract all function spans.
///
/// Returns a vector of (function_name, start_line, end_line) tuples.
pub fn parse_file_for_spans(file_path: &Path) -> Result<Vec<FunctionSpan>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read file {}: {}", file_path.display(), e))?;

    let syntax_tree = verus_syn::parse_file(&content)
        .map_err(|e| format!("Failed to parse file {}: {}", file_path.display(), e))?;

    let mut visitor = FunctionSpanVisitor::new();
    visitor.visit_file(&syntax_tree);

    Ok(visitor.functions)
}

/// Parse all source files in a project and build a lookup map.
///
/// Returns a map from (relative_path, function_name, definition_line) -> end_line.
/// We use definition_line (from SCIP) as part of the key to handle multiple
/// functions with the same name in the same file (e.g., different impl blocks).
pub fn build_function_span_map(
    project_root: &Path,
    relative_paths: &[String],
) -> HashMap<(String, String, usize), usize> {
    let mut span_map = HashMap::new();

    for rel_path in relative_paths {
        let full_path = project_root.join(rel_path);
        if !full_path.exists() {
            continue;
        }

        if let Ok(functions) = parse_file_for_spans(&full_path) {
            for func in functions {
                // Key: (relative_path, function_name, start_line)
                // Value: end_line
                let key = (rel_path.clone(), func.name.clone(), func.start_line);
                span_map.insert(key, func.end_line);
            }
        }
    }

    span_map
}

/// Get the end line for a function given its path, name, and start line.
///
/// If we can't find an exact match, we try to find a function with the same name
/// whose start line is close to the given start line (within a small tolerance).
pub fn get_function_end_line(
    span_map: &HashMap<(String, String, usize), usize>,
    relative_path: &str,
    function_name: &str,
    start_line: usize,
) -> Option<usize> {
    // Try exact match first
    let key = (
        relative_path.to_string(),
        function_name.to_string(),
        start_line,
    );
    if let Some(&end_line) = span_map.get(&key) {
        return Some(end_line);
    }

    // Try fuzzy match: find a function with the same name in the same file
    // whose start line is within tolerance of our start line.
    // We use 15 lines to account for doc comments which are included in the
    // function's span by the parser, but SCIP points to the signature line.
    const TOLERANCE: usize = 15;

    for ((path, name, parsed_start), &end_line) in span_map.iter() {
        if path == relative_path && name == function_name {
            let diff = if *parsed_start > start_line {
                parsed_start - start_line
            } else {
                start_line - parsed_start
            };

            if diff <= TOLERANCE {
                return Some(end_line);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_simple_function() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
fn hello_world() {{
    println!("Hello, world!");
}}

fn another_function(x: i32) -> i32 {{
    x + 1
}}
"#
        )
        .unwrap();

        let spans = parse_file_for_spans(file.path()).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].name, "hello_world");
        assert_eq!(spans[1].name, "another_function");

        // End lines should be after start lines
        assert!(spans[0].end_line >= spans[0].start_line);
        assert!(spans[1].end_line >= spans[1].start_line);
    }
}
