//! AST parsing and symbol extraction module using Tree-Sitter & Polyglot Engine.

pub mod database;
pub mod dataflow_def;
pub mod dataflow_walker;
pub mod engine;
pub mod frontend;
pub mod manifests;
pub mod metrics;
mod metrics_heuristic;
mod metrics_walker;
pub mod polyglot;
pub mod test_safety;
mod test_safety_checks;
pub mod trait_def;
pub mod trait_walker;
pub mod types;
pub mod walker;
pub mod zoom;

pub use dataflow_walker::extract_dataflow;
pub use engine::AstEngine;
pub use metrics::analyze_file_path;
pub use test_safety::{analyze_safety_path, analyze_verification_command};
pub use trait_walker::extract_trait_bindings;
pub use types::AstLanguage;
pub use zoom::ZoomSliceResult;

#[cfg(test)]
mod metrics_tests;
#[cfg(test)]
mod test_safety_tests;
#[cfg(test)]
mod tests;
