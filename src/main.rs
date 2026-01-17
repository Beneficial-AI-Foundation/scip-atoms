//! Probe Verus - Analyze Verus projects: call graphs and verification
//!
//! This tool provides multiple subcommands:
//! - `atomize`: Generate call graph atoms with line numbers from SCIP indexes
//! - `list-functions`: List all functions in a Rust/Verus project
//! - `verify`: Run Verus verification and analyze results (or analyze existing output)
//! - `specify`: Extract function specifications (requires/ensures) to JSON
//! - `run`: Run both atomize and verify (designed for Docker/CI usage)

use clap::{Parser, Subcommand};
use probe_verus::constants::{DEFAULT_ATOMS_OUTPUT, DEFAULT_OUTPUT_DIR, DEFAULT_SPECS_OUTPUT};
use std::path::PathBuf;

// Import command implementations
mod commands;
use commands::{cmd_atomize, cmd_functions, cmd_run, cmd_specify, cmd_verify, OutputFormat};

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
        #[arg(short, long, default_value = DEFAULT_ATOMS_OUTPUT)]
        output: PathBuf,

        /// Force regeneration of the SCIP index
        #[arg(short, long)]
        regenerate_scip: bool,

        /// Include dependencies-with-locations (detailed per-call location info)
        #[arg(long)]
        with_locations: bool,
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

        /// Enrich results with code-names from atoms.json file
        /// If no file specified, looks for atoms.json in current directory
        #[arg(long)]
        with_code_names: Option<Option<PathBuf>>,
    },

    /// Extract function specifications (requires/ensures) to JSON
    Specify {
        /// Path to search (file or directory)
        path: PathBuf,

        /// Output file path (default: specs.json)
        #[arg(long, default_value = DEFAULT_SPECS_OUTPUT)]
        json_output: PathBuf,

        /// Path to atoms.json file for code-name lookup (required for dictionary output)
        #[arg(long)]
        with_code_names: PathBuf,

        /// Include raw specification text (requires/ensures clauses) in output
        #[arg(long)]
        with_spec_text: bool,
    },

    /// Run both atomize and verify commands (designed for Docker/CI usage)
    ///
    /// This is the recommended entrypoint for Docker containers and CI pipelines.
    /// It runs atomize followed by verify, with proper error handling and JSON output.
    Run {
        /// Path to the Rust/Verus project
        project_path: PathBuf,

        /// Output directory for results (default: ./output)
        #[arg(short, long, default_value = DEFAULT_OUTPUT_DIR)]
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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Atomize {
            project_path,
            output,
            regenerate_scip,
            with_locations,
        } => {
            cmd_atomize(project_path, output, regenerate_scip, with_locations);
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
            with_code_names,
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
                with_code_names,
            );
        }
        Commands::Specify {
            path,
            json_output,
            with_code_names,
            with_spec_text,
        } => {
            cmd_specify(path, json_output, with_code_names, with_spec_text);
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
