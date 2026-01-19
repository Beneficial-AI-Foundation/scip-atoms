//! Parser module using verus_syn to extract accurate function spans.
//!
//! SCIP only provides the location of function names, not their full body spans.
//! This module parses the actual source files to get accurate start/end line numbers.
//!
//! This module also provides functionality to find all functions in a project,
//! including support for Verus-specific constructs (spec, proof, exec functions).

use crate::FunctionMode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use verus_syn::spanned::Spanned;
use verus_syn::visit::Visit;
use verus_syn::{FnMode, ImplItemFn, Item, ItemFn, ItemMacro, TraitItemFn, Visibility};
use walkdir::WalkDir;

/// Type alias for spec clause line ranges: (requires_range, ensures_range)
/// Each range is Option<(start_line, end_line)> using 1-based line numbers.
pub type SpecRanges = (Option<(usize, usize)>, Option<(usize, usize)>);

/// Function span information
#[derive(Debug, Clone)]
pub struct FunctionSpan {
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Verus function mode
    pub mode: FunctionMode,
    /// Line range of requires clause (start, end), if present
    pub requires_range: Option<(usize, usize)>,
    /// Line range of ensures clause (start, end), if present
    pub ensures_range: Option<(usize, usize)>,
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

    /// Convert FnMode to FunctionMode
    fn convert_mode(mode: &FnMode) -> FunctionMode {
        match mode {
            FnMode::Spec(_) | FnMode::SpecChecked(_) => FunctionMode::Spec,
            FnMode::Proof(_) | FnMode::ProofAxiom(_) => FunctionMode::Proof,
            FnMode::Exec(_) | FnMode::Default => FunctionMode::Exec,
        }
    }

    /// Extract requires/ensures line ranges from a signature's spec
    fn extract_spec_ranges(sig: &verus_syn::Signature) -> SpecRanges {
        let requires_range = sig.spec.requires.as_ref().map(|req| {
            let span = req.span();
            (span.start().line, span.end().line)
        });

        let ensures_range = sig.spec.ensures.as_ref().map(|ens| {
            let span = ens.span();
            (span.start().line, span.end().line)
        });

        (requires_range, ensures_range)
    }
}

impl<'ast> Visit<'ast> for FunctionSpanVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        let start_line = span.start().line;
        let end_line = span.end().line;
        let mode = Self::convert_mode(&node.sig.mode);
        let (requires_range, ensures_range) = Self::extract_spec_ranges(&node.sig);

        self.functions.push(FunctionSpan {
            name,
            start_line,
            end_line,
            mode,
            requires_range,
            ensures_range,
        });

        // Continue visiting nested items
        verus_syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        let start_line = span.start().line;
        let end_line = span.end().line;
        let mode = Self::convert_mode(&node.sig.mode);
        let (requires_range, ensures_range) = Self::extract_spec_ranges(&node.sig);

        self.functions.push(FunctionSpan {
            name,
            start_line,
            end_line,
            mode,
            requires_range,
            ensures_range,
        });

        // Continue visiting nested items
        verus_syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        let start_line = span.start().line;
        let end_line = span.end().line;
        let mode = Self::convert_mode(&node.sig.mode);
        let (requires_range, ensures_range) = Self::extract_spec_ranges(&node.sig);

        self.functions.push(FunctionSpan {
            name,
            start_line,
            end_line,
            mode,
            requires_range,
            ensures_range,
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

    // Handle verus! and cfg_if! macro blocks by parsing their contents
    fn visit_item_macro(&mut self, node: &'ast ItemMacro) {
        if let Some(ident) = &node.mac.path.get_ident() {
            if *ident == "verus" {
                // Try to parse the macro body as items
                if let Ok(items) = verus_syn::parse2::<VerusMacroBody>(node.mac.tokens.clone()) {
                    for item in items.items {
                        self.visit_item(&item);
                    }
                }
            } else if *ident == "cfg_if" {
                // Try to parse the cfg_if! macro body
                // cfg_if! has syntax: if #[cfg(...)] { items } else if #[cfg(...)] { items } else { items }
                // We want to extract items from ALL branches since all may contain function definitions
                if let Ok(branches) = verus_syn::parse2::<CfgIfMacroBody>(node.mac.tokens.clone()) {
                    for items in branches.all_items {
                        for item in items {
                            self.visit_item(&item);
                        }
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

/// Helper struct to parse cfg_if! macro body
/// The syntax is: if #[cfg(...)] { items } else if #[cfg(...)] { items } else { items }
struct CfgIfMacroBody {
    all_items: Vec<Vec<Item>>,
}

impl verus_syn::parse::Parse for CfgIfMacroBody {
    fn parse(input: verus_syn::parse::ParseStream) -> verus_syn::Result<Self> {
        use verus_syn::Token;

        let mut all_items = Vec::new();

        // Parse: if #[cfg(...)] { items }
        if input.peek(Token![if]) {
            input.parse::<Token![if]>()?;

            // Skip the #[cfg(...)] attribute
            // In macro token streams, the tokens are:
            //   # followed by a Group{delimiter: Bracket} containing the attribute content
            // So we parse # and then a Group, not using bracketed! which expects [ ] tokens
            input.parse::<Token![#]>()?;
            let _attr_group: proc_macro2::Group = input.parse()?;

            // Parse the block { items }
            let content;
            verus_syn::braced!(content in input);
            let mut items = Vec::new();
            while !content.is_empty() {
                items.push(content.parse()?);
            }
            all_items.push(items);
        }

        // Parse any else if or else branches
        while input.peek(Token![else]) {
            input.parse::<Token![else]>()?;

            if input.peek(Token![if]) {
                // else if #[cfg(...)] { items }
                input.parse::<Token![if]>()?;
                input.parse::<Token![#]>()?;
                let _attr_group: proc_macro2::Group = input.parse()?;

                let content;
                verus_syn::braced!(content in input);
                let mut items = Vec::new();
                while !content.is_empty() {
                    items.push(content.parse()?);
                }
                all_items.push(items);
            } else {
                // else { items }
                let content;
                verus_syn::braced!(content in input);
                let mut items = Vec::new();
                while !content.is_empty() {
                    items.push(content.parse()?);
                }
                all_items.push(items);
                break; // else is always last
            }
        }

        Ok(CfgIfMacroBody { all_items })
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

/// Span and mode information for a function
#[derive(Debug, Clone)]
pub struct SpanAndMode {
    pub end_line: usize,
    pub mode: FunctionMode,
    /// Line range of requires clause (start, end), if present
    pub requires_range: Option<(usize, usize)>,
    /// Line range of ensures clause (start, end), if present
    pub ensures_range: Option<(usize, usize)>,
}

/// Parse all source files in a project and build a lookup map.
///
/// Returns a map from (relative_path, function_name, definition_line) -> SpanAndMode.
/// We use definition_line (from SCIP) as part of the key to handle multiple
/// functions with the same name in the same file (e.g., different impl blocks).
pub fn build_function_span_map(
    project_root: &Path,
    relative_paths: &[String],
) -> HashMap<(String, String, usize), SpanAndMode> {
    let mut span_map = HashMap::new();

    for rel_path in relative_paths {
        let full_path = project_root.join(rel_path);
        if !full_path.exists() {
            continue;
        }

        if let Ok(functions) = parse_file_for_spans(&full_path) {
            for func in functions {
                // Key: (relative_path, function_name, start_line)
                // Value: SpanAndMode (end_line + mode + spec ranges)
                let key = (rel_path.clone(), func.name.clone(), func.start_line);
                span_map.insert(
                    key,
                    SpanAndMode {
                        end_line: func.end_line,
                        mode: func.mode,
                        requires_range: func.requires_range,
                        ensures_range: func.ensures_range,
                    },
                );
            }
        }
    }

    span_map
}

/// Get the end line for a function given its path, name, and start line.
///
/// If we can't find an exact match, we try to find a function with the same name
/// where the SCIP-reported start line falls within the parsed span.
pub fn get_function_end_line(
    span_map: &HashMap<(String, String, usize), SpanAndMode>,
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
    if let Some(span_and_mode) = span_map.get(&key) {
        return Some(span_and_mode.end_line);
    }

    // Try containment match: find a function with the same name in the same file
    // where the SCIP start_line falls within the parsed span [parsed_start, end_line].
    // This works because verus_syn includes attributes/docs in the span, so the
    // actual signature line (what SCIP reports) should be within that span.
    for ((path, name, parsed_start), span_and_mode) in span_map.iter() {
        if path == relative_path && name == function_name {
            // SCIP's start_line should be within [parsed_start, end_line]
            if start_line >= *parsed_start && start_line <= span_and_mode.end_line {
                return Some(span_and_mode.end_line);
            }
        }
    }

    None
}

/// Get the function mode (exec, proof, spec) given its path, name, and start line.
///
/// Uses the same lookup strategy as get_function_end_line.
pub fn get_function_mode(
    span_map: &HashMap<(String, String, usize), SpanAndMode>,
    relative_path: &str,
    function_name: &str,
    start_line: usize,
) -> Option<FunctionMode> {
    // Try exact match first
    let key = (
        relative_path.to_string(),
        function_name.to_string(),
        start_line,
    );
    if let Some(span_and_mode) = span_map.get(&key) {
        return Some(span_and_mode.mode);
    }

    // Try containment match
    for ((path, name, parsed_start), span_and_mode) in span_map.iter() {
        if path == relative_path
            && name == function_name
            && start_line >= *parsed_start
            && start_line <= span_and_mode.end_line
        {
            return Some(span_and_mode.mode);
        }
    }

    None
}

/// Get the spec ranges (requires/ensures) for a function.
///
/// Returns (requires_range, ensures_range) where each is Option<(start_line, end_line)>.
pub fn get_function_spec_ranges(
    span_map: &HashMap<(String, String, usize), SpanAndMode>,
    relative_path: &str,
    function_name: &str,
    start_line: usize,
) -> SpecRanges {
    // Try exact match first
    let key = (
        relative_path.to_string(),
        function_name.to_string(),
        start_line,
    );
    if let Some(span_and_mode) = span_map.get(&key) {
        return (span_and_mode.requires_range, span_and_mode.ensures_range);
    }

    // Try containment match
    for ((path, name, parsed_start), span_and_mode) in span_map.iter() {
        if path == relative_path
            && name == function_name
            && start_line >= *parsed_start
            && start_line <= span_and_mode.end_line
        {
            return (span_and_mode.requires_range, span_and_mode.ensures_range);
        }
    }

    (None, None)
}

/// Line range for spec text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecText {
    #[serde(rename = "lines-start")]
    pub lines_start: usize,
    #[serde(rename = "lines-end")]
    pub lines_end: usize,
}

/// Detailed function information for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    #[serde(skip_serializing)]
    pub name: String,
    #[serde(rename = "code-path", skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(rename = "spec-text")]
    pub spec_text: SpecText,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>, // "impl", "trait", or "standalone"
    /// Whether the function has a specification (requires or ensures clause)
    #[serde(default)]
    pub specified: bool,
    /// Whether the function has requires clause (precondition)
    #[serde(default)]
    pub has_requires: bool,
    /// Whether the function has ensures clause (postcondition)
    #[serde(default)]
    pub has_ensures: bool,
    /// Whether the function body contains assume() or admit() (trusted assumptions)
    #[serde(default)]
    pub has_trusted_assumption: bool,
    /// Raw text of the requires clause (precondition), if present and requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_text: Option<String>,
    /// Raw text of the ensures clause (postcondition), if present and requested
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ensures_text: Option<String>,
}

/// Output format for function listing
#[derive(Debug, Serialize, Deserialize)]
pub struct ParsedOutput {
    pub functions: Vec<FunctionInfo>,
    pub functions_by_file: HashMap<String, Vec<FunctionInfo>>,
    pub summary: ParseSummary,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ParseSummary {
    pub total_functions: usize,
    pub total_files: usize,
}

/// Visitor that collects detailed function information
struct FunctionInfoVisitor {
    functions: Vec<FunctionInfo>,
    file_path: Option<String>,
    file_content: Option<String>,
    include_verus_constructs: bool,
    include_methods: bool,
    show_visibility: bool,
    show_kind: bool,
    include_spec_text: bool,
}

impl FunctionInfoVisitor {
    fn new(
        file_path: Option<String>,
        file_content: Option<String>,
        include_verus_constructs: bool,
        include_methods: bool,
        show_visibility: bool,
        show_kind: bool,
        include_spec_text: bool,
    ) -> Self {
        Self {
            functions: Vec::new(),
            file_path,
            file_content,
            include_verus_constructs,
            include_methods,
            show_visibility,
            show_kind,
            include_spec_text,
        }
    }

    /// Extract raw text from source content given a span (line range).
    /// Returns the text from start_line to end_line (inclusive, 1-indexed).
    fn extract_text_from_span(&self, start_line: usize, end_line: usize) -> Option<String> {
        let content = self.file_content.as_ref()?;
        let lines: Vec<&str> = content.lines().collect();

        // Convert to 0-indexed
        let start_idx = start_line.saturating_sub(1);
        let end_idx = end_line.min(lines.len());

        if start_idx >= lines.len() || start_idx >= end_idx {
            return None;
        }

        let text = lines[start_idx..end_idx].join("\n");
        Some(text.trim().to_string())
    }

    /// Extract spec text (requires or ensures) from a signature spec clause.
    fn extract_spec_text<T: Spanned>(&self, spec_clause: Option<&T>) -> Option<String> {
        if !self.include_spec_text {
            return None;
        }
        let clause = spec_clause?;
        let span = clause.span();
        self.extract_text_from_span(span.start().line, span.end().line)
    }

    /// Check if the function body (between start and end lines) contains assume() or admit()
    fn has_trusted_assumption(&self, start_line: usize, end_line: usize) -> bool {
        if let Some(content) = &self.file_content {
            let lines: Vec<&str> = content.lines().collect();
            // Lines are 1-indexed, convert to 0-indexed
            let start_idx = start_line.saturating_sub(1);
            let end_idx = end_line.min(lines.len());

            for line in &lines[start_idx..end_idx] {
                // Check for assume() or admit() calls
                // We look for the pattern with opening paren to avoid matching variable names
                if line.contains("assume(") || line.contains("admit(") {
                    return true;
                }
            }
        }
        false
    }

    fn extract_function_kind(&self, sig: &verus_syn::Signature) -> String {
        let mode_str = match sig.mode {
            FnMode::Spec(_) => "spec",
            FnMode::SpecChecked(_) => "spec(checked)",
            FnMode::Proof(_) => "proof",
            FnMode::ProofAxiom(_) => "proof(axiom)",
            FnMode::Exec(_) => "exec",
            FnMode::Default => "",
        };

        if sig.constness.is_some() {
            if mode_str.is_empty() {
                "const fn".to_string()
            } else {
                format!("{} const fn", mode_str)
            }
        } else if !mode_str.is_empty() {
            format!("{} fn", mode_str)
        } else {
            "fn".to_string()
        }
    }

    fn extract_visibility(&self, vis: &Visibility) -> String {
        match vis {
            Visibility::Public(_) => "pub".to_string(),
            Visibility::Restricted(r) => {
                if r.path.segments.len() == 1 {
                    let seg = &r.path.segments[0];
                    format!("pub({})", seg.ident)
                } else {
                    "pub(restricted)".to_string()
                }
            }
            Visibility::Inherited => "private".to_string(),
        }
    }

    fn should_include_function(&self, sig: &verus_syn::Signature) -> bool {
        if self.include_verus_constructs {
            // Include all functions including spec fn
            true
        } else {
            // Exclude only spec fn (no body to verify)
            // Include: fn, proof fn, exec fn (these have bodies that get verified)
            !matches!(sig.mode, FnMode::Spec(_) | FnMode::SpecChecked(_))
        }
    }

    fn add_function(
        &mut self,
        name: String,
        span: proc_macro2::Span,
        sig: &verus_syn::Signature,
        vis: &Visibility,
        context: Option<String>,
    ) {
        if !self.should_include_function(sig) {
            return;
        }

        let kind = if self.show_kind {
            Some(self.extract_function_kind(sig))
        } else {
            None
        };

        let visibility = if self.show_visibility {
            Some(self.extract_visibility(vis))
        } else {
            None
        };

        let start_line = span.start().line;
        let end_line = span.end().line;

        // Extract spec information
        let has_requires = sig.spec.requires.is_some();
        let has_ensures = sig.spec.ensures.is_some();
        let has_trusted_assumption = self.has_trusted_assumption(start_line, end_line);

        // Extract spec text if requested
        let requires_text = self.extract_spec_text(sig.spec.requires.as_ref());
        let ensures_text = self.extract_spec_text(sig.spec.ensures.as_ref());

        self.functions.push(FunctionInfo {
            name,
            file: self.file_path.clone(),
            spec_text: SpecText {
                lines_start: start_line,
                lines_end: end_line,
            },
            kind,
            visibility,
            context,
            specified: has_requires || has_ensures,
            has_requires,
            has_ensures,
            has_trusted_assumption,
            requires_text,
            ensures_text,
        });
    }
}

impl<'ast> Visit<'ast> for FunctionInfoVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        let name = node.sig.ident.to_string();
        let span = node.span();
        self.add_function(
            name,
            span,
            &node.sig,
            &node.vis,
            Some("standalone".to_string()),
        );
        verus_syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        if !self.include_methods {
            return;
        }

        let name = node.sig.ident.to_string();
        let span = node.span();
        self.add_function(name, span, &node.sig, &node.vis, Some("impl".to_string()));
        verus_syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        if !self.include_methods {
            return;
        }

        let name = node.sig.ident.to_string();
        let span = node.span();
        let vis = Visibility::Inherited;
        self.add_function(name, span, &node.sig, &vis, Some("trait".to_string()));
        verus_syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast verus_syn::ItemImpl) {
        verus_syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast verus_syn::ItemTrait) {
        verus_syn::visit::visit_item_trait(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast verus_syn::ItemMod) {
        verus_syn::visit::visit_item_mod(self, node);
    }

    fn visit_item_macro(&mut self, node: &'ast ItemMacro) {
        if let Some(ident) = &node.mac.path.get_ident() {
            if *ident == "verus" {
                if let Ok(items) = verus_syn::parse2::<VerusMacroBody>(node.mac.tokens.clone()) {
                    for item in items.items {
                        self.visit_item(&item);
                    }
                }
            } else if *ident == "cfg_if" {
                if let Ok(branches) = verus_syn::parse2::<CfgIfMacroBody>(node.mac.tokens.clone()) {
                    for items in branches.all_items {
                        for item in items {
                            self.visit_item(&item);
                        }
                    }
                }
            }
        }
        verus_syn::visit::visit_item_macro(self, node);
    }
}

/// Parse a file and extract detailed function information
pub fn parse_file_for_functions(
    file_path: &Path,
    include_verus_constructs: bool,
    include_methods: bool,
    show_visibility: bool,
    show_kind: bool,
    include_spec_text: bool,
) -> Result<Vec<FunctionInfo>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read file {}: {}", file_path.display(), e))?;

    let syntax_tree = verus_syn::parse_file(&content)
        .map_err(|e| format!("Failed to parse file {}: {}", file_path.display(), e))?;

    let mut visitor = FunctionInfoVisitor::new(
        Some(file_path.to_string_lossy().to_string()),
        Some(content),
        include_verus_constructs,
        include_methods,
        show_visibility,
        show_kind,
        include_spec_text,
    );
    visitor.visit_file(&syntax_tree);

    Ok(visitor.functions)
}

/// Find all Rust files in a directory
fn find_rust_files(path: &Path) -> Vec<std::path::PathBuf> {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "rs"))
        .map(|e| e.path().to_path_buf())
        .collect()
}

/// Parse all functions from a path (file or directory)
pub fn parse_all_functions(
    path: &Path,
    include_verus_constructs: bool,
    include_methods: bool,
    show_visibility: bool,
    show_kind: bool,
    include_spec_text: bool,
) -> ParsedOutput {
    let mut all_functions = Vec::new();
    let mut functions_by_file: HashMap<String, Vec<FunctionInfo>> = HashMap::new();
    let mut total_files = 0;

    // Get the base directory to strip from paths (to make them project-relative)
    // This matches how verus-analyzer generates relative_path in SCIP:
    // - For a directory: use the directory itself as base, so paths are relative to it
    // - For a file: use grandparent to include the parent directory name
    let base_dir: Option<&Path> = if path.is_file() {
        path.parent().and_then(|p| p.parent())
    } else {
        Some(path)
    };

    // Helper to make path relative to project root (like atoms.json format)
    let make_relative = |full_path: &Path| -> String {
        if let Some(base) = base_dir {
            if let Ok(rel) = full_path.strip_prefix(base) {
                return rel.to_string_lossy().to_string();
            }
        }
        full_path.to_string_lossy().to_string()
    };

    if path.is_file() {
        match parse_file_for_functions(
            path,
            include_verus_constructs,
            include_methods,
            show_visibility,
            show_kind,
            include_spec_text,
        ) {
            Ok(mut functions) => {
                let relative_path = make_relative(path);
                // Update file paths in functions to use relative path
                for func in &mut functions {
                    func.file = Some(relative_path.clone());
                }
                if !functions.is_empty() {
                    functions_by_file.insert(relative_path, functions.clone());
                    all_functions.extend(functions);
                    total_files = 1;
                }
            }
            Err(e) => {
                eprintln!("Error parsing file: {}", e);
            }
        }
    } else {
        let rust_files = find_rust_files(path);
        total_files = rust_files.len();

        for file_path in rust_files {
            match parse_file_for_functions(
                &file_path,
                include_verus_constructs,
                include_methods,
                show_visibility,
                show_kind,
                include_spec_text,
            ) {
                Ok(mut functions) => {
                    if !functions.is_empty() {
                        let relative_path = make_relative(&file_path);
                        // Update file paths in functions to use relative path
                        for func in &mut functions {
                            func.file = Some(relative_path.clone());
                        }
                        functions_by_file.insert(relative_path, functions.clone());
                        all_functions.extend(functions);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: {}", e);
                }
            }
        }
    }

    ParsedOutput {
        functions: all_functions.clone(),
        functions_by_file,
        summary: ParseSummary {
            total_functions: all_functions.len(),
            total_files,
        },
    }
}

/// Find all functions with their line numbers (simplified output format)
/// Returns a map from file path to list of (function_name, line_number)
pub fn find_all_functions(
    path: &Path,
    include_verus_constructs: bool,
) -> HashMap<String, Vec<(String, usize)>> {
    let output = parse_all_functions(path, include_verus_constructs, true, false, false, false);

    output
        .functions_by_file
        .into_iter()
        .map(|(file_path, functions)| {
            let simplified: Vec<(String, usize)> = functions
                .into_iter()
                .map(|f| (f.name, f.spec_text.lines_start))
                .collect();
            (file_path, simplified)
        })
        .collect()
}

/// Get a simple list of unique function names
pub fn get_function_names(path: &Path, include_verus_constructs: bool) -> Vec<String> {
    let output = parse_all_functions(path, include_verus_constructs, true, false, false, false);
    let mut names: std::collections::HashSet<String> =
        output.functions.into_iter().map(|f| f.name).collect();
    let mut sorted: Vec<String> = names.drain().collect();
    sorted.sort();
    sorted
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

    #[test]
    fn test_parse_file_for_functions() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
pub fn public_func() {{}}

fn private_func() {{}}

impl Foo {{
    pub fn method(&self) {{}}
}}
"#
        )
        .unwrap();

        let functions =
            parse_file_for_functions(file.path(), true, true, true, true, false).unwrap();
        assert_eq!(functions.len(), 3);

        // Check visibility is captured
        let public_func = functions.iter().find(|f| f.name == "public_func").unwrap();
        assert_eq!(public_func.visibility, Some("pub".to_string()));

        let private_func = functions.iter().find(|f| f.name == "private_func").unwrap();
        assert_eq!(private_func.visibility, Some("private".to_string()));
    }
}
