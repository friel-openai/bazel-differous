pub mod bazel;
pub mod hash;
pub mod impact;
pub mod models;

pub use hash::{generate_hashes, GenerateHashesConfig, GenerateHashesResult};
pub use impact::{compute_impacted_targets, get_impacted_targets};
pub use models::{
    read_dep_edges_file, read_target_hashes, DependencyEdges, ImpactedTargetDistance,
    ImpactedTargetsResult, TargetHash, TargetHashes,
};

/// Returns the current crate version; helpful for tracing and diagnostics.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
