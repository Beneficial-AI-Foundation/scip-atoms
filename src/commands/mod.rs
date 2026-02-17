//! Command implementations for probe-verus CLI.
//!
//! This module contains the implementation of each CLI subcommand:
//! - `atomize`: Generate call graph atoms from SCIP indexes
//! - `verify`: Run Verus verification and analyze results
//! - `functions`: List all functions in a project
//! - `specify`: Extract function specifications to JSON
//! - `specs-data`: Generate specs_data.json for the specs browser
//! - `tracked-csv`: Generate curve25519_functions.csv for the dashboard
//! - `stubify`: Convert .md files with YAML frontmatter to JSON
//! - `run`: Run both atomize and verify (for CI/Docker)

mod atomize;
mod functions;
mod run;
mod specify;
mod specs_data;
mod stubify;
mod tracked_csv;
mod verify;

pub use atomize::cmd_atomize;
pub use functions::cmd_functions;
pub use run::cmd_run;
pub use specify::cmd_specify;
pub use specs_data::cmd_specs_data;
pub use stubify::cmd_stubify;
pub use tracked_csv::cmd_tracked_csv;
pub use verify::cmd_verify;

// Re-export types needed by main.rs
pub use functions::OutputFormat;
