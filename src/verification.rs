//! Verification output parsing module.
//!
//! This module provides functionality to parse Verus/Cargo verification output,
//! including compilation errors, verification failures, and verification results.
//! Ported from the Python find_verus_functions_syn.py script.

use regex::Regex;
use rust_lapper::{Interval, Lapper};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

/// Function metadata stored in the interval tree
#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionInterval {
    pub name: String,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub has_trusted_assumption: bool,
}

/// Interval type for rust-lapper (uses usize for start/stop)
type FuncInterval = Interval<usize, FunctionInterval>;

/// Efficient function index using interval trees for O(log n) lookups
///
/// Instead of linear scans, this uses rust-lapper to efficiently query
/// which function contains a given line number.
struct FunctionIndex {
    /// Map from normalized file path to interval tree of functions
    trees: HashMap<String, Lapper<usize, FunctionInterval>>,
    /// All files we know about (for fuzzy path matching)
    known_files: Vec<String>,
}

impl FunctionIndex {
    /// Build a function index from parsed function info
    pub fn from_functions(functions: &[crate::verus_parser::FunctionInfo]) -> Self {
        let mut intervals_by_file: HashMap<String, Vec<FuncInterval>> = HashMap::new();

        for func in functions {
            let file_path = func.file.clone().unwrap_or_default();
            if file_path.is_empty() {
                continue;
            }

            let interval = Interval {
                start: func.start_line,
                stop: func.end_line + 1, // rust-lapper uses half-open intervals [start, stop)
                val: FunctionInterval {
                    name: func.name.clone(),
                    file: file_path.clone(),
                    start_line: func.start_line,
                    end_line: func.end_line,
                    has_trusted_assumption: func.has_trusted_assumption,
                },
            };

            intervals_by_file
                .entry(file_path)
                .or_default()
                .push(interval);
        }

        let mut trees = HashMap::new();
        let mut known_files = Vec::new();

        for (file, intervals) in intervals_by_file {
            known_files.push(file.clone());
            trees.insert(file, Lapper::new(intervals));
        }

        Self { trees, known_files }
    }

    /// Find the function containing the given line in the given file
    ///
    /// Returns the function info if found, handling fuzzy path matching
    /// (exact > suffix > filename-only).
    pub fn find_at_line(&self, file_path: &str, line: usize) -> Option<&FunctionInterval> {
        // Find the best matching file
        let matching_file = self.find_matching_file(file_path)?;

        // Query the interval tree - O(log n)
        let tree = self.trees.get(matching_file)?;
        let mut results: Vec<_> = tree.find(line, line + 1).collect();

        // If multiple functions contain this line (nested), return the innermost
        // (smallest span)
        results.sort_by_key(|iv| iv.stop - iv.start);
        results.first().map(|iv| &iv.val)
    }

    /// Find the best matching file path with priority: exact > suffix > filename-only
    fn find_matching_file(&self, query_path: &str) -> Option<&String> {
        let query = std::path::Path::new(query_path);
        let query_str = query.to_string_lossy();

        let mut best_match: Option<&String> = None;
        let mut best_score = 0; // 0=none, 1=filename, 2=suffix, 3=exact

        for file_key in &self.known_files {
            let key_path = std::path::Path::new(file_key);
            let key_str = key_path.to_string_lossy();

            // Exact match (highest priority)
            if query == key_path {
                return Some(file_key); // Can't do better
            }

            // Suffix match (high priority)
            if query_str.ends_with(&*key_str) || key_str.ends_with(&*query_str) {
                if best_score < 2 {
                    best_match = Some(file_key);
                    best_score = 2;
                }
                continue;
            }

            // Filename-only match (lowest priority)
            if best_score < 1 && query.file_name() == key_path.file_name() {
                best_match = Some(file_key);
                best_score = 1;
            }
        }

        best_match
    }
}

/// A compilation or verification error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationError {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<i32>,
    pub column: Option<i32>,
    pub full_message: Vec<String>,
}

/// A verification failure with detailed information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationFailure {
    pub error_type: String,
    pub file: Option<String>,
    pub line: Option<i32>,
    pub column: Option<i32>,
    pub message: String,
    pub assertion_details: Vec<String>,
    pub full_error_text: String,
}

/// Parser for compilation errors from cargo/verus output
pub struct CompilationErrorParser {
    error_pattern: Regex,
    cargo_error_pattern: Regex,
    warning_pattern: Regex,
    file_location_pattern: Regex,
    process_error_pattern: Regex,
    memory_error_pattern: Regex,
    exit_status_pattern: Regex,
    verus_command_exit_pattern: Regex,
    verification_results_pattern: Regex,
    verification_error_patterns: Vec<Regex>,
}

impl Default for CompilationErrorParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CompilationErrorParser {
    pub fn new() -> Self {
        Self {
            error_pattern: Regex::new(r"error(?:\[E\d+\])?: (.+)").unwrap(),
            cargo_error_pattern: Regex::new(r"error: could not compile `([^`]+)`").unwrap(),
            warning_pattern: Regex::new(r"warning: (.+)").unwrap(),
            file_location_pattern: Regex::new(r"-->\s+([^:]+):(\d+):(\d+)").unwrap(),
            process_error_pattern: Regex::new(r"process didn't exit successfully: (.+)").unwrap(),
            memory_error_pattern: Regex::new(r"memory allocation of \d+ bytes failed").unwrap(),
            exit_status_pattern: Regex::new(r"\(exit status: (\d+)\)").unwrap(),
            verus_command_exit_pattern: Regex::new(
                r"Verus command completed with exit code: (\d+)",
            )
            .unwrap(),
            verification_results_pattern: Regex::new(
                r"verification results::\s*(\d+)\s+verified,\s*(\d+)\s+errors?",
            )
            .unwrap(),
            verification_error_patterns: vec![
                Regex::new(r"error: assertion failed").unwrap(),
                Regex::new(r"error: postcondition not satisfied").unwrap(),
                Regex::new(r"error: precondition not satisfied").unwrap(),
                Regex::new(r"error: loop invariant not preserved").unwrap(),
                Regex::new(r"error: loop invariant not satisfied on entry").unwrap(),
                Regex::new(r"error: assertion not satisfied").unwrap(),
            ],
        }
    }

    /// Check if the output contains verification results summary
    pub fn has_verification_results(&self, output_content: &str) -> bool {
        self.verification_results_pattern.is_match(output_content)
    }

    /// Parse compilation output and extract errors and warnings
    pub fn parse_compilation_output(
        &self,
        output_content: &str,
    ) -> (Vec<CompilationError>, Vec<CompilationError>) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut current_error: Option<CompilationError> = None;
        let mut current_warning: Option<CompilationError> = None;

        let has_verification_results = self.has_verification_results(output_content);
        let lines: Vec<&str> = output_content.lines().collect();

        for line in &lines {
            let line = line.trim();

            // Skip verification results summary lines
            if self.verification_results_pattern.is_match(line) {
                continue;
            }

            // Check for cargo compilation errors
            if let Some(caps) = self.cargo_error_pattern.captures(line) {
                if has_verification_results {
                    continue;
                }

                if let Some(err) = current_error.take() {
                    errors.push(err);
                }
                current_error = Some(CompilationError {
                    message: format!("Compilation failed for crate: {}", &caps[1]),
                    file: None,
                    line: None,
                    column: None,
                    full_message: vec![line.to_string()],
                });
                continue;
            }

            // Check for memory allocation errors
            if self.memory_error_pattern.is_match(line) {
                if let Some(ref mut err) = current_error {
                    err.full_message.push(line.to_string());
                    err.message = format!("{} - {}", err.message, line);
                } else {
                    errors.push(CompilationError {
                        message: line.to_string(),
                        file: None,
                        line: None,
                        column: None,
                        full_message: vec![line.to_string()],
                    });
                }
                continue;
            }

            // Check for Verus command exit code messages
            if let Some(caps) = self.verus_command_exit_pattern.captures(line) {
                let exit_code: i32 = caps[1].parse().unwrap_or(1);
                if let Some(ref mut err) = current_error {
                    err.full_message.push(line.to_string());
                    err.message = format!("{} (exit code: {})", err.message, exit_code);
                } else {
                    current_error = Some(CompilationError {
                        message: format!("Verus command failed with exit code {}", exit_code),
                        file: None,
                        line: None,
                        column: None,
                        full_message: vec![line.to_string()],
                    });
                }
                continue;
            }

            // Check for process failure errors
            if let Some(caps) = self.process_error_pattern.captures(line) {
                if let Some(ref mut err) = current_error {
                    err.full_message.push(line.to_string());
                    err.message = format!("{} - {}", err.message, &caps[1]);
                } else {
                    current_error = Some(CompilationError {
                        message: format!("Process execution failed: {}", &caps[1]),
                        file: None,
                        line: None,
                        column: None,
                        full_message: vec![line.to_string()],
                    });
                }
                continue;
            }

            // Check for standard error format
            if let Some(caps) = self.error_pattern.captures(line) {
                // Skip verification errors (we handle those separately)
                let is_verification_error = self
                    .verification_error_patterns
                    .iter()
                    .any(|p| p.is_match(line));

                if is_verification_error {
                    continue;
                }

                if let Some(err) = current_error.take() {
                    errors.push(err);
                }
                current_error = Some(CompilationError {
                    message: caps[1].trim().to_string(),
                    file: None,
                    line: None,
                    column: None,
                    full_message: vec![line.to_string()],
                });
                continue;
            }

            // Check for warning
            if let Some(caps) = self.warning_pattern.captures(line) {
                if let Some(warn) = current_warning.take() {
                    warnings.push(warn);
                }
                current_warning = Some(CompilationError {
                    message: caps[1].trim().to_string(),
                    file: None,
                    line: None,
                    column: None,
                    full_message: vec![line.to_string()],
                });
                continue;
            }

            // Check for file location
            if let Some(caps) = self.file_location_pattern.captures(line) {
                let file_path = caps[1].to_string();
                let line_num: i32 = caps[2].parse().unwrap_or(0);
                let column: i32 = caps[3].parse().unwrap_or(0);

                if let Some(ref mut err) = current_error {
                    err.file = Some(file_path);
                    err.line = Some(line_num);
                    err.column = Some(column);
                    err.full_message.push(line.to_string());
                } else if let Some(ref mut warn) = current_warning {
                    warn.file = Some(file_path);
                    warn.line = Some(line_num);
                    warn.column = Some(column);
                    warn.full_message.push(line.to_string());
                }
                continue;
            }

            // Add continuation lines
            if line.starts_with('|')
                || line.starts_with('^')
                || line.starts_with('=')
                || line.starts_with("Caused by:")
                || line.starts_with("(signal:")
                || line.contains("process didn't exit successfully:")
                || self.exit_status_pattern.is_match(line)
            {
                if let Some(ref mut err) = current_error {
                    err.full_message.push(line.to_string());
                    if line.starts_with("Caused by:") || line.contains("(signal:") {
                        err.message = format!("{} - {}", err.message, line.trim());
                    }
                    if let Some(caps) = self.exit_status_pattern.captures(line) {
                        err.message = format!("{} (exit status: {})", err.message, &caps[1]);
                    }
                } else if let Some(ref mut warn) = current_warning {
                    warn.full_message.push(line.to_string());
                }
            } else if line.is_empty() {
                if let Some(err) = current_error.take() {
                    if !err.full_message.is_empty() {
                        errors.push(err);
                    }
                }
                if let Some(warn) = current_warning.take() {
                    if !warn.full_message.is_empty() {
                        warnings.push(warn);
                    }
                }
            }
        }

        // Don't forget any remaining errors/warnings
        if let Some(err) = current_error {
            errors.push(err);
        }
        if let Some(warn) = current_warning {
            warnings.push(warn);
        }

        (errors, warnings)
    }
}

/// Parser for verification results
pub struct VerificationParser {
    error_pattern: Regex,
    verification_error_types: Vec<&'static str>,
    ansi_escape_pattern: Regex,
}

impl Default for VerificationParser {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationParser {
    pub fn new() -> Self {
        Self {
            error_pattern: Regex::new(r"-->\s+([^:]+):(\d+):\d+").unwrap(),
            verification_error_types: vec![
                "assertion failed",
                "postcondition not satisfied",
                "precondition not satisfied",
                "loop invariant not preserved",
                "loop invariant not satisfied on entry",
                "assertion not satisfied",
            ],
            ansi_escape_pattern: Regex::new(r"\x1b\[[0-9;]*m").unwrap(),
        }
    }

    /// Parse verification output file and extract files with errors and their line numbers
    pub fn parse_verification_output(
        &self,
        output_file_path: &Path,
    ) -> Result<HashMap<String, Vec<i32>>, std::io::Error> {
        let content = fs::read_to_string(output_file_path)?;
        Ok(self.parse_verification_output_from_content(&content))
    }

    /// Parse verification output content and extract files with errors and their line numbers
    pub fn parse_verification_output_from_content(
        &self,
        output_content: &str,
    ) -> HashMap<String, Vec<i32>> {
        let mut errors_by_file: HashMap<String, Vec<i32>> = HashMap::new();
        let lines: Vec<&str> = output_content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            if let Some(caps) = self.error_pattern.captures(line) {
                let file_path = caps[1].to_string();
                let line_number: i32 = caps[2].parse().unwrap_or(0);

                // Look back to see if this is an actual error
                let mut is_actual_error = false;
                for j in (i.saturating_sub(10)..i).rev() {
                    let prev_line = lines[j].trim();

                    if (prev_line.starts_with("error:") || prev_line.starts_with("error["))
                        && !prev_line.starts_with("note:")
                        && !prev_line.contains("has been running for")
                        && !prev_line.contains("finished in")
                        && !prev_line.contains("check has been running")
                        && !prev_line.contains("check finished in")
                    {
                        is_actual_error = true;
                        break;
                    }

                    if prev_line.starts_with("note:")
                        && (prev_line.contains("has been running for")
                            || prev_line.contains("finished in")
                            || prev_line.contains("check has been running")
                            || prev_line.contains("check finished in"))
                    {
                        is_actual_error = false;
                        break;
                    }
                }

                if is_actual_error {
                    errors_by_file
                        .entry(file_path)
                        .or_default()
                        .push(line_number);
                }
            }
        }

        errors_by_file
    }

    /// Parse verification failures and return detailed information
    pub fn parse_verification_failures(&self, output_content: &str) -> Vec<VerificationFailure> {
        let mut failures = Vec::new();
        let lines: Vec<&str> = output_content.lines().collect();

        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim();

            // Check for verification error types
            let mut error_type: Option<&str> = None;
            for &err_type in &self.verification_error_types {
                if line.contains(err_type) {
                    error_type = Some(err_type);
                    break;
                }
            }

            if let Some(err_type) = error_type {
                if line.to_lowercase().contains("error") {
                    let mut file_path: Option<String> = None;
                    let mut line_number: Option<i32> = None;
                    let mut column: Option<i32> = None;

                    let mut full_error_lines = Vec::new();
                    let mut location_found_at: Option<usize> = None;

                    // Collect error context (up to 15 lines)
                    for j in i..std::cmp::min(i + 15, lines.len()) {
                        let current_line = lines[j];
                        full_error_lines.push(current_line);

                        if let Some(caps) = self.error_pattern.captures(current_line) {
                            if location_found_at.is_none() {
                                file_path = Some(caps[1].to_string());
                                line_number = Some(caps[2].parse().unwrap_or(0));
                                location_found_at = Some(j);

                                // Try to extract column
                                let parts: Vec<&str> = current_line.split(':').collect();
                                if parts.len() >= 3 {
                                    if let Ok(col) = parts.last().unwrap_or(&"").parse::<i32>() {
                                        column = Some(col);
                                    }
                                }
                            }
                        }

                        // Check if we've reached the end of this error
                        if let Some(loc_at) = location_found_at {
                            if j > loc_at + 1 {
                                let next_line = current_line.trim();
                                if next_line.is_empty() && j + 1 < lines.len() {
                                    let next_next_line = lines[j + 1].trim();
                                    if next_next_line.starts_with("error:")
                                        || next_next_line.starts_with("verification results")
                                        || next_next_line.starts_with("note:")
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Clean ANSI escape codes
                    let clean_full_text: Vec<String> = full_error_lines
                        .iter()
                        .map(|l| {
                            self.ansi_escape_pattern
                                .replace_all(l.trim_end(), "")
                                .to_string()
                        })
                        .collect();

                    let complete_error_text = clean_full_text.join("\n").trim().to_string();

                    // Extract assertion details
                    let assertion_details: Vec<String> = clean_full_text
                        .iter()
                        .filter(|l| {
                            let clean_line = l.trim();
                            !clean_line.is_empty()
                                && (clean_line.contains("assert")
                                    || clean_line.contains('|')
                                    || clean_line.starts_with("-->"))
                        })
                        .take(10)
                        .cloned()
                        .collect();

                    let clean_file_path =
                        file_path.map(|f| self.ansi_escape_pattern.replace_all(&f, "").to_string());
                    let clean_message = self
                        .ansi_escape_pattern
                        .replace_all(line.trim(), "")
                        .to_string();

                    failures.push(VerificationFailure {
                        error_type: err_type.to_string(),
                        file: clean_file_path,
                        line: line_number,
                        column,
                        message: clean_message,
                        assertion_details,
                        full_error_text: complete_error_text,
                    });
                }
            }

            i += 1;
        }

        failures
    }

    /// Find the function that contains or is closest above the given line number
    pub fn find_function_at_line(
        &self,
        file_path: &str,
        line_number: i32,
        all_functions_with_lines: &HashMap<String, Vec<(String, usize)>>,
    ) -> Option<String> {
        let file_path_normalized = std::path::Path::new(file_path);
        let file_path_str = file_path_normalized.to_string_lossy();

        // Find matching file with priority: exact > suffix > filename-only
        // We need to find the BEST match, not just the first match
        let mut best_match: Option<&String> = None;
        let mut best_match_score = 0; // 0=none, 1=filename, 2=suffix, 3=exact

        for file_key in all_functions_with_lines.keys() {
            let file_key_path = std::path::Path::new(file_key);
            let file_key_str = file_key_path.to_string_lossy();

            // Exact match (highest priority)
            if file_path_normalized == file_key_path {
                best_match = Some(file_key);
                break; // Can't do better than exact
            }

            // Suffix match (high priority)
            // Check if error path ends with known file path or vice versa
            if file_path_str.ends_with(&*file_key_str) || file_key_str.ends_with(&*file_path_str) {
                if best_match_score < 2 {
                    best_match = Some(file_key);
                    best_match_score = 2;
                }
                continue;
            }

            // Filename-only match (lowest priority, only if no better match)
            if best_match_score < 1 && file_path_normalized.file_name() == file_key_path.file_name()
            {
                best_match = Some(file_key);
                best_match_score = 1;
            }
        }

        let matching_file = best_match?;
        let functions_in_file = all_functions_with_lines.get(matching_file)?;

        // Find closest function above the line
        let mut closest_function: Option<&str> = None;
        let mut closest_line: usize = 0;

        for (func_name, func_line) in functions_in_file {
            if *func_line <= line_number as usize && *func_line > closest_line {
                closest_function = Some(func_name);
                closest_line = *func_line;
            }
        }

        closest_function.map(|s| s.to_string())
    }
}

/// Runner for Verus verification
pub struct VerusRunner;

impl Default for VerusRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl VerusRunner {
    pub fn new() -> Self {
        Self
    }

    /// Set up environment variables for Verus verification
    fn setup_environment(&self) {
        let boring_stub = "/tmp/boring-stub";
        let _ = fs::create_dir_all(boring_stub);
        let _ = fs::create_dir_all(format!("{}/lib", boring_stub));
        let _ = fs::create_dir_all(format!("{}/include", boring_stub));

        std::env::set_var("BORING_BSSL_PATH", boring_stub);
        std::env::set_var("BORING_BSSL_ASSUME_PATCHED", "1");
        std::env::set_var("DOCS_RS", "1");
    }

    /// Run cargo verus verification and return output and exit code
    pub fn run_verification(
        &self,
        work_dir: &Path,
        package: Option<&str>,
        module: Option<&str>,
        function: Option<&str>,
        extra_args: Option<&[String]>,
    ) -> Result<(String, i32), std::io::Error> {
        self.setup_environment();

        let mut cmd = Command::new("cargo");
        cmd.arg("verus").arg("verify");

        if let Some(pkg) = package {
            cmd.arg("-p").arg(pkg);
        }

        // Verus-specific args go after --
        let mut has_verus_args = false;
        if module.is_some() || function.is_some() {
            cmd.arg("--");
            has_verus_args = true;

            if let Some(mod_name) = module {
                cmd.arg("--verify-only-module").arg(mod_name);
            }
            if let Some(func_name) = function {
                cmd.arg("--verify-function").arg(func_name);
            }
        }

        if let Some(args) = extra_args {
            if !has_verus_args && !args.is_empty() {
                cmd.arg("--");
            }
            for arg in args {
                cmd.arg(arg);
            }
        }

        cmd.current_dir(work_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let output = cmd.output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}\n{}", stdout, stderr);

        let exit_code = output.status.code().unwrap_or(1);

        Ok((combined, exit_code))
    }
}

/// Comprehensive analysis result
#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub status: AnalysisStatus,
    pub summary: AnalysisSummary,
    pub verification: VerificationResult,
    pub compilation: CompilationResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    Success,
    VerificationFailed,
    CompilationFailed,
    FunctionsOnly,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisSummary {
    /// Total verifiable functions (those with requires/ensures)
    pub total_functions: usize,
    pub failed_functions: usize,
    pub verified_functions: usize,
    /// Functions with assume() or admit() - not fully verified
    pub unverified_functions: usize,
    pub verification_errors: usize,
    pub compilation_errors: usize,
    pub compilation_warnings: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompilationResult {
    pub errors: Vec<CompilationError>,
    pub warnings: Vec<CompilationError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerificationResult {
    pub failed_functions: Vec<FunctionLocation>,
    pub verified_functions: Vec<FunctionLocation>,
    /// Functions with assume() or admit() - not fully verified
    pub unverified_functions: Vec<FunctionLocation>,
    pub errors: Vec<VerificationFailure>,
}

/// Function location info - aligned with atoms.json format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionLocation {
    #[serde(rename = "display-name")]
    pub display_name: String,
    #[serde(rename = "code-path")]
    pub code_path: String,
    #[serde(rename = "code-text")]
    pub code_text: CodeTextInfo,
}

/// Code text info with line ranges - aligned with atoms.json format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeTextInfo {
    #[serde(rename = "lines-start")]
    pub lines_start: usize,
    #[serde(rename = "lines-end")]
    pub lines_end: usize,
}

/// Verification output analyzer
///
/// Analyzes Verus verification output to categorize functions as:
/// - verified: Has specs (requires/ensures), passed verification, no assume/admit
/// - failed: Has specs, had verification errors
/// - unverified: Has specs, contains assume() or admit()
///
/// Includes: fn, proof fn, exec fn with requires/ensures
/// Excludes: spec fn (no body to verify), functions without specs
pub struct VerificationAnalyzer {
    compilation_parser: CompilationErrorParser,
    verification_parser: VerificationParser,
}

impl Default for VerificationAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl VerificationAnalyzer {
    pub fn new() -> Self {
        Self {
            compilation_parser: CompilationErrorParser::new(),
            verification_parser: VerificationParser::new(),
        }
    }

    /// Analyze verification output content
    pub fn analyze_output(
        &self,
        path: &Path,
        output_content: &str,
        exit_code: Option<i32>,
        module_filter: Option<&str>,
        function_filter: Option<&str>,
    ) -> AnalysisResult {
        // Parse compilation errors and warnings
        let (compilation_errors, compilation_warnings) = self
            .compilation_parser
            .parse_compilation_output(output_content);

        // Get all functions with full info (including end lines and spec info)
        // Note: We set include_verus_constructs to false to exclude spec fn (no body to verify)
        // but still include proof fn and exec fn (they have bodies that get verified)
        let parsed_output = crate::verus_parser::parse_all_functions(
            path, false, // exclude only spec fn (no body to verify)
            true,  // include_methods
            false, // show_visibility
            false, // show_kind
        );

        // Filter to only verifiable functions (those with requires or ensures)
        let verifiable_functions: Vec<_> = parsed_output
            .functions
            .iter()
            .filter(|f| f.has_requires || f.has_ensures)
            .cloned()
            .collect();

        // Build interval tree index for O(log n) lookups
        let function_index = FunctionIndex::from_functions(&verifiable_functions);

        // Parse verification errors from content
        let errors_by_file = self
            .verification_parser
            .parse_verification_output_from_content(output_content);

        // Parse detailed verification failures
        let verification_failures = self
            .verification_parser
            .parse_verification_failures(output_content);

        // Track which specific function locations failed (by key: name, file, start_line)
        let mut failed_function_keys: std::collections::HashSet<(String, String, usize)> =
            std::collections::HashSet::new();

        // Helper closure to mark a function as failed - now uses O(log n) interval tree lookup
        let mut mark_failed = |error_file: &str, error_line: i32| {
            if let Some(func_info) = function_index.find_at_line(error_file, error_line as usize) {
                failed_function_keys.insert((
                    func_info.name.clone(),
                    func_info.file.clone(),
                    func_info.start_line,
                ));
            }
        };

        // Mark failed functions from error locations
        for (file_path, error_lines) in &errors_by_file {
            for error_line in error_lines {
                mark_failed(file_path, *error_line);
            }
        }

        // Mark failed functions from detailed failures
        for failure in &verification_failures {
            if let (Some(file), Some(line)) = (&failure.file, failure.line) {
                mark_failed(file, line);
            }
        }

        // Determine status
        let has_compilation_errors = !compilation_errors.is_empty();
        let has_verification_failures = !verification_failures.is_empty();
        let has_verification_results = self
            .compilation_parser
            .has_verification_results(output_content);

        let mut status = if has_verification_results {
            if has_verification_failures {
                AnalysisStatus::VerificationFailed
            } else {
                AnalysisStatus::Success
            }
        } else if has_compilation_errors {
            AnalysisStatus::CompilationFailed
        } else {
            AnalysisStatus::Success
        };

        // Handle non-zero exit code without other indicators
        if let Some(code) = exit_code {
            if code != 0
                && !has_compilation_errors
                && !has_verification_failures
                && !has_verification_results
            {
                status = AnalysisStatus::CompilationFailed;
            }
        }

        // Categorize functions into: failed, verified, unverified
        let (failed_locations, verified_locations, unverified_locations) =
            if status == AnalysisStatus::CompilationFailed {
                (Vec::new(), Vec::new(), Vec::new())
            } else {
                let mut failed = Vec::new();
                let mut verified = Vec::new();
                let mut unverified = Vec::new();

                for func in &verifiable_functions {
                    let file_path = func.file.clone().unwrap_or_default();
                    let key = (func.name.clone(), file_path.clone(), func.start_line);

                    let location = FunctionLocation {
                        display_name: func.name.clone(),
                        code_path: file_path,
                        code_text: CodeTextInfo {
                            lines_start: func.start_line,
                            lines_end: func.end_line,
                        },
                    };

                    if failed_function_keys.contains(&key) {
                        // Function has verification errors
                        failed.push(location);
                    } else if func.has_trusted_assumption {
                        // Function has assume() or admit() - not fully verified
                        unverified.push(location);
                    } else {
                        // Function passed verification without trusted assumptions
                        verified.push(location);
                    }
                }

                (failed, verified, unverified)
            };

        // Apply filters if provided
        let filter_fn = |loc: &FunctionLocation| -> bool {
            if let Some(mod_filter) = module_filter {
                let module_path = mod_filter.replace("::", "/");
                if !loc.code_path.contains(&format!("/{}.rs", module_path))
                    && !loc.code_path.contains(&format!("/{}/", module_path))
                {
                    return false;
                }
            }
            if let Some(func_filter) = function_filter {
                if loc.display_name != func_filter {
                    return false;
                }
            }
            true
        };

        let filtered_failed: Vec<_> = failed_locations
            .into_iter()
            .filter(|l| filter_fn(l))
            .collect();
        let filtered_verified: Vec<_> = verified_locations
            .into_iter()
            .filter(|l| filter_fn(l))
            .collect();
        let filtered_unverified: Vec<_> = unverified_locations
            .into_iter()
            .filter(|l| filter_fn(l))
            .collect();

        let total_functions =
            filtered_failed.len() + filtered_verified.len() + filtered_unverified.len();

        AnalysisResult {
            status,
            summary: AnalysisSummary {
                total_functions,
                failed_functions: filtered_failed.len(),
                verified_functions: filtered_verified.len(),
                unverified_functions: filtered_unverified.len(),
                verification_errors: verification_failures.len(),
                compilation_errors: compilation_errors.len(),
                compilation_warnings: compilation_warnings.len(),
            },
            verification: VerificationResult {
                failed_functions: filtered_failed,
                verified_functions: filtered_verified,
                unverified_functions: filtered_unverified,
                errors: verification_failures,
            },
            compilation: CompilationResult {
                errors: compilation_errors,
                warnings: compilation_warnings,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_function_at_line_prefers_suffix_match_over_filename() {
        // Simulate the bug: two files with same name but different paths
        let mut all_functions: HashMap<String, Vec<(String, usize)>> = HashMap::new();

        // field_lemmas/constants_lemmas.rs has lemma_one_limbs_bounded_54 at line 52
        all_functions.insert(
            "src/lemmas/field_lemmas/constants_lemmas.rs".to_string(),
            vec![("lemma_one_limbs_bounded_54".to_string(), 52)],
        );

        // edwards_lemmas/constants_lemmas.rs has lemma_edwards_d_limbs_bounded at line 43
        all_functions.insert(
            "src/lemmas/edwards_lemmas/constants_lemmas.rs".to_string(),
            vec![("lemma_edwards_d_limbs_bounded".to_string(), 43)],
        );

        let parser = VerificationParser::new();

        // Error is at edwards_lemmas/constants_lemmas.rs:54
        // Should find lemma_edwards_d_limbs_bounded (starts at 43, contains line 54)
        // NOT lemma_one_limbs_bounded_54 (in field_lemmas, starts at 52)
        let result = parser.find_function_at_line(
            "src/lemmas/edwards_lemmas/constants_lemmas.rs",
            54,
            &all_functions,
        );

        assert_eq!(result, Some("lemma_edwards_d_limbs_bounded".to_string()));
    }

    #[test]
    fn test_find_function_at_line_with_partial_path() {
        let mut all_functions: HashMap<String, Vec<(String, usize)>> = HashMap::new();

        all_functions.insert(
            "../curve25519-dalek/curve25519-dalek/src/lemmas/edwards_lemmas/constants_lemmas.rs"
                .to_string(),
            vec![("lemma_edwards_d_limbs_bounded".to_string(), 43)],
        );

        all_functions.insert(
            "../curve25519-dalek/curve25519-dalek/src/lemmas/field_lemmas/constants_lemmas.rs"
                .to_string(),
            vec![("lemma_one_limbs_bounded_54".to_string(), 52)],
        );

        let parser = VerificationParser::new();

        // Error path from Verus output might be shorter
        let result = parser.find_function_at_line(
            "curve25519-dalek/src/lemmas/edwards_lemmas/constants_lemmas.rs",
            54,
            &all_functions,
        );

        assert_eq!(result, Some("lemma_edwards_d_limbs_bounded".to_string()));
    }
}
