//! Taxonomy classification for function specifications.
//!
//! Loads classification rules from a TOML config file and evaluates them
//! against structured function metadata (mode, ensures_calls, etc.)
//! to produce spec taxonomy labels.

use crate::verus_parser::FunctionInfo;
use crate::FunctionMode;
use serde::Deserialize;
use std::path::Path;

/// Top-level taxonomy config (wraps the `[taxonomy]` table).
#[derive(Debug, Deserialize)]
pub struct TaxonomyConfig {
    pub taxonomy: TaxonomyRoot,
}

/// The `[taxonomy]` table containing version and rules.
#[derive(Debug, Deserialize)]
pub struct TaxonomyRoot {
    pub version: String,
    pub rules: Vec<TaxonomyRule>,
    /// Stop words: function call names to ignore in ensures_calls/requires_calls.
    /// Common utility calls (len, subrange, old, unwrap, Some, etc.) carry no
    /// classification signal and can be filtered out to simplify rule writing.
    #[serde(default)]
    pub stop_words: Vec<String>,
}

/// A single classification rule.
#[derive(Debug, Deserialize)]
pub struct TaxonomyRule {
    pub label: String,
    pub description: String,
    pub trust: String,
    #[serde(rename = "match")]
    pub match_criteria: MatchCriteria,
}

/// Match criteria for a rule. All specified criteria must match (AND).
/// Within list criteria, any match suffices (OR).
#[derive(Debug, Deserialize, Default)]
pub struct MatchCriteria {
    /// Function mode must be one of these (exec, proof, spec)
    pub mode: Option<Vec<String>>,
    /// Function context must be one of these (impl, trait, standalone)
    pub context: Option<Vec<String>>,
    /// At least one ensures call name must contain one of these substrings
    pub ensures_calls_contain: Option<Vec<String>>,
    /// At least one requires call name must contain one of these substrings
    pub requires_calls_contain: Option<Vec<String>>,
    /// Function name must contain one of these substrings
    pub name_contains: Option<Vec<String>>,
    /// Code path must contain one of these substrings
    pub path_contains: Option<Vec<String>>,
    /// Function must have ensures clause
    pub has_ensures: Option<bool>,
    /// Function must have requires clause
    pub has_requires: Option<bool>,
    /// Function must have decreases clause
    pub has_decreases: Option<bool>,
    /// Function must have trusted assumption (assume/admit)
    pub has_trusted_assumption: Option<bool>,
    /// Whether the ensures clause has no function calls (empty ensures_calls)
    pub ensures_calls_empty: Option<bool>,
    /// Whether the requires clause has no function calls (empty requires_calls)
    pub requires_calls_empty: Option<bool>,
    /// At least one ensures full path must contain one of these substrings
    pub ensures_calls_full_contain: Option<Vec<String>>,
    /// At least one requires full path must contain one of these substrings
    pub requires_calls_full_contain: Option<Vec<String>>,
    /// At least one ensures function call (non-method) must contain one of these substrings
    pub ensures_fn_calls_contain: Option<Vec<String>>,
    /// At least one ensures method call must contain one of these substrings
    pub ensures_method_calls_contain: Option<Vec<String>>,
    /// At least one requires function call (non-method) must contain one of these substrings
    pub requires_fn_calls_contain: Option<Vec<String>>,
    /// At least one requires method call must contain one of these substrings
    pub requires_method_calls_contain: Option<Vec<String>>,
}

/// Load a taxonomy config from a TOML file.
pub fn load_taxonomy_config(path: &Path) -> Result<TaxonomyConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read taxonomy config: {e}"))?;
    toml::from_str(&content).map_err(|e| format!("Failed to parse taxonomy config: {e}"))
}

/// Classify a function against all taxonomy rules.
///
/// Returns a list of labels for all matching rules.
/// Rules are evaluated independently; all matching rules contribute their label.
/// If the config defines `stop_words`, those are filtered from ensures_calls/requires_calls
/// before rule evaluation.
pub fn classify_function(func: &FunctionInfo, config: &TaxonomyConfig) -> Vec<String> {
    // Apply stop-word filtering if configured
    let filtered;
    let effective_func = if config.taxonomy.stop_words.is_empty() {
        func
    } else {
        filtered = filter_stop_words(func, &config.taxonomy.stop_words);
        &filtered
    };

    let mut labels = Vec::new();
    for rule in &config.taxonomy.rules {
        if rule_matches(effective_func, &rule.match_criteria) {
            labels.push(rule.label.clone());
        }
    }
    labels
}

/// Create a copy of FunctionInfo with stop words removed from ensures_calls and requires_calls.
fn filter_stop_words(func: &FunctionInfo, stop_words: &[String]) -> FunctionInfo {
    let mut filtered = func.clone();
    filtered
        .ensures_calls
        .retain(|c| !stop_words.iter().any(|sw| c == sw));
    filtered
        .requires_calls
        .retain(|c| !stop_words.iter().any(|sw| c == sw));
    filtered
}

/// Detailed explanation of why a rule matched or didn't match a function.
#[derive(Debug)]
pub struct RuleExplanation {
    pub label: String,
    pub matched: bool,
    /// For each criterion that was checked, the name and whether it passed.
    pub criteria_results: Vec<(String, bool)>,
}

/// Explain which rules matched and which didn't, and why.
///
/// Returns an explanation for every rule in the config.
pub fn explain_function(func: &FunctionInfo, config: &TaxonomyConfig) -> Vec<RuleExplanation> {
    let filtered;
    let effective_func = if config.taxonomy.stop_words.is_empty() {
        func
    } else {
        filtered = filter_stop_words(func, &config.taxonomy.stop_words);
        &filtered
    };

    config
        .taxonomy
        .rules
        .iter()
        .map(|rule| {
            let results = explain_rule_match(effective_func, &rule.match_criteria);
            let all_passed = results.iter().all(|(_, passed)| *passed);
            RuleExplanation {
                label: rule.label.clone(),
                matched: all_passed,
                criteria_results: results,
            }
        })
        .collect()
}

/// Explain each criterion of a rule match, returning (criterion_name, passed).
fn explain_rule_match(func: &FunctionInfo, criteria: &MatchCriteria) -> Vec<(String, bool)> {
    let mut results = Vec::new();

    if let Some(modes) = &criteria.mode {
        let func_mode = mode_to_string(&func.mode);
        let passed = modes.iter().any(|m| m == func_mode);
        results.push((format!("mode={:?}", modes), passed));
    }

    if let Some(contexts) = &criteria.context {
        let func_ctx = func.context.as_deref().unwrap_or("");
        let passed = contexts.iter().any(|c| c == func_ctx);
        results.push((format!("context={:?}", contexts), passed));
    }

    if let Some(patterns) = &criteria.ensures_calls_contain {
        let passed = func
            .ensures_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((format!("ensures_calls_contain={:?}", patterns), passed));
    }

    if let Some(patterns) = &criteria.requires_calls_contain {
        let passed = func
            .requires_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((format!("requires_calls_contain={:?}", patterns), passed));
    }

    if let Some(patterns) = &criteria.name_contains {
        let passed = patterns.iter().any(|pat| func.name.contains(pat.as_str()));
        results.push((format!("name_contains={:?}", patterns), passed));
    }

    if let Some(patterns) = &criteria.path_contains {
        let path = func.file.as_deref().unwrap_or("");
        let passed = patterns.iter().any(|pat| path.contains(pat.as_str()));
        results.push((format!("path_contains={:?}", patterns), passed));
    }

    if let Some(expected) = criteria.has_ensures {
        let passed = func.has_ensures == expected;
        results.push((format!("has_ensures={}", expected), passed));
    }

    if let Some(expected) = criteria.has_requires {
        let passed = func.has_requires == expected;
        results.push((format!("has_requires={}", expected), passed));
    }

    if let Some(expected) = criteria.has_decreases {
        let passed = func.has_decreases == expected;
        results.push((format!("has_decreases={}", expected), passed));
    }

    if let Some(expected) = criteria.has_trusted_assumption {
        let passed = func.has_trusted_assumption == expected;
        results.push((format!("has_trusted_assumption={}", expected), passed));
    }

    if let Some(expected) = criteria.ensures_calls_empty {
        let passed = func.ensures_calls.is_empty() == expected;
        results.push((format!("ensures_calls_empty={}", expected), passed));
    }

    if let Some(expected) = criteria.requires_calls_empty {
        let passed = func.requires_calls.is_empty() == expected;
        results.push((format!("requires_calls_empty={}", expected), passed));
    }

    if let Some(patterns) = &criteria.ensures_calls_full_contain {
        let passed = func
            .ensures_calls_full
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((format!("ensures_calls_full_contain={:?}", patterns), passed));
    }

    if let Some(patterns) = &criteria.requires_calls_full_contain {
        let passed = func
            .requires_calls_full
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((
            format!("requires_calls_full_contain={:?}", patterns),
            passed,
        ));
    }

    if let Some(patterns) = &criteria.ensures_fn_calls_contain {
        let passed = func
            .ensures_fn_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((format!("ensures_fn_calls_contain={:?}", patterns), passed));
    }

    if let Some(patterns) = &criteria.ensures_method_calls_contain {
        let passed = func
            .ensures_method_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((
            format!("ensures_method_calls_contain={:?}", patterns),
            passed,
        ));
    }

    if let Some(patterns) = &criteria.requires_fn_calls_contain {
        let passed = func
            .requires_fn_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((format!("requires_fn_calls_contain={:?}", patterns), passed));
    }

    if let Some(patterns) = &criteria.requires_method_calls_contain {
        let passed = func
            .requires_method_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())));
        results.push((
            format!("requires_method_calls_contain={:?}", patterns),
            passed,
        ));
    }

    results
}

/// Check if a function matches all specified criteria of a rule.
///
/// All specified criteria must match (AND logic).
/// Within a list criterion, any match suffices (OR logic).
fn rule_matches(func: &FunctionInfo, criteria: &MatchCriteria) -> bool {
    // Mode check
    if let Some(modes) = &criteria.mode {
        let func_mode = mode_to_string(&func.mode);
        if !modes.iter().any(|m| m == func_mode) {
            return false;
        }
    }

    // Context check
    if let Some(contexts) = &criteria.context {
        let func_ctx = func.context.as_deref().unwrap_or("");
        if !contexts.iter().any(|c| c == func_ctx) {
            return false;
        }
    }

    // ensures_calls_contain: ANY call name contains ANY substring
    if let Some(patterns) = &criteria.ensures_calls_contain {
        if !func
            .ensures_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // requires_calls_contain: ANY call name contains ANY substring
    if let Some(patterns) = &criteria.requires_calls_contain {
        if !func
            .requires_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // name_contains: function name contains ANY substring
    if let Some(patterns) = &criteria.name_contains {
        if !patterns.iter().any(|pat| func.name.contains(pat.as_str())) {
            return false;
        }
    }

    // path_contains: code path contains ANY substring
    if let Some(patterns) = &criteria.path_contains {
        let path = func.file.as_deref().unwrap_or("");
        if !patterns.iter().any(|pat| path.contains(pat.as_str())) {
            return false;
        }
    }

    // Boolean flag checks
    if let Some(expected) = criteria.has_ensures {
        if func.has_ensures != expected {
            return false;
        }
    }
    if let Some(expected) = criteria.has_requires {
        if func.has_requires != expected {
            return false;
        }
    }
    if let Some(expected) = criteria.has_decreases {
        if func.has_decreases != expected {
            return false;
        }
    }
    if let Some(expected) = criteria.has_trusted_assumption {
        if func.has_trusted_assumption != expected {
            return false;
        }
    }

    // ensures_calls_empty: whether the ensures clause has no function calls
    if let Some(expected) = criteria.ensures_calls_empty {
        if func.ensures_calls.is_empty() != expected {
            return false;
        }
    }

    // requires_calls_empty: whether the requires clause has no function calls
    if let Some(expected) = criteria.requires_calls_empty {
        if func.requires_calls.is_empty() != expected {
            return false;
        }
    }

    // ensures_calls_full_contain: ANY full path contains ANY substring
    if let Some(patterns) = &criteria.ensures_calls_full_contain {
        if !func
            .ensures_calls_full
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // requires_calls_full_contain: ANY full path contains ANY substring
    if let Some(patterns) = &criteria.requires_calls_full_contain {
        if !func
            .requires_calls_full
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // ensures_fn_calls_contain: ANY non-method call contains ANY substring
    if let Some(patterns) = &criteria.ensures_fn_calls_contain {
        if !func
            .ensures_fn_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // ensures_method_calls_contain: ANY method call contains ANY substring
    if let Some(patterns) = &criteria.ensures_method_calls_contain {
        if !func
            .ensures_method_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // requires_fn_calls_contain: ANY non-method call contains ANY substring
    if let Some(patterns) = &criteria.requires_fn_calls_contain {
        if !func
            .requires_fn_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    // requires_method_calls_contain: ANY method call contains ANY substring
    if let Some(patterns) = &criteria.requires_method_calls_contain {
        if !func
            .requires_method_calls
            .iter()
            .any(|call| patterns.iter().any(|pat| call.contains(pat.as_str())))
        {
            return false;
        }
    }

    true
}

fn mode_to_string(mode: &FunctionMode) -> &'static str {
    match mode {
        FunctionMode::Exec => "exec",
        FunctionMode::Proof => "proof",
        FunctionMode::Spec => "spec",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verus_parser::SpecText;

    fn make_func(mode: FunctionMode, ensures_calls: Vec<&str>) -> FunctionInfo {
        FunctionInfo {
            name: "test_fn".to_string(),
            file: Some("src/test.rs".to_string()),
            spec_text: SpecText {
                lines_start: 1,
                lines_end: 10,
            },
            mode,
            kind: None,
            visibility: None,
            context: Some("standalone".to_string()),
            specified: !ensures_calls.is_empty(),
            has_requires: false,
            has_ensures: !ensures_calls.is_empty(),
            has_decreases: false,
            has_trusted_assumption: false,
            requires_text: None,
            ensures_text: None,
            ensures_calls: ensures_calls.into_iter().map(String::from).collect(),
            requires_calls: Vec::new(),
            ensures_calls_full: Vec::new(),
            requires_calls_full: Vec::new(),
            ensures_fn_calls: Vec::new(),
            ensures_method_calls: Vec::new(),
            requires_fn_calls: Vec::new(),
            requires_method_calls: Vec::new(),
        }
    }

    fn make_config(toml_str: &str) -> TaxonomyConfig {
        toml::from_str(toml_str).expect("Failed to parse test TOML")
    }

    #[test]
    fn test_mode_match() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            [[taxonomy.rules]]
            label = "spec-def"
            description = "Specification definition"
            trust = "n/a"
            [taxonomy.rules.match]
            mode = ["spec"]
        "#,
        );
        let func = make_func(FunctionMode::Spec, vec![]);
        assert_eq!(classify_function(&func, &config), vec!["spec-def"]);

        let exec_func = make_func(FunctionMode::Exec, vec![]);
        assert!(classify_function(&exec_func, &config).is_empty());
    }

    #[test]
    fn test_ensures_calls_contain() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            [[taxonomy.rules]]
            label = "data-invariant"
            description = "Data invariant"
            trust = "high"
            [taxonomy.rules.match]
            ensures_calls_contain = ["is_canonical", "is_valid"]
        "#,
        );
        let func = make_func(
            FunctionMode::Exec,
            vec!["is_canonical_scalar52", "scalar52_to_nat"],
        );
        assert_eq!(classify_function(&func, &config), vec!["data-invariant"]);

        let no_match = make_func(FunctionMode::Exec, vec!["scalar52_to_nat"]);
        assert!(classify_function(&no_match, &config).is_empty());
    }

    #[test]
    fn test_multiple_labels() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            [[taxonomy.rules]]
            label = "data-invariant"
            description = "Data invariant"
            trust = "high"
            [taxonomy.rules.match]
            ensures_calls_contain = ["is_canonical"]
            [[taxonomy.rules]]
            label = "functional-correctness"
            description = "Functional correctness"
            trust = "highest"
            [taxonomy.rules.match]
            ensures_calls_contain = ["_to_nat"]
            mode = ["exec"]
        "#,
        );
        let func = make_func(
            FunctionMode::Exec,
            vec!["is_canonical_scalar52", "scalar52_to_nat"],
        );
        let labels = classify_function(&func, &config);
        assert_eq!(labels, vec!["data-invariant", "functional-correctness"]);
    }

    #[test]
    fn test_ensures_calls_empty() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            [[taxonomy.rules]]
            label = "memory-safety"
            description = "Direct structural/memory assertions"
            trust = "high"
            [taxonomy.rules.match]
            has_ensures = true
            ensures_calls_empty = true
            mode = ["exec"]
        "#,
        );
        // Exec function with ensures but NO function calls in ensures -> should match
        let mut func = make_func(FunctionMode::Exec, vec![]);
        func.has_ensures = true;
        assert_eq!(classify_function(&func, &config), vec!["memory-safety"]);

        // Exec function with ensures AND function calls -> should NOT match
        let func2 = make_func(FunctionMode::Exec, vec!["spec_foo"]);
        assert!(classify_function(&func2, &config).is_empty());

        // Exec function with no ensures at all -> should NOT match
        let func3 = make_func(FunctionMode::Exec, vec![]);
        assert!(classify_function(&func3, &config).is_empty());
    }

    #[test]
    fn test_stop_words() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            stop_words = ["len", "old", "unwrap"]
            [[taxonomy.rules]]
            label = "memory-safety"
            description = "Direct structural assertions"
            trust = "high"
            [taxonomy.rules.match]
            has_ensures = true
            ensures_calls_empty = true
            mode = ["exec"]
        "#,
        );
        // Function with ensures calls that are ALL stop words -> after filtering, empty
        let mut func = make_func(FunctionMode::Exec, vec!["len", "old"]);
        func.has_ensures = true;
        assert_eq!(classify_function(&func, &config), vec!["memory-safety"]);

        // Function with ensures calls that include non-stop-word -> not empty after filtering
        let mut func2 = make_func(FunctionMode::Exec, vec!["len", "recover"]);
        func2.has_ensures = true;
        assert!(classify_function(&func2, &config).is_empty());
    }

    #[test]
    fn test_explain() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            [[taxonomy.rules]]
            label = "fc"
            description = "Functional correctness"
            trust = "highest"
            [taxonomy.rules.match]
            ensures_calls_contain = ["spec_"]
            mode = ["exec"]
        "#,
        );
        let func = make_func(FunctionMode::Exec, vec!["spec_foo"]);
        let explanations = explain_function(&func, &config);
        assert_eq!(explanations.len(), 1);
        assert!(explanations[0].matched);
        assert!(explanations[0].criteria_results.iter().all(|(_, p)| *p));

        let func2 = make_func(FunctionMode::Proof, vec!["spec_foo"]);
        let explanations2 = explain_function(&func2, &config);
        assert!(!explanations2[0].matched);
        // mode criterion should have failed
        let mode_result = explanations2[0]
            .criteria_results
            .iter()
            .find(|(name, _)| name.contains("mode"));
        assert!(mode_result.is_some());
        assert!(!mode_result.unwrap().1);
    }

    #[test]
    fn test_and_logic() {
        let config = make_config(
            r#"
            [taxonomy]
            version = "1"
            [[taxonomy.rules]]
            label = "fc"
            description = "Functional correctness"
            trust = "highest"
            [taxonomy.rules.match]
            ensures_calls_contain = ["spec_"]
            mode = ["exec"]
        "#,
        );
        // proof mode + spec_ call -> should NOT match (mode fails)
        let func = make_func(FunctionMode::Proof, vec!["spec_foo"]);
        assert!(classify_function(&func, &config).is_empty());

        // exec mode + spec_ call -> should match
        let func2 = make_func(FunctionMode::Exec, vec!["spec_foo"]);
        assert_eq!(classify_function(&func2, &config), vec!["fc"]);
    }
}
