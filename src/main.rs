//! SCIP Atoms - Generate compact call graph data and analyze Verus verification
//!
//! This tool provides multiple subcommands:
//! - `atoms`: Generate SCIP-based call graph data with line numbers
//! - `functions`: List all functions in a Rust/Verus project
//! - `verify`: Run Verus verification and analyze results (or analyze existing output)

use clap::{Parser, Subcommand};
use scip_atoms::{
    build_call_graph, convert_to_atoms_with_parsed_spans, find_duplicate_scip_names,
    parse_scip_json,
    verification::{AnalysisStatus, VerificationAnalyzer, VerusRunner},
    verus_parser::{self, ParsedOutput},
};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "scip-atoms")]
#[command(author, version, about = "Generate compact call graph data and analyze Verus verification", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate SCIP-based call graph atoms with line numbers
    Atoms {
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
    Functions {
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

fn cmd_atoms(project_path: PathBuf, output: PathBuf, regenerate_scip: bool) {
    println!("═══════════════════════════════════════════════════════════");
    println!("  SCIP Atoms - Generate Compact Call Graph Data");
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

    // Check for duplicate scip_names
    let duplicates = find_duplicate_scip_names(&atoms);
    if !duplicates.is_empty() {
        println!();
        println!(
            "  ⚠ WARNING: Found {} duplicate scip_name(s):",
            duplicates.len()
        );
        for dup in &duplicates {
            println!("    - '{}'", dup.scip_name);
            for occ in &dup.occurrences {
                println!(
                    "      at {}:{} ({})",
                    occ.code_path, occ.lines_start, occ.display_name
                );
            }
        }
        println!();
        println!("    Duplicate scip_names may indicate trait implementations that");
        println!("    cannot be distinguished. Consider filing an issue if this is");
        println!("    causing problems with downstream tools.");
    }

    // Write the output
    let json = serde_json::to_string_pretty(&atoms).expect("Failed to serialize JSON");
    std::fs::write(&output, &json).expect("Failed to write output file");

    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("  ✓ SUCCESS");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    println!("Output written to: {}", output.display());
    println!();
    println!("Summary:");
    println!("  - Total functions: {}", atoms.len());
    println!(
        "  - Total dependencies: {}",
        atoms.iter().map(|a| a.dependencies.len()).sum::<usize>()
    );
    println!("  - Output format: atoms with line numbers and visibility flags");
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
                eprintln!("Run with a project path first: scip-atoms verify <project-path>");
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
    let result = analyzer.analyze_output(
        &project_path,
        &verification_output,
        Some(exit_code),
        verify_only_module.as_deref(),
        verify_function.as_deref(),
    );

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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Atoms {
            project_path,
            output,
            regenerate_scip,
        } => {
            cmd_atoms(project_path, output, regenerate_scip);
        }
        Commands::Functions {
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
            );
        }
    }
}
