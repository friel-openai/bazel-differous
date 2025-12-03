use crate::models::{
    read_dep_edges_file, read_target_hashes, DependencyEdges, ImpactedTargetDistance,
    ImpactedTargetsResult, TargetHash, TargetHashes,
};
use anyhow::{anyhow, bail, Result};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImpactKind {
    Direct,
    Indirect,
}

pub fn get_impacted_targets<P, Q, R>(
    start_path: P,
    final_path: Q,
    dep_edges_path: Option<R>,
    target_types: Option<Vec<String>>,
) -> Result<ImpactedTargetsResult>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
    R: AsRef<Path>,
{
    let start_hashes = read_target_hashes(&start_path)?;
    let final_hashes = read_target_hashes(&final_path)?;
    let target_types_set = target_types.map(|t| t.into_iter().collect::<HashSet<_>>());

    let impacted =
        compute_impacted_targets(&start_hashes, &final_hashes, target_types_set.as_ref())?;

    if let Some(dep_path) = dep_edges_path {
        let deps = read_dep_edges_file(dep_path)?;
        let distances = compute_distances(&start_hashes, &final_hashes, &deps, &impacted)?;
        Ok(ImpactedTargetsResult {
            impacted,
            distances: Some(distances),
        })
    } else {
        Ok(ImpactedTargetsResult {
            impacted,
            distances: None,
        })
    }
}

pub fn compute_impacted_targets(
    start_hashes: &TargetHashes,
    final_hashes: &TargetHashes,
    target_types: Option<&HashSet<String>>,
) -> Result<Vec<String>> {
    let mut impacted = Vec::new();

    let mut all_labels = BTreeSet::new();
    all_labels.extend(start_hashes.keys().cloned());
    all_labels.extend(final_hashes.keys().cloned());

    for label in all_labels.iter() {
        let start_hash = start_hashes.get(label);
        let final_hash = final_hashes.get(label);
        let changed = match (start_hash, final_hash) {
            (Some(start), Some(end)) => start.raw != end.raw,
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        };

        if changed {
            impacted.push(label.clone());
        }
    }

    if let Some(allowed_types) = target_types {
        let mut filtered = Vec::new();
        for label in impacted.into_iter() {
            let hash = target_hash_for_label(&label, start_hashes, final_hashes)?;

            let target_type = hash.target_type().ok_or_else(|| {
                anyhow!(
                    "No target type info for {label}; regenerate hashes with --includeTargetType"
                )
            })?;

            if allowed_types.contains(target_type) {
                filtered.push(label);
            }
        }
        impacted = filtered;
    }

    impacted.sort_by(|a, b| compare_by_type_then_label(a, b, start_hashes, final_hashes));

    impacted.dedup();

    Ok(impacted)
}

fn target_hash_for_label<'a>(
    label: &str,
    start_hashes: &'a TargetHashes,
    final_hashes: &'a TargetHashes,
) -> Result<&'a TargetHash> {
    final_hashes
        .get(label)
        .or_else(|| start_hashes.get(label))
        .ok_or_else(|| anyhow!("missing hash entry for {label}"))
}

fn target_type_for_label<'a>(
    label: &str,
    start_hashes: &'a TargetHashes,
    final_hashes: &'a TargetHashes,
) -> Option<&'a str> {
    final_hashes
        .get(label)
        .and_then(|hash| hash.target_type())
        .or_else(|| start_hashes.get(label).and_then(|hash| hash.target_type()))
}

fn compare_by_type_then_label(
    left: &str,
    right: &str,
    start_hashes: &TargetHashes,
    final_hashes: &TargetHashes,
) -> Ordering {
    let left_type = target_type_for_label(left, start_hashes, final_hashes);
    let right_type = target_type_for_label(right, start_hashes, final_hashes);

    let rank = |value: Option<&str>| match value {
        Some("SourceFile") => 0,
        Some("GeneratedFile") => 1,
        Some("Rule") => 2,
        Some(_) => 3,
        None => 4,
    };

    rank(left_type)
        .cmp(&rank(right_type))
        .then_with(|| left.cmp(right))
}

fn compute_distances(
    start_hashes: &TargetHashes,
    final_hashes: &TargetHashes,
    dep_edges: &DependencyEdges,
    impacted: &[String],
) -> Result<Vec<ImpactedTargetDistance>> {
    let mut kind_by_label: BTreeMap<String, ImpactKind> = BTreeMap::new();

    for label in impacted {
        let start_hash = start_hashes.get(label);
        let final_hash = final_hashes.get(label);

        let kind = classify_impact(start_hash, final_hash);
        kind_by_label.insert(label.clone(), kind);
    }

    let mut memo: HashMap<String, ImpactedTargetDistance> = HashMap::new();
    let mut visiting = HashSet::new();
    let mut results = Vec::with_capacity(impacted.len());

    for label in impacted {
        let distance =
            calculate_distance(label, dep_edges, &kind_by_label, &mut memo, &mut visiting)?;
        results.push(distance);
    }

    Ok(results)
}

fn classify_impact(start_hash: Option<&TargetHash>, final_hash: Option<&TargetHash>) -> ImpactKind {
    match (start_hash, final_hash) {
        (None, _) | (_, None) => ImpactKind::Direct,
        (Some(start), Some(end)) => match (&start.direct_hash, &end.direct_hash) {
            (Some(start_direct), Some(end_direct)) if start_direct == end_direct => {
                ImpactKind::Indirect
            }
            _ => ImpactKind::Direct,
        },
    }
}

fn calculate_distance(
    label: &str,
    dep_edges: &DependencyEdges,
    impacted_kinds: &BTreeMap<String, ImpactKind>,
    memo: &mut HashMap<String, ImpactedTargetDistance>,
    visiting: &mut HashSet<String>,
) -> Result<ImpactedTargetDistance> {
    if let Some(cached) = memo.get(label) {
        return Ok(cached.clone());
    }

    if !visiting.insert(label.to_string()) {
        bail!("cycle detected while computing distance for {label}");
    }

    let result = match impacted_kinds.get(label) {
        Some(ImpactKind::Direct) => ImpactedTargetDistance {
            label: label.to_string(),
            target_distance: 0,
            package_distance: 0,
        },
        Some(ImpactKind::Indirect) => {
            let deps = dep_edges.get(label).ok_or_else(|| {
                anyhow!("{label} was indirectly impacted but has no dependencies in dep graph")
            })?;

            let mut distances = Vec::new();
            for dep in deps {
                if !impacted_kinds.contains_key(dep) {
                    continue;
                }

                let dep_distance =
                    calculate_distance(dep, dep_edges, impacted_kinds, memo, visiting)?;
                let crosses_package = package_segment(label) != package_segment(dep);
                distances.push((
                    dep_distance.target_distance + 1,
                    dep_distance.package_distance + if crosses_package { 1 } else { 0 },
                ));
            }

            if distances.is_empty() {
                bail!("{label} was indirectly impacted but has no impacted dependencies");
            }

            let target_distance = distances.iter().map(|(t, _)| *t).min().unwrap_or(0);
            let package_distance = distances.iter().map(|(_, p)| *p).min().unwrap_or(0);

            ImpactedTargetDistance {
                label: label.to_string(),
                target_distance,
                package_distance,
            }
        }
        None => bail!("{label} was not marked as impacted"),
    };

    visiting.remove(label);
    memo.insert(label.to_string(), result.clone());
    Ok(result)
}

fn package_segment(label: &str) -> &str {
    label.split(':').next().unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TargetHash;

    fn hash(value: &str) -> TargetHash {
        TargetHash::parse(value).unwrap()
    }

    #[test]
    fn impacted_targets_include_added_changed_removed() {
        let start = BTreeMap::from([("//pkg:a".into(), hash("h1"))]);
        let final_map = BTreeMap::from([
            ("//pkg:a".into(), hash("h2")),
            ("//pkg:b".into(), hash("h3")),
        ]);

        let impacted = compute_impacted_targets(&start, &final_map, None).unwrap();
        assert_eq!(impacted, vec!["//pkg:a", "//pkg:b"]);
    }

    #[test]
    fn target_type_filter_requires_type_info() {
        let start = BTreeMap::from([("//pkg:a".into(), hash("h1"))]);
        let final_map = BTreeMap::from([("//pkg:a".into(), hash("h2"))]);
        let res = compute_impacted_targets(
            &start,
            &final_map,
            Some(&HashSet::from(["Rule".to_string()])),
        );
        assert!(res.is_err());
    }

    #[test]
    fn computes_distances_for_indirect_changes() {
        let start = BTreeMap::from([
            ("//pkg:a".into(), hash("Rule#old_a~d1")),
            ("//pkg:b".into(), hash("Rule#b~d2")),
        ]);
        let final_map = BTreeMap::from([
            ("//pkg:a".into(), hash("Rule#new_a~d1")),
            ("//pkg:b".into(), hash("Rule#new_b~d3")),
        ]);

        let impacted = compute_impacted_targets(&start, &final_map, None).unwrap();

        let deps = BTreeMap::from([
            ("//pkg:a".into(), vec!["//pkg:b".into()]),
            ("//pkg:b".into(), Vec::new()),
        ]);

        let distances = compute_distances(&start, &final_map, &deps, &impacted).unwrap();

        let mut sorted = distances;
        sorted.sort_by(|a, b| a.label.cmp(&b.label));

        let a_metrics = sorted.iter().find(|d| d.label == "//pkg:a").unwrap();
        assert_eq!(a_metrics.target_distance, 1);
        assert_eq!(a_metrics.package_distance, 0);

        let b_metrics = sorted.iter().find(|d| d.label == "//pkg:b").unwrap();
        assert_eq!(b_metrics.target_distance, 0);
    }
}
