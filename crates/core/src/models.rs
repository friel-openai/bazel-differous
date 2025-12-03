use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{collections::BTreeMap, fs::File, io::BufReader, path::Path};

pub type TargetHashes = BTreeMap<String, TargetHash>;
pub type DependencyEdges = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetHash {
    pub raw: String,
    pub transitive_hash: String,
    pub direct_hash: Option<String>,
    pub target_type: Option<String>,
}

impl TargetHash {
    pub fn parse(raw: &str) -> Result<Self> {
        let (target_type, remainder) = match raw.split_once('#') {
            Some((kind, rest)) => (Some(kind.to_string()), rest.to_string()),
            None => (None, raw.to_string()),
        };

        let (transitive_hash, direct_hash) = match remainder.split_once('~') {
            Some((transitive, direct)) => (transitive.to_string(), Some(direct.to_string())),
            None => (remainder.clone(), None),
        };

        if transitive_hash.is_empty() {
            bail!("target hash string cannot be empty");
        }

        Ok(Self {
            raw: raw.to_string(),
            transitive_hash,
            direct_hash,
            target_type,
        })
    }

    pub fn target_type(&self) -> Option<&str> {
        self.target_type.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImpactedTargetDistance {
    pub label: String,
    #[serde(rename = "targetDistance")]
    pub target_distance: usize,
    #[serde(rename = "packageDistance")]
    pub package_distance: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactedTargetsResult {
    pub impacted: Vec<String>,
    pub distances: Option<Vec<ImpactedTargetDistance>>,
}

pub fn read_target_hashes<P: AsRef<Path>>(path: P) -> Result<TargetHashes> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref)
        .with_context(|| format!("failed to open hashes file {}", path_ref.display()))?;
    let reader = BufReader::new(file);
    let raw_map: BTreeMap<String, String> = serde_json::from_reader(reader)
        .with_context(|| format!("failed to parse JSON hashes from {}", path_ref.display()))?;

    raw_map
        .into_iter()
        .map(|(label, raw_hash)| {
            let parsed = TargetHash::parse(&raw_hash)
                .with_context(|| format!("invalid hash for {label}"))?;
            Ok((label, parsed))
        })
        .collect()
}

pub fn read_dep_edges_file<P: AsRef<Path>>(path: P) -> Result<DependencyEdges> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref)
        .with_context(|| format!("failed to open dep edges file {}", path_ref.display()))?;
    let reader = BufReader::new(file);
    serde_json::from_reader(reader)
        .with_context(|| format!("failed to parse dep edges JSON from {}", path_ref.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hash_with_type_and_direct() {
        let parsed = TargetHash::parse("Rule#abc~def").unwrap();
        assert_eq!(parsed.target_type(), Some("Rule"));
        assert_eq!(parsed.transitive_hash, "abc");
        assert_eq!(parsed.direct_hash.as_deref(), Some("def"));
    }

    #[test]
    fn parses_hash_without_type() {
        let parsed = TargetHash::parse("abc123").unwrap();
        assert_eq!(parsed.target_type(), None);
        assert_eq!(parsed.transitive_hash, "abc123");
        assert_eq!(parsed.direct_hash, None);
    }
}
