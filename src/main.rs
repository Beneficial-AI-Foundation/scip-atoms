use scip_atoms::{build_call_graph, convert_to_atoms_with_parsed_spans, parse_scip_json};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn check_command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn print_usage(program_name: &str) {
    eprintln!(
        "Usage: {} <project_path> [output_json] [--regenerate-scip]",
        program_name
    );
    eprintln!();
    eprintln!("This tool will:");
    eprintln!("  1. Look for existing SCIP data in <project_path>/data/");
    eprintln!("  2. If not found (or --regenerate-scip is used), generate SCIP index and JSON");
    eprintln!("  3. Parse the SCIP JSON and build call graph");
    eprintln!("  4. Generate an atoms JSON with line numbers instead of function bodies");
    eprintln!();
    eprintln!("Arguments:");
    eprintln!("  <project_path>       Path to the Rust/Verus project");
    eprintln!("  [output_json]        Output file path (default: atoms.json)");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --regenerate-scip, -r    Force regeneration of the SCIP index and JSON");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} ./my-rust-project", program_name);
    eprintln!("  {} ./my-rust-project output.json", program_name);
    eprintln!("  {} ./my-rust-project --regenerate-scip", program_name);
    eprintln!("  {} ./my-rust-project output.json -r", program_name);
    eprintln!();
    eprintln!("Prerequisites (only needed when generating SCIP data):");
    eprintln!("  - verus-analyzer must be installed (rustup component add verus-analyzer)");
    eprintln!("  - scip-cli must be installed (cargo install scip-cli)");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse arguments
    let mut project_path: Option<String> = None;
    let mut output_path: Option<String> = None;
    let mut regenerate_scip = false;

    for arg in args.iter().skip(1) {
        if arg == "--regenerate-scip" || arg == "-r" {
            regenerate_scip = true;
        } else if arg.starts_with('-') {
            eprintln!("Unknown option: {}", arg);
            print_usage(&args[0]);
        } else if project_path.is_none() {
            project_path = Some(arg.clone());
        } else if output_path.is_none() {
            output_path = Some(arg.clone());
        } else {
            eprintln!("Too many arguments");
            print_usage(&args[0]);
        }
    }

    let project_path = match project_path {
        Some(p) => p,
        None => {
            eprintln!("Missing required argument: <project_path>");
            print_usage(&args[0]);
            unreachable!()
        }
    };

    // Default output path is "atoms.json" in the current directory
    let output_path = output_path.unwrap_or_else(|| "atoms.json".to_string());

    println!("═══════════════════════════════════════════════════════════");
    println!("  SCIP Atoms - Generate Compact Call Graph Data");
    println!("═══════════════════════════════════════════════════════════");
    println!();

    // Verify project path exists
    let project_path_buf = PathBuf::from(&project_path);
    if !project_path_buf.exists() {
        eprintln!("✗ Error: Project path does not exist: {}", project_path);
        std::process::exit(1);
    }

    // Check if it's a valid Rust project
    let cargo_toml = project_path_buf.join("Cargo.toml");
    if !cargo_toml.exists() {
        eprintln!(
            "✗ Error: Not a valid Rust project (Cargo.toml not found): {}",
            project_path
        );
        std::process::exit(1);
    }
    println!("  ✓ Valid Rust project found");

    // Check for existing SCIP JSON in data/ folder
    let data_dir = project_path_buf.join("data");
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
        println!("Generating SCIP index for {} {}...", project_path, reason);
        println!("  (This may take a while for large projects)");

        let scip_status = Command::new("verus-analyzer")
            .args(["scip", "."])
            .current_dir(&project_path_buf)
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

        let generated_scip_path = project_path_buf.join("index.scip");
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
        convert_to_atoms_with_parsed_spans(&call_graph, &symbol_to_display_name, &project_path_buf);
    println!("  ✓ Converted {} functions to atoms format", atoms.len());

    // Write the output
    let json = serde_json::to_string_pretty(&atoms).expect("Failed to serialize JSON");
    std::fs::write(&output_path, &json).expect("Failed to write output file");

    println!();
    println!("═══════════════════════════════════════════════════════════");
    println!("  ✓ SUCCESS");
    println!("═══════════════════════════════════════════════════════════");
    println!();
    println!("Output written to: {}", output_path);
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
