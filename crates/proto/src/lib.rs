#![allow(dead_code)]
#![allow(clippy::all)]

pub mod blaze_query {
    include!(concat!(env!("OUT_DIR"), "/blaze_query.rs"));
}

// Backwards-compatible alias for callers expecting `build::...`.
pub use blaze_query as build;

pub mod analysis {
    include!(concat!(env!("OUT_DIR"), "/analysis.rs"));
}

pub mod stardoc_output {
    include!(concat!(env!("OUT_DIR"), "/stardoc_output.rs"));
}

/// Returns a short label to make it obvious the crate linked correctly.
pub fn status() -> &'static str {
    "bazel-differrous-proto ready"
}
