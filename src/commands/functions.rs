//! Functions command - List all functions in a Rust/Verus project.

use probe_verus::verus_parser::{self, ParsedOutput};
use std::path::PathBuf;

/// Output format for function listing.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// Just function names, one per line
    Text,
    /// Full JSON output with all details
    Json,
    /// Detailed text output with file locations
    Detailed,
}

/// Execute the list-functions command.
///
/// Lists all functions in a Rust/Verus project with optional metadata.
pub fn cmd_functions(
    path: PathBuf,
    format: OutputFormat,
    exclude_verus_constructs: bool,
    exclude_methods: bool,
    show_visibility: bool,
    show_kind: bool,
    output: Option<PathBuf>,
) {
    if !path.exists() {
        eprintln!("Error: Path does not exist: {}", path.display());
        std::process::exit(1);
    }

    let include_verus_constructs = !exclude_verus_constructs;
    let include_methods = !exclude_methods;

    let parsed_output: ParsedOutput = verus_parser::parse_all_functions(
        &path,
        include_verus_constructs,
        include_methods,
        show_visibility,
        show_kind,
        false, // include_spec_text - not needed for list-functions
    );

    // Determine actual output format
    let actual_format = if output.is_some() {
        OutputFormat::Json
    } else {
        format
    };

    match actual_format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&parsed_output).unwrap();
            if let Some(output_path) = output {
                std::fs::write(&output_path, &json).expect("Failed to write JSON output");
                println!("JSON output written to {}", output_path.display());
            } else {
                println!("{}", json);
            }
        }
        OutputFormat::Text => {
            // Just print function names, one per line
            let mut names: Vec<_> = parsed_output
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            names.sort();
            names.dedup();
            for name in names {
                println!("{}", name);
            }
        }
        OutputFormat::Detailed => {
            for func in &parsed_output.functions {
                print!("{}", func.name);
                if let Some(ref kind) = func.kind {
                    print!(" [{}]", kind);
                }
                if let Some(ref vis) = func.visibility {
                    print!(" ({})", vis);
                }
                if let Some(ref file) = func.file {
                    print!(
                        " @ {}:{}:{}",
                        file, func.spec_text.lines_start, func.spec_text.lines_end
                    );
                }
                if let Some(ref context) = func.context {
                    print!(" in {}", context);
                }
                println!();
            }
            println!(
                "\nSummary: {} functions in {} files",
                parsed_output.summary.total_functions, parsed_output.summary.total_files
            );
        }
    }
}
