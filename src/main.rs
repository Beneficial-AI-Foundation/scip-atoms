//! Probe Verus - Analyze Verus projects: call graphs and verification
//!
//! This tool provides multiple subcommands:
//! - `atomize`: Generate call graph atoms with line numbers from SCIP indexes
//! - `list-functions`: List all functions in a Rust/Verus project
//! - `verify`: Run Verus verification and analyze results (or analyze existing output)
//! - `specify`: Extract function specifications (requires/ensures) to JSON
//! - `run`: Run both atomize and verify (designed for Docker/CI usage)

use clap::{Parser, Subcommand};
use probe_verus::{
    build_call_graph, convert_to_atoms_with_parsed_spans, find_duplicate_scip_names,
    parse_scip_json,
    verification::{enrich_with_scip_names, AnalysisStatus, VerificationAnalyzer, VerusRunner},
    verus_parser::{self, ParsedOutput},
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "probe-verus")]
#[command(author, version, about = "Probe Verus projects: call graphs and verification analysis", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate call graph atoms with line numbers from SCIP indexes
    Atomize {
        /// Path to the Rust/Verus project
        project_path: PathBuf,

        /// Output file path (default: atoms.json)
        #[arg(short, long, default_value = "atoms.json")]
        output: PathBuf,

        /// Force regeneration of the SCIP index
        #[arg(short, long)]
        regenerate_scip: bool,
    },

    /// List all functions in a Rust/Verus project
    #[command(name = "list-functions")]
    ListFunctions {
        /// Path to search (file or directory)
        path: PathBuf,

        /// Output format
        #[arg(short, long, value_enum, default_value = "text")]
        format: OutputFormat,

        /// Exclude Verus constructs (spec, proof, exec) and only include regular functions
        #[arg(long)]
        exclude_verus_constructs: bool,

        /// Exclude trait and impl methods
        #[arg(long)]
        exclude_methods: bool,

        /// Show function visibility (pub/private)
        #[arg(long)]
        show_visibility: bool,

        /// Show function kind (fn, spec fn, proof fn, etc.)
        #[arg(long)]
        show_kind: bool,

        /// Output JSON to specified file
        #[arg(long)]
        json_output: Option<PathBuf>,
    },

    /// Run Verus verification and analyze results, or analyze existing output
    ///
    /// If no project_path is given, uses cached verification output from data/verification_output.txt
    #[command(name = "verify")]
    Verify {
        /// Path to the Rust/Verus project (optional if using cached output)
        project_path: Option<PathBuf>,

        /// Analyze existing verification output file instead of running verification
        #[arg(long)]
        from_file: Option<PathBuf>,

        /// Exit code from the verification command (only used with --from-file)
        #[arg(long)]
        exit_code: Option<i32>,

        /// Package to verify (for workspace projects)
        #[arg(short, long)]
        package: Option<String>,

        /// Module to verify (e.g., backend::serial::u64::field_verus)
        #[arg(long)]
        verify_only_module: Option<String>,

        /// Function to verify
        #[arg(long)]
        verify_function: Option<String>,

        /// Output JSON results to specified file (default: results.json)
        #[arg(long)]
        json_output: Option<PathBuf>,

        /// Don't cache the verification output
        #[arg(long)]
        no_cache: bool,

        /// Enrich results with scip-names from atoms.json file
        /// If no file specified, looks for atoms.json in current directory
        #[arg(long)]
        with_scip_names: Option<Option<PathBuf>>,
    },

    /// Extract function specifications (requires/ensures) to JSON
    Specify {
        /// Path to search (file or directory)
        path: PathBuf,

        /// Output file path (default: specs.json)
        #[arg(long, default_value = "specs.json")]
        json_output: PathBuf,

        /// Path to atoms.json file for scip-name lookup (required for dictionary output)
        #[arg(long)]
        with_scip_names: PathBuf,
    },

    /// Run both atomize and verify commands (designed for Docker/CI usage)
    ///
    /// This is the recommended entrypoint for Docker containers and CI pipelines.
    /// It runs atomize followed by verify, with proper error handling and JSON output.
    Run {
        /// Path to the Rust/Verus project
        project_path: PathBuf,

        /// Output directory for results (default: ./output)
        #[arg(short, long, default_value = "./output")]
        output: PathBuf,

        /// Run only the atomize command
        #[arg(long)]
        atomize_only: bool,

        /// Run only the verify command
        #[arg(long)]
        verify_only: bool,

        /// Package name for workspace projects (passed to verify)
        #[arg(short, long)]
        package: Option<String>,

        /// Force regeneration of the SCIP index
        #[arg(long)]
        regenerate_scip: bool,

        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum OutputFormat {
    /// Just function names, one per line
    Text,
    /// Full JSON output with all details
    Json,
    /// Detailed text output with file locations
    Detailed,
}

fn check_command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn cmd_atomize(project_path: PathBuf, output: PathBuf, regenerate_scip: bool) {
    println!("═══════════════════════════════════════════════════════════");
    println!("  Probe Verus - Atomize: Generate Call Graph Data");
    println!("═══════════════════════════════════════════════════════════");
    println!();

    // Verify project path exists
    if !project_path.exists() {
        eprintln!(
            "✗ Error: Project path does not exist: {}",
            project_path.display()
        );
        std::process::exit(1);
    }

    // Check if it's a valid Rust project
    let cargo_toml = project_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        eprintln!(
            "✗ Error: Not a valid Rust project (Cargo.toml not found): {}",
            project_path.display()
        );
        std::process::exit(1);
    }
    println!("  ✓ Valid Rust project found");

    // Check for existing SCIP JSON in data/ folder
    let data_dir = project_path.join("data");
    let cached_scip_path = data_dir.join("index.scip");
    let cached_json_path = data_dir.join("index.scip.json");

    // Use cached JSON if available and not regenerating
    if cached_json_path.exists() && !regenerate_scip {
        println!(
            "  ✓ Found existing SCIP JSON at {}",
            cached_json_path.display()
        );
        println!("    (use --regenerate-scip to force regeneration)");
        println!();
    } else {
        // Need to generate - check prerequisites
        if !check_command_exists("verus-analyzer") {
            eprintln!("✗ Error: verus-analyzer not found in PATH");
            eprintln!("  Install with: rustup component add verus-analyzer");
            std::process::exit(1);
        }
        if !check_command_exists("scip") {
            eprintln!("✗ Error: scip not found in PATH");
            eprintln!("  Install with: cargo install scip-cli");
            std::process::exit(1);
        }
        println!("  ✓ Prerequisites verified (verus-analyzer, scip)");
        println!();

        // Run verus-analyzer scip to generate index
        let reason = if regenerate_scip {
            "(regeneration requested)"
        } else {
            "(no existing SCIP data found)"
        };
        println!(
            "Generating SCIP index for {} {}...",
            project_path.display(),
            reason
        );
        println!("  (This may take a while for large projects)");

        let scip_status = Command::new("verus-analyzer")
            .args(["scip", "."])
            .current_dir(&project_path)
            .status();

        match scip_status {
            Ok(status) if status.success() => {
                println!("  ✓ SCIP index generated successfully");
            }
            Ok(status) => {
                eprintln!(
                    "✗ Error: verus-analyzer scip failed with status: {}",
                    status
                );
                eprintln!("  Make sure the project compiles successfully first");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("✗ Error: Failed to run verus-analyzer: {}", e);
                std::process::exit(1);
            }
        }

        let generated_scip_path = project_path.join("index.scip");
        if !generated_scip_path.exists() {
            eprintln!(
                "✗ Error: index.scip not found at {}",
                generated_scip_path.display()
            );
            eprintln!("  verus-analyzer scip may have failed silently");
            std::process::exit(1);
        }

        // Ensure data directory exists
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");
        }

        // Move the generated index.scip to data/ folder
        std::fs::rename(&generated_scip_path, &cached_scip_path)
            .expect("Failed to move index.scip to data folder");
        println!("  ✓ index.scip saved to {}", cached_scip_path.display());

        // Convert SCIP to JSON and save to data/ folder
        println!("Converting index.scip to JSON...");

        let scip_output = Command::new("scip")
            .args(["print", "--json", cached_scip_path.to_str().unwrap()])
            .output();

        match scip_output {
            Ok(output) if output.status.success() => {
                std::fs::write(&cached_json_path, output.stdout)
                    .expect("Failed to write SCIP JSON file");
                println!("  ✓ SCIP JSON saved to {}", cached_json_path.display());
            }
            Ok(output) => {
                eprintln!("✗ Error: scip print failed with status: {}", output.status);
                if !output.stderr.is_empty() {
                    eprintln!("  stderr: {}", String::from_utf8_lossy(&output.stderr));
                }
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("✗ Error: Failed to run scip: {}", e);
                std::process::exit(1);
            }
        }
        println!();
    }

    // Parse SCIP JSON and build call graph
    println!("Parsing SCIP JSON and building call graph...");

    let scip_index = match parse_scip_json(cached_json_path.to_str().unwrap()) {
        Ok(idx) => idx,
        Err(e) => {
            eprintln!("✗ Failed to parse SCIP JSON: {}", e);
            std::process::exit(1);
        }
    };

    let (call_graph, symbol_to_display_name) = build_call_graph(&scip_index);
    println!("  ✓ Call graph built with {} functions", call_graph.len());
    println!();

    // Convert to atoms format with line numbers
    println!("Converting to atoms format with accurate line numbers...");
    println!("  Parsing source files with verus_syn for accurate function spans...");

    let atoms =
        convert_to_atoms_with_parsed_spans(&call_graph, &symbol_to_display_name, &project_path);
    println!("  ✓ Converted {} functions to atoms format", atoms.len());

    // Check for duplicate scip_names - these are now a fatal error
    let duplicates = find_duplicate_scip_names(&atoms);
    if !duplicates.is_empty() {
        eprintln!();
        eprintln!(
            "✗ ERROR: Found {} duplicate scip_name(s):",
            duplicates.len()
        );
        for dup in &duplicates {
            eprintln!("    - '{}'", dup.scip_name);
            for occ in &dup.occurrences {
                eprintln!(
                    "      at {}:{} ({})",
                    occ.code_path, occ.lines_start, occ.display_name
                );
            }
        }
        eprintln!();
        eprintln!("    Duplicate scip_names cannot be used as dictionary keys.");
        eprintln!("    This may indicate trait implementations that cannot be distinguished.");
        eprintln!("    Consider filing an issue if this is unexpected.");
        std::process::exit(1);
    }

    // Convert atoms list to dictionary keyed by scip_name
    let atoms_dict: std::collections::HashMap<String, _> = atoms
        .into_iter()
        .map(|atom| (atom.scip_name.clone(), atom))
        .collect();

    // Write the output
    let json = serde_json::to_string_pretty(&atoms_dict).expect("Failed to serialize JSON");
    std::fs::write(&output, &json).expect("Failed to write output file");

    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("  ✓ SUCCESS");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    println!("Output written to: {}", output.display());
    println!();
    println!("Summary:");
    println!("  - Total functions: {}", atoms_dict.len());
    println!(
        "  - Total dependencies: {}",
        atoms_dict
            .values()
            .map(|a| a.dependencies.len())
            .sum::<usize>()
    );
    println!("  - Output format: dictionary keyed by scip_name");
    println!();
}

fn cmd_functions(
    path: PathBuf,
    format: OutputFormat,
    exclude_verus_constructs: bool,
    exclude_methods: bool,
    show_visibility: bool,
    show_kind: bool,
    json_output: Option<PathBuf>,
) {
    if !path.exists() {
        eprintln!("Error: Path does not exist: {}", path.display());
        std::process::exit(1);
    }

    let include_verus_constructs = !exclude_verus_constructs;
    let include_methods = !exclude_methods;

    let output: ParsedOutput = verus_parser::parse_all_functions(
        &path,
        include_verus_constructs,
        include_methods,
        show_visibility,
        show_kind,
    );

    // Determine actual output format
    let actual_format = if json_output.is_some() {
        OutputFormat::Json
    } else {
        format
    };

    match actual_format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&output).unwrap();
            if let Some(output_path) = json_output {
                std::fs::write(&output_path, &json).expect("Failed to write JSON output");
                println!("JSON output written to {}", output_path.display());
            } else {
                println!("{}", json);
            }
        }
        OutputFormat::Text => {
            // Just print function names, one per line
            let mut names: Vec<_> = output.functions.iter().map(|f| f.name.as_str()).collect();
            names.sort();
            names.dedup();
            for name in names {
                println!("{}", name);
            }
        }
        OutputFormat::Detailed => {
            for func in &output.functions {
                print!("{}", func.name);
                if let Some(ref kind) = func.kind {
                    print!(" [{}]", kind);
                }
                if let Some(ref vis) = func.visibility {
                    print!(" ({})", vis);
                }
                if let Some(ref file) = func.file {
                    print!(" @ {}:{}:{}", file, func.start_line, func.end_line);
                }
                if let Some(ref context) = func.context {
                    print!(" in {}", context);
                }
                println!();
            }
            println!(
                "\nSummary: {} functions in {} files",
                output.summary.total_functions, output.summary.total_files
            );
        }
    }
}

// Cache directory and files
const DATA_DIR: &str = "data";
const CACHE_OUTPUT_FILE: &str = "data/verification_output.txt";
const CACHE_CONFIG_FILE: &str = "data/verification_config.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct VerificationConfig {
    project_path: String,
    package: Option<String>,
    exit_code: i32,
}

#[allow(clippy::too_many_arguments)]
fn cmd_verify(
    project_path: Option<PathBuf>,
    from_file: Option<PathBuf>,
    exit_code_arg: Option<i32>,
    package: Option<String>,
    verify_only_module: Option<String>,
    verify_function: Option<String>,
    json_output: Option<PathBuf>,
    no_cache: bool,
    with_scip_names: Option<Option<PathBuf>>,
) {
    // Determine the project path and verification output source
    let (project_path, verification_output, exit_code) = if let Some(ref path) = project_path {
        // Project path provided
        if !path.exists() {
            eprintln!("Error: Project path does not exist: {}", path.display());
            std::process::exit(1);
        }

        let (output, code) = if let Some(ref output_file) = from_file {
            // Use provided output file
            if !output_file.exists() {
                eprintln!(
                    "Error: Output file does not exist: {}",
                    output_file.display()
                );
                std::process::exit(1);
            }

            let content = match std::fs::read_to_string(output_file) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error reading output file: {}", e);
                    std::process::exit(1);
                }
            };

            println!(
                "Analyzing verification output from: {}",
                output_file.display()
            );
            (content, exit_code_arg.unwrap_or(0))
        } else {
            // Run verification
            println!("════════════════════════════════════════════════════════════");
            println!("  Running Verus verification...");
            println!("════════════════════════════════════════════════════════════");

            let runner = VerusRunner::new();
            match runner.run_verification(
                path,
                package.as_deref(),
                verify_only_module.as_deref(),
                verify_function.as_deref(),
                None,
            ) {
                Ok((output, code)) => {
                    println!();
                    println!("════════════════════════════════════════════════════════════");
                    println!("  Verification completed with exit code: {}", code);
                    println!("════════════════════════════════════════════════════════════");
                    println!();

                    // Quick status check
                    if output.contains("verification results::") {
                        if output.contains(", 0 errors") {
                            println!("✓ Verification succeeded!");
                        } else {
                            println!("✗ Verification failed with errors");
                        }
                    } else if code != 0 {
                        println!("✗ Compilation or verification failed");
                    }

                    // Cache the output unless --no-cache is specified
                    if !no_cache {
                        if let Err(e) = std::fs::create_dir_all(DATA_DIR) {
                            eprintln!("Warning: Could not create data directory: {}", e);
                        } else {
                            // Save verification output
                            if let Err(e) = std::fs::write(CACHE_OUTPUT_FILE, &output) {
                                eprintln!("Warning: Could not cache verification output: {}", e);
                            }
                            // Save config (project path, package, exit code)
                            let config = VerificationConfig {
                                project_path: path.to_string_lossy().to_string(),
                                package: package.clone(),
                                exit_code: code,
                            };
                            if let Ok(config_json) = serde_json::to_string_pretty(&config) {
                                if let Err(e) = std::fs::write(CACHE_CONFIG_FILE, config_json) {
                                    eprintln!("Warning: Could not save verification config: {}", e);
                                } else {
                                    println!("Cached verification output to {}", CACHE_OUTPUT_FILE);
                                }
                            }
                        }
                    }

                    (output, code)
                }
                Err(e) => {
                    eprintln!("✗ Failed to run verification: {}", e);
                    std::process::exit(1);
                }
            }
        };

        (path.clone(), output, code)
    } else {
        // No project path - use cached output
        println!("════════════════════════════════════════════════════════════");
        println!("  Using cached verification output");
        println!("════════════════════════════════════════════════════════════");

        // Load config
        let config: VerificationConfig = match std::fs::read_to_string(CACHE_CONFIG_FILE) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: Could not parse {}: {}", CACHE_CONFIG_FILE, e);
                    eprintln!("Run with a project path first to cache verification output.");
                    std::process::exit(1);
                }
            },
            Err(_) => {
                eprintln!("Error: No cached verification found.");
                eprintln!("Run with a project path first: probe-verus verify <project-path>");
                std::process::exit(1);
            }
        };

        // Load cached output
        let output = match std::fs::read_to_string(CACHE_OUTPUT_FILE) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: Could not read cached output: {}", e);
                std::process::exit(1);
            }
        };

        let path = PathBuf::from(&config.project_path);
        if !path.exists() {
            eprintln!(
                "Warning: Cached project path no longer exists: {}",
                path.display()
            );
        }

        println!("  Project: {}", config.project_path);
        if let Some(ref pkg) = config.package {
            println!("  Package: {}", pkg);
        }
        println!("  Exit code: {}", config.exit_code);
        println!("════════════════════════════════════════════════════════════");
        println!();

        (path, output, config.exit_code)
    };

    // Analyze the output
    let analyzer = VerificationAnalyzer::new();
    let mut result = analyzer.analyze_output(
        &project_path,
        &verification_output,
        Some(exit_code),
        verify_only_module.as_deref(),
        verify_function.as_deref(),
    );

    // Enrich with scip-names if requested
    if let Some(atoms_path_opt) = with_scip_names {
        // Use provided path or default to atoms.json
        let atoms_path = atoms_path_opt.unwrap_or_else(|| PathBuf::from("atoms.json"));

        if atoms_path.exists() {
            println!("Populating scip-names from {}...", atoms_path.display());
            match enrich_with_scip_names(&mut result, &atoms_path) {
                Ok(count) => {
                    println!("  Enriched {} functions with scip-names", count);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to enrich with scip-names: {}", e);
                }
            }
        } else {
            eprintln!("Warning: atoms.json not found at {}", atoms_path.display());
        }
    }

    // Always write to JSON file (default: results.json)
    let output_path = json_output.unwrap_or_else(|| PathBuf::from("results.json"));
    let json = serde_json::to_string_pretty(&result).expect("Failed to serialize JSON");
    std::fs::write(&output_path, &json).expect("Failed to write JSON output");

    // Print summary
    println!();
    println!("Summary:");
    println!("  Status: {:?}", result.status);
    println!(
        "  Total verifiable functions: {}",
        result.summary.total_functions
    );
    println!("  Verified: {}", result.summary.verified_functions);
    println!("  Failed: {}", result.summary.failed_functions);
    println!(
        "  Unverified (assume/admit): {}",
        result.summary.unverified_functions
    );

    if !result.verification.failed_functions.is_empty() {
        println!();
        println!("Failed functions:");
        for func in &result.verification.failed_functions {
            println!(
                "  - {} @ {}:{}",
                func.display_name, func.code_path, func.code_text.lines_start
            );
        }
    }

    if !result.compilation.errors.is_empty() {
        println!();
        println!("Compilation errors:");
        for err in &result.compilation.errors {
            println!("  - {}", err.message);
            if let Some(ref file) = err.file {
                if let Some(line) = err.line {
                    println!("    at {}:{}", file, line);
                }
            }
        }
    }

    println!();
    println!("JSON output written to {}", output_path.display());

    // Exit with appropriate code
    if result.status != AnalysisStatus::Success {
        std::process::exit(1);
    }
}

fn cmd_specify(path: PathBuf, output: PathBuf, atoms_path: PathBuf) {
    use std::collections::HashMap;

    if !path.exists() {
        eprintln!("Error: Path does not exist: {}", path.display());
        std::process::exit(1);
    }

    if !atoms_path.exists() {
        eprintln!("Error: atoms.json not found at {}", atoms_path.display());
        std::process::exit(1);
    }

    // Load atoms.json to get scip-name mappings
    #[derive(serde::Deserialize)]
    struct AtomEntry {
        #[serde(rename = "display-name")]
        display_name: String,
        #[serde(rename = "code-path")]
        code_path: String,
        #[serde(rename = "code-text")]
        code_text: CodeText,
    }

    #[derive(serde::Deserialize)]
    struct CodeText {
        #[serde(rename = "lines-start")]
        lines_start: usize,
    }

    let atoms_content = std::fs::read_to_string(&atoms_path).expect("Failed to read atoms.json");
    let atoms: HashMap<String, AtomEntry> =
        serde_json::from_str(&atoms_content).expect("Failed to parse atoms.json");

    // Parse all functions with spec info (requires/ensures)
    let parsed: ParsedOutput = verus_parser::parse_all_functions(
        &path, true,  // include_verus_constructs
        true,  // include_methods
        false, // show_visibility
        false, // show_kind
    );

    // Helper to extract suffix for path matching
    fn extract_suffix(path: &str) -> &str {
        if let Some(pos) = path.find("/src/") {
            return &path[pos + 1..];
        }
        path
    }

    // Match functions to scip-names and build output dictionary
    const LINE_TOLERANCE: usize = 5;
    let mut output_map: HashMap<String, verus_parser::FunctionInfo> = HashMap::new();
    let mut matched_count = 0;
    let mut unmatched_count = 0;

    for func in parsed.functions {
        let func_path = func.file.as_deref().unwrap_or("");
        let func_suffix = extract_suffix(func_path);
        let func_line = func.start_line;

        // Find best matching atom by path and line
        let mut best_match: Option<&str> = None;
        let mut best_line_diff = usize::MAX;

        for (scip_name, atom) in &atoms {
            let atom_suffix = extract_suffix(&atom.code_path);

            let path_matches = func_path.ends_with(&atom.code_path)
                || atom.code_path.ends_with(func_path)
                || func_suffix == atom_suffix;

            if path_matches && func.name == atom.display_name {
                let line_diff =
                    (func_line as isize - atom.code_text.lines_start as isize).unsigned_abs();

                if line_diff <= LINE_TOLERANCE && line_diff < best_line_diff {
                    best_match = Some(scip_name);
                    best_line_diff = line_diff;

                    if line_diff == 0 {
                        break;
                    }
                }
            }
        }

        if let Some(scip_name) = best_match {
            output_map.insert(scip_name.to_string(), func);
            matched_count += 1;
        } else {
            unmatched_count += 1;
        }
    }

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

/// Result of the run command for JSON output
#[derive(serde::Serialize)]
struct RunResult {
    status: String,
    atomize: Option<AtomizeResult>,
    verify: Option<VerifyResult>,
}

#[derive(serde::Serialize)]
struct AtomizeResult {
    success: bool,
    output_file: String,
    total_functions: Option<usize>,
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct VerifyResult {
    success: bool,
    output_file: String,
    summary: Option<VerifySummary>,
    error: Option<String>,
}

#[derive(serde::Serialize, Clone)]
struct VerifySummary {
    total_functions: usize,
    verified: usize,
    failed: usize,
    unverified: usize,
}

fn cmd_run(
    project_path: PathBuf,
    output_dir: PathBuf,
    atomize_only: bool,
    verify_only: bool,
    package: Option<String>,
    regenerate_scip: bool,
    verbose: bool,
) {
    // Validate project path
    if !project_path.exists() {
        eprintln!(
            "Error: Project path does not exist: {}",
            project_path.display()
        );
        std::process::exit(1);
    }

    let cargo_toml = project_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        eprintln!(
            "Error: Not a valid Rust project (Cargo.toml not found): {}",
            project_path.display()
        );
        std::process::exit(1);
    }

    // Create output directory
    if let Err(e) = std::fs::create_dir_all(&output_dir) {
        eprintln!("Error: Failed to create output directory: {}", e);
        std::process::exit(1);
    }

    let atoms_path = output_dir.join("atoms.json");
    let results_path = output_dir.join("results.json");

    println!("═══════════════════════════════════════════════════════════════");
    println!("  probe-verus run");
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("  Project: {}", project_path.display());
    println!("  Output:  {}", output_dir.display());
    if let Some(ref pkg) = package {
        println!("  Package: {}", pkg);
    }
    println!();

    let mut run_result = RunResult {
        status: "success".to_string(),
        atomize: None,
        verify: None,
    };

    // === Run atomize ===
    if !verify_only {
        println!("───────────────────────────────────────────────────────────────");
        println!("  Step 1: Atomize (generate call graph)");
        println!("───────────────────────────────────────────────────────────────");
        println!();

        let atomize_result =
            run_atomize_internal(&project_path, &atoms_path, regenerate_scip, verbose);

        match &atomize_result {
            Ok(count) => {
                println!("  ✓ Atomize completed: {} functions", count);
                println!("  → {}", atoms_path.display());
                run_result.atomize = Some(AtomizeResult {
                    success: true,
                    output_file: atoms_path.display().to_string(),
                    total_functions: Some(*count),
                    error: None,
                });
            }
            Err(e) => {
                eprintln!("  ✗ Atomize failed: {}", e);
                run_result.status = "atomize_failed".to_string();
                run_result.atomize = Some(AtomizeResult {
                    success: false,
                    output_file: atoms_path.display().to_string(),
                    total_functions: None,
                    error: Some(e.clone()),
                });
            }
        }
        println!();
    }

    // === Run verify ===
    if !atomize_only {
        println!("───────────────────────────────────────────────────────────────");
        println!("  Step 2: Verify (run Verus verification)");
        println!("───────────────────────────────────────────────────────────────");
        println!();

        let verify_result = run_verify_internal(
            &project_path,
            &results_path,
            package.as_deref(),
            if atoms_path.exists() {
                Some(&atoms_path)
            } else {
                None
            },
            verbose,
        );

        match &verify_result {
            Ok(summary) => {
                println!("  ✓ Verify completed");
                println!("    Total:      {}", summary.total_functions);
                println!("    Verified:   {}", summary.verified);
                println!("    Failed:     {}", summary.failed);
                println!("    Unverified: {}", summary.unverified);
                println!("  → {}", results_path.display());

                run_result.verify = Some(VerifyResult {
                    success: true,
                    output_file: results_path.display().to_string(),
                    summary: Some(summary.clone()),
                    error: None,
                });

                // Mark as verification_failed if there are failures
                if summary.failed > 0 && run_result.status == "success" {
                    run_result.status = "verification_failed".to_string();
                }
            }
            Err(e) => {
                eprintln!("  ✗ Verify failed: {}", e);
                if run_result.status == "success" {
                    run_result.status = "verify_failed".to_string();
                }
                run_result.verify = Some(VerifyResult {
                    success: false,
                    output_file: results_path.display().to_string(),
                    summary: None,
                    error: Some(e.clone()),
                });
            }
        }
        println!();
    }

    // === Summary ===
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Summary");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    if let Some(ref a) = run_result.atomize {
        if a.success {
            println!("  atomize: ✓ Success → {}", a.output_file);
        } else {
            println!("  atomize: ✗ Failed");
        }
    }

    if let Some(ref v) = run_result.verify {
        if v.success {
            println!("  verify:  ✓ Success → {}", v.output_file);
        } else {
            println!("  verify:  ✗ Failed");
        }
    }

    println!();
    println!("  Status: {}", run_result.status);
    println!();

    // Write summary JSON
    let summary_path = output_dir.join("run_summary.json");
    if let Ok(json) = serde_json::to_string_pretty(&run_result) {
        if let Err(e) = std::fs::write(&summary_path, &json) {
            eprintln!("Warning: Could not write summary: {}", e);
        }
    }

    // Exit with appropriate code
    let exit_code = match run_result.status.as_str() {
        "success" => 0,
        "verification_failed" => 0, // Verification ran successfully, just found failures
        _ => 1,
    };
    std::process::exit(exit_code);
}

/// Internal atomize implementation that returns Result for better error handling
fn run_atomize_internal(
    project_path: &PathBuf,
    output: &PathBuf,
    regenerate_scip: bool,
    verbose: bool,
) -> Result<usize, String> {
    // Check for existing SCIP JSON
    let data_dir = project_path.join("data");
    let cached_scip_path = data_dir.join("index.scip");
    let cached_json_path = data_dir.join("index.scip.json");

    // Generate SCIP if needed
    if !cached_json_path.exists() || regenerate_scip {
        if !check_command_exists("verus-analyzer") {
            return Err("verus-analyzer not found in PATH".to_string());
        }
        if !check_command_exists("scip") {
            return Err("scip not found in PATH".to_string());
        }

        if verbose {
            println!("    Generating SCIP index...");
        }

        let scip_status = Command::new("verus-analyzer")
            .args(["scip", "."])
            .current_dir(project_path)
            .stdout(if verbose {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .stderr(if verbose {
                Stdio::inherit()
            } else {
                Stdio::null()
            })
            .status();

        match scip_status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                return Err(format!(
                    "verus-analyzer scip failed with status: {}",
                    status
                ))
            }
            Err(e) => return Err(format!("Failed to run verus-analyzer: {}", e)),
        }

        let generated_scip_path = project_path.join("index.scip");
        if !generated_scip_path.exists() {
            return Err("index.scip not generated".to_string());
        }

        // Move to data directory
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            return Err(format!("Failed to create data directory: {}", e));
        }
        if let Err(e) = std::fs::rename(&generated_scip_path, &cached_scip_path) {
            return Err(format!("Failed to move index.scip: {}", e));
        }

        // Convert to JSON
        if verbose {
            println!("    Converting to JSON...");
        }

        let scip_output = Command::new("scip")
            .args(["print", "--json", cached_scip_path.to_str().unwrap()])
            .output();

        match scip_output {
            Ok(output) if output.status.success() => {
                if let Err(e) = std::fs::write(&cached_json_path, output.stdout) {
                    return Err(format!("Failed to write SCIP JSON: {}", e));
                }
            }
            Ok(output) => return Err(format!("scip print failed: {}", output.status)),
            Err(e) => return Err(format!("Failed to run scip: {}", e)),
        }
    }

    // Parse and build call graph
    let scip_index = parse_scip_json(cached_json_path.to_str().unwrap())
        .map_err(|e| format!("Failed to parse SCIP JSON: {}", e))?;

    let (call_graph, symbol_to_display_name) = build_call_graph(&scip_index);
    let atoms =
        convert_to_atoms_with_parsed_spans(&call_graph, &symbol_to_display_name, project_path);

    // Check for duplicates
    let duplicates = find_duplicate_scip_names(&atoms);
    if !duplicates.is_empty() {
        return Err(format!("Found {} duplicate scip_name(s)", duplicates.len()));
    }

    let count = atoms.len();

    // Convert to dictionary and write
    let atoms_dict: std::collections::HashMap<String, _> = atoms
        .into_iter()
        .map(|atom| (atom.scip_name.clone(), atom))
        .collect();

    let json = serde_json::to_string_pretty(&atoms_dict)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
    std::fs::write(output, &json).map_err(|e| format!("Failed to write output: {}", e))?;

    Ok(count)
}

/// Internal verify implementation that returns Result for better error handling
fn run_verify_internal(
    project_path: &Path,
    output: &Path,
    package: Option<&str>,
    atoms_path: Option<&Path>,
    verbose: bool,
) -> Result<VerifySummary, String> {
    let runner = VerusRunner::new();

    let (verification_output, exit_code) = runner
        .run_verification(project_path, package, None, None, None)
        .map_err(|e| format!("Failed to run verification: {}", e))?;

    if verbose {
        println!("{}", verification_output);
    }

    let analyzer = VerificationAnalyzer::new();
    let mut result = analyzer.analyze_output(
        project_path,
        &verification_output,
        Some(exit_code),
        None,
        None,
    );

    // Enrich with scip-names if atoms.json exists
    if let Some(atoms) = atoms_path {
        if atoms.exists() {
            if let Err(e) = enrich_with_scip_names(&mut result, atoms) {
                eprintln!("    Warning: Failed to enrich with scip-names: {}", e);
            }
        }
    }

    // Write results
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
    std::fs::write(output, &json).map_err(|e| format!("Failed to write output: {}", e))?;

    Ok(VerifySummary {
        total_functions: result.summary.total_functions,
        verified: result.summary.verified_functions,
        failed: result.summary.failed_functions,
        unverified: result.summary.unverified_functions,
    })
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Atomize {
            project_path,
            output,
            regenerate_scip,
        } => {
            cmd_atomize(project_path, output, regenerate_scip);
        }
        Commands::ListFunctions {
            path,
            format,
            exclude_verus_constructs,
            exclude_methods,
            show_visibility,
            show_kind,
            json_output,
        } => {
            cmd_functions(
                path,
                format,
                exclude_verus_constructs,
                exclude_methods,
                show_visibility,
                show_kind,
                json_output,
            );
        }
        Commands::Verify {
            project_path,
            from_file,
            exit_code,
            package,
            verify_only_module,
            verify_function,
            json_output,
            no_cache,
            with_scip_names,
        } => {
            cmd_verify(
                project_path,
                from_file,
                exit_code,
                package,
                verify_only_module,
                verify_function,
                json_output,
                no_cache,
                with_scip_names,
            );
        }
        Commands::Specify {
            path,
            json_output,
            with_scip_names,
        } => {
            cmd_specify(path, json_output, with_scip_names);
        }
        Commands::Run {
            project_path,
            output,
            atomize_only,
            verify_only,
            package,
            regenerate_scip,
            verbose,
        } => {
            cmd_run(
                project_path,
                output,
                atomize_only,
                verify_only,
                package,
                regenerate_scip,
                verbose,
            );
        }
    }
}
