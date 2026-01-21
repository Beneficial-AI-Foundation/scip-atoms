//! Shared constants for probe-verus.
//!
//! This module centralizes magic numbers and configuration values
//! to improve readability and maintainability.

// =============================================================================
// SCIP Symbol Kinds
// =============================================================================
// These correspond to the `kind` field in SCIP Symbol entries.
// See: https://sourcegraph.com/docs/code-search/code-navigation/writing_an_indexer#symbolinformation

/// SCIP kind for method definitions (instance methods)
pub const SCIP_KIND_METHOD: i32 = 6;

/// SCIP kind for function definitions
pub const SCIP_KIND_FUNCTION: i32 = 17;

/// SCIP kind for constructor definitions
pub const SCIP_KIND_CONSTRUCTOR: i32 = 26;

/// SCIP kind for macro definitions (used by verus-analyzer for some functions)
pub const SCIP_KIND_MACRO: i32 = 80;

/// Check if a SCIP symbol kind represents a function-like entity.
///
/// This includes regular functions, methods, constructors, and some macros
/// that verus-analyzer uses to represent certain function types.
#[inline]
pub fn is_function_like_kind(kind: i32) -> bool {
    matches!(
        kind,
        SCIP_KIND_METHOD | SCIP_KIND_FUNCTION | SCIP_KIND_CONSTRUCTOR | SCIP_KIND_MACRO
    )
}

// =============================================================================
// SCIP Symbol Roles
// =============================================================================
// Bitflags for the `symbol_roles` field in SCIP Occurrence entries.

/// Symbol role bit indicating this occurrence is a definition
pub const SYMBOL_ROLE_DEFINITION: i32 = 1;

/// Check if a symbol_roles value indicates a definition.
#[inline]
pub fn is_definition(symbol_roles: Option<i32>) -> bool {
    symbol_roles.unwrap_or(0) & SYMBOL_ROLE_DEFINITION != 0
}

// =============================================================================
// Matching Tolerances
// =============================================================================

/// Line number tolerance for matching functions between different tools.
///
/// verus-analyzer and verus_syn may report slightly different start lines
/// due to differences in how they handle attributes and doc comments.
/// This tolerance allows fuzzy matching within a reasonable range.
pub const LINE_TOLERANCE: usize = 5;

/// Number of lines to look back from a definition for type context.
///
/// Used when collecting nearby type references to help disambiguate
/// trait implementations (e.g., `impl From<T> for Container<X>` vs `Container<Y>`).
pub const TYPE_CONTEXT_LOOKBACK_LINES: i32 = 5;

// =============================================================================
// Cache Configuration
// =============================================================================

/// Directory name for cached data within a project
pub const DATA_DIR: &str = "data";

/// Filename for cached verification output
pub const VERIFICATION_OUTPUT_FILE: &str = "verification_output.txt";

/// Filename for cached verification configuration
pub const VERIFICATION_CONFIG_FILE: &str = "verification_config.json";

/// Filename for the SCIP index binary
pub const SCIP_INDEX_FILE: &str = "index.scip";

/// Filename for the SCIP index JSON
pub const SCIP_INDEX_JSON_FILE: &str = "index.scip.json";

// =============================================================================
// Default Output Filenames
// =============================================================================

/// Default output filename for atomize command
pub const DEFAULT_ATOMS_OUTPUT: &str = "atoms.json";

/// Default output filename for verify command
pub const DEFAULT_RESULTS_OUTPUT: &str = "proofs.json";

/// Default output filename for specify command
pub const DEFAULT_SPECS_OUTPUT: &str = "specs.json";

/// Default output filename for stubify command
pub const DEFAULT_STUBS_OUTPUT: &str = "stubs.json";

/// Default output directory for run command
pub const DEFAULT_OUTPUT_DIR: &str = "./output";

// =============================================================================
// SCIP Symbol Prefixes
// =============================================================================

/// Expected prefix for SCIP symbols from rust-analyzer/verus-analyzer
pub const SCIP_SYMBOL_PREFIX: &str = "rust-analyzer cargo ";

/// Prefix for probe-style URIs
pub const PROBE_URI_PREFIX: &str = "probe:";
