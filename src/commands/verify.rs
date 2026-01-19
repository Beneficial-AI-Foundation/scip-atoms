//! Verify command - Run Verus verification and analyze results.

use probe_verus::constants::{DATA_DIR, VERIFICATION_CONFIG_FILE, VERIFICATION_OUTPUT_FILE};
use probe_verus::verification::{
    enrich_with_code_names, AnalysisResult, AnalysisStatus, VerificationAnalyzer, VerusRunner,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Cached verification configuration.
#[derive(Serialize, Deserialize)]
pub struct VerificationConfig {
    pub project_path: String,
    pub package: Option<String>,
    pub exit_code: i32,
}

/// Get the path to the cached verification output file.
fn cache_output_file() -> String {
    format!("{}/{}", DATA_DIR, VERIFICATION_OUTPUT_FILE)
}

/// Get the path to the cached verification config file.
fn cache_config_file() -> String {
    format!("{}/{}", DATA_DIR, VERIFICATION_CONFIG_FILE)
}

/// Execute the verify command.
///
/// Runs Verus verification on a project and analyzes results.
/// Supports caching for quick re-analysis.
#[allow(clippy::too_many_arguments)]
pub fn cmd_verify(
    project_path: Option<PathBuf>,
    from_file: Option<PathBuf>,
    exit_code_arg: Option<i32>,
    package: Option<String>,
    verify_only_module: Option<String>,
    verify_function: Option<String>,
    output: Option<PathBuf>,
    no_cache: bool,
    with_atoms: Option<Option<PathBuf>>,
) {
    // Determine the project path and verification output source
    let (project_path, verification_output, exit_code) = get_verification_data(
        project_path,
        from_file,
        exit_code_arg,
        package.clone(),
        no_cache,
    );

    // Analyze the output
    let analyzer = VerificationAnalyzer::new();
    let mut result = analyzer.analyze_output(
        &project_path,
        &verification_output,
        Some(exit_code),
        verify_only_module.as_deref(),
        verify_function.as_deref(),
    );

    // Enrich with code-names if requested
    if let Some(atoms_path_opt) = with_atoms {
        enrich_result_with_code_names(&mut result, atoms_path_opt);
    }

    // Write JSON output
    let output_path = output.unwrap_or_else(|| PathBuf::from("proofs.json"));
    let json = serde_json::to_string_pretty(&result).expect("Failed to serialize JSON");
    std::fs::write(&output_path, &json).expect("Failed to write JSON output");

    // Print summary
    print_summary(&result);

    // Print failed functions if any
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

    // Print compilation errors if any
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

/// Get verification data from either running verification or using cached data.
fn get_verification_data(
    project_path: Option<PathBuf>,
    from_file: Option<PathBuf>,
    exit_code_arg: Option<i32>,
    package: Option<String>,
    no_cache: bool,
) -> (PathBuf, String, i32) {
    if let Some(ref path) = project_path {
        get_verification_data_from_project(path, from_file, exit_code_arg, package, no_cache)
    } else {
        get_verification_data_from_cache()
    }
}

/// Get verification data from a project (running verification or using a file).
fn get_verification_data_from_project(
    path: &Path,
    from_file: Option<PathBuf>,
    exit_code_arg: Option<i32>,
    package: Option<String>,
    no_cache: bool,
) -> (PathBuf, String, i32) {
    if !path.exists() {
        eprintln!("Error: Project path does not exist: {}", path.display());
        std::process::exit(1);
    }

    let (output, code) = if let Some(ref output_file) = from_file {
        get_output_from_file(output_file, exit_code_arg)
    } else {
        run_verification(path, package.as_deref(), no_cache, &package)
    };

    (path.to_path_buf(), output, code)
}

/// Get verification output from an existing file.
fn get_output_from_file(output_file: &PathBuf, exit_code_arg: Option<i32>) -> (String, i32) {
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
}

/// Run Verus verification on the project.
fn run_verification(
    path: &Path,
    package: Option<&str>,
    no_cache: bool,
    package_for_cache: &Option<String>,
) -> (String, i32) {
    println!("════════════════════════════════════════════════════════════");
    println!("  Running Verus verification...");
    println!("════════════════════════════════════════════════════════════");

    let runner = VerusRunner::new();
    match runner.run_verification(path, package, None, None, None) {
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
                cache_verification_output(path, package_for_cache, code, &output);
            }

            (output, code)
        }
        Err(e) => {
            eprintln!("✗ Failed to run verification: {}", e);
            std::process::exit(1);
        }
    }
}

/// Cache verification output to the data directory.
fn cache_verification_output(path: &Path, package: &Option<String>, code: i32, output: &str) {
    if let Err(e) = std::fs::create_dir_all(DATA_DIR) {
        eprintln!("Warning: Could not create data directory: {}", e);
        return;
    }

    // Save verification output
    if let Err(e) = std::fs::write(cache_output_file(), output) {
        eprintln!("Warning: Could not cache verification output: {}", e);
        return;
    }

    // Save config (project path, package, exit code)
    let config = VerificationConfig {
        project_path: path.to_string_lossy().to_string(),
        package: package.clone(),
        exit_code: code,
    };

    if let Ok(config_json) = serde_json::to_string_pretty(&config) {
        if let Err(e) = std::fs::write(cache_config_file(), config_json) {
            eprintln!("Warning: Could not save verification config: {}", e);
        } else {
            println!("Cached verification output to {}", cache_output_file());
        }
    }
}

/// Get verification data from cache.
fn get_verification_data_from_cache() -> (PathBuf, String, i32) {
    println!("════════════════════════════════════════════════════════════");
    println!("  Using cached verification output");
    println!("════════════════════════════════════════════════════════════");

    // Load config
    let config: VerificationConfig = match std::fs::read_to_string(cache_config_file()) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Error: Could not parse {}: {}", cache_config_file(), e);
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
    let output = match std::fs::read_to_string(cache_output_file()) {
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
}

/// Enrich the analysis result with code-names from atoms.json.
fn enrich_result_with_code_names(result: &mut AnalysisResult, atoms_path_opt: Option<PathBuf>) {
    let atoms_path = atoms_path_opt.unwrap_or_else(|| PathBuf::from("atoms.json"));

    if atoms_path.exists() {
        println!("Populating code-names from {}...", atoms_path.display());
        match enrich_with_code_names(result, &atoms_path) {
            Ok(count) => {
                println!("  Enriched {} functions with code-names", count);
            }
            Err(e) => {
                eprintln!("Warning: Failed to enrich with code-names: {}", e);
            }
        }
    } else {
        eprintln!("Warning: atoms.json not found at {}", atoms_path.display());
    }
}

/// Print the verification summary.
fn print_summary(result: &AnalysisResult) {
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
}

/// Internal verify implementation that returns Result for better error handling.
/// Used by the `run` command.
pub fn verify_internal(
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

    // Enrich with code-names if atoms.json exists
    if let Some(atoms) = atoms_path {
        if atoms.exists() {
            if let Err(e) = enrich_with_code_names(&mut result, atoms) {
                eprintln!("    Warning: Failed to enrich with code-names: {}", e);
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

/// Summary of verification results.
#[derive(Clone)]
pub struct VerifySummary {
    pub total_functions: usize,
    pub verified: usize,
    pub failed: usize,
    pub unverified: usize,
}
