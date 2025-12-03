use crate::bazel::{
    bazel_output_base, build_query_expression, run_cquery, run_query, BazelOptions,
};
use anyhow::{anyhow, bail, Context, Result};
use bazel_differrous_proto::build::{Attribute, Rule, Target};
use hex::encode as hex_encode;
use prost::Message;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

const DEFAULT_IGNORED_ATTRS: &[&str] = &["generator_location"];

#[derive(Debug, Clone)]
pub struct GenerateHashesConfig {
    pub workspace: PathBuf,
    pub include_target_type: bool,
    pub use_cquery: bool,
    pub keep_going: bool,
    pub bazel_path: PathBuf,
    pub startup_options: Vec<String>,
    pub command_options: Vec<String>,
    pub cquery_options: Vec<String>,
    pub exclude_external_targets: bool,
    pub ignored_attrs: Vec<String>,
    pub fine_grained_external_repos: Vec<String>,
    pub fine_grained_external_repos_file: Option<PathBuf>,
    pub content_hash_path: Option<PathBuf>,
    pub seed_filepaths: Option<PathBuf>,
    pub modified_filepaths: Option<PathBuf>,
    pub target_types: Option<Vec<String>>,
    pub track_dep_edges: bool,
}

impl Default for GenerateHashesConfig {
    fn default() -> Self {
        Self {
            workspace: PathBuf::new(),
            include_target_type: false,
            use_cquery: false,
            keep_going: true,
            bazel_path: PathBuf::new(),
            startup_options: Vec::new(),
            command_options: Vec::new(),
            cquery_options: Vec::new(),
            exclude_external_targets: false,
            ignored_attrs: Vec::new(),
            fine_grained_external_repos: Vec::new(),
            fine_grained_external_repos_file: None,
            content_hash_path: None,
            seed_filepaths: None,
            modified_filepaths: None,
            target_types: None,
            track_dep_edges: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateHashesResult {
    pub hashes: BTreeMap<String, String>,
    pub dep_edges: BTreeMap<String, Option<Vec<String>>>,
}

pub async fn generate_hashes(config: &GenerateHashesConfig) -> Result<GenerateHashesResult> {
    let fine_grained_raw = load_fine_grained_repos(
        &config.fine_grained_external_repos,
        config.fine_grained_external_repos_file.as_deref(),
    )?;
    let fine_grained_trimmed: HashSet<String> =
        fine_grained_raw.iter().map(|r| trim_repo_name(r)).collect();

    let target_type_filter = config.target_types.as_ref().map(|list| {
        list.iter()
            .map(|s| s.to_string())
            .collect::<HashSet<String>>()
    });

    let content_hashes = load_content_hash_map(
        config
            .content_hash_path
            .as_ref()
            .map(|p| config.workspace.join(p)),
    )?;
    let seed_hash = compute_seed_hash(
        config
            .seed_filepaths
            .as_ref()
            .map(|p| config.workspace.join(p)),
    )?;
    let modified_paths = load_path_list(
        config
            .modified_filepaths
            .as_ref()
            .map(|p| config.workspace.join(p)),
    )?;

    let ignored_attrs: HashSet<String> = config
        .ignored_attrs
        .iter()
        .map(|s| s.to_string())
        .chain(DEFAULT_IGNORED_ATTRS.iter().map(|s| s.to_string()))
        .collect();

    let bazel_opts = BazelOptions {
        workspace: config.workspace.clone(),
        bazel_path: config.bazel_path.clone(),
        startup_options: config.startup_options.clone(),
        command_options: config.command_options.clone(),
        cquery_options: config.cquery_options.clone(),
        use_cquery: config.use_cquery,
        keep_going: config.keep_going,
    };

    // Output base is needed to locate external repository roots.
    let output_base = bazel_output_base(&bazel_opts).await?;

    let resolver = ExternalRepoResolver {
        workspace: config.workspace.clone(),
        bazel_path: bazel_opts.bazel_path.clone(),
        startup_options: bazel_opts.startup_options.clone(),
        output_base,
    };

    let graph = BazelGraph::load(
        &bazel_opts,
        &fine_grained_raw,
        config.exclude_external_targets,
    )
    .await?;

    let mut engine = HashEngine::new(HashEngineConfig {
        include_target_type: config.include_target_type,
        target_types: target_type_filter,
        ignored_attrs,
        fine_grained_external_repos: fine_grained_trimmed,
        seed_hash,
        content_hashes,
        modified_filepaths: modified_paths,
        track_dep_edges: config.track_dep_edges,
        resolver,
    });

    let results = engine.compute(graph)?;
    Ok(results)
}

fn load_fine_grained_repos(cli_values: &[String], file: Option<&Path>) -> Result<HashSet<String>> {
    if let Some(path) = file {
        if !cli_values.is_empty() {
            bail!("fineGrainedHashExternalReposFile and fineGrainedHashExternalRepos are mutually exclusive");
        }
        let f = File::open(path)
            .with_context(|| format!("failed to open fine-grained repo file {}", path.display()))?;
        let reader = BufReader::new(f);
        Ok(reader
            .lines()
            .map_while(Result::ok)
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    } else {
        Ok(cli_values.iter().map(|s| s.to_string()).collect())
    }
}

fn load_content_hash_map(path: Option<PathBuf>) -> Result<Option<HashMap<String, String>>> {
    match path {
        None => Ok(None),
        Some(p) => {
            let file = File::open(&p)
                .with_context(|| format!("failed to open content hash file {}", p.display()))?;
            let reader = BufReader::new(file);
            let map: HashMap<String, String> =
                serde_json::from_reader(reader).context("failed to parse content hash JSON")?;
            Ok(Some(map))
        }
    }
}

fn compute_seed_hash(path: Option<PathBuf>) -> Result<Vec<u8>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };

    let file = File::open(&path)
        .with_context(|| format!("failed to open seed file list {}", path.display()))?;
    let mut hasher = Sha256::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        let entry = PathBuf::from(line);
        let data = std::fs::read(&entry).with_context(|| {
            format!(
                "failed to read seed file {} referenced by {}",
                entry.display(),
                path.display()
            )
        })?;
        hasher.update(data);
    }
    Ok(hasher.finalize().to_vec())
}

fn load_path_list(path: Option<PathBuf>) -> Result<HashSet<PathBuf>> {
    let Some(path) = path else {
        return Ok(HashSet::new());
    };

    let file = File::open(&path)
        .with_context(|| format!("failed to open path list {}", path.display()))?;
    let mut set = HashSet::new();
    for line in BufReader::new(file).lines() {
        let value = line?;
        if value.trim().is_empty() {
            continue;
        }
        set.insert(PathBuf::from(value.trim()));
    }
    Ok(set)
}

#[derive(Debug)]
struct HashEngineConfig {
    include_target_type: bool,
    target_types: Option<HashSet<String>>,
    ignored_attrs: HashSet<String>,
    fine_grained_external_repos: HashSet<String>,
    seed_hash: Vec<u8>,
    content_hashes: Option<HashMap<String, String>>,
    modified_filepaths: HashSet<PathBuf>,
    track_dep_edges: bool,
    resolver: ExternalRepoResolver,
}

struct HashEngine {
    config: HashEngineConfig,
    source_hasher: SourceFileHasher,
}

impl HashEngine {
    fn new(config: HashEngineConfig) -> Self {
        let source_hasher = SourceFileHasher::new(
            config.resolver.clone(),
            config.content_hashes.clone(),
            config
                .fine_grained_external_repos
                .iter()
                .cloned()
                .collect::<HashSet<_>>(),
            config.modified_filepaths.clone(),
        );

        Self {
            config,
            source_hasher,
        }
    }

    fn compute(&mut self, graph: BazelGraph) -> Result<GenerateHashesResult> {
        let mut source_digests: HashMap<String, Vec<u8>> = HashMap::new();
        for source in &graph.sources {
            let seed = seed_for_source(source);
            let digest = self
                .source_hasher
                .digest(&source.name, &seed)
                .with_context(|| format!("failed to hash source {}", source.name))?;
            debug!(
                source = %source.name,
                seed = %hex_encode(&seed),
                digest = %hex_encode(&digest),
                "source digest"
            );
            source_digests.insert(source.name.clone(), digest);
        }

        let mut rule_digests: HashMap<String, TargetDigest> = HashMap::new();
        let mut results: BTreeMap<String, TargetHashValue> = BTreeMap::new();

        {
            let mut rule_hasher = RuleHasher {
                use_cquery: graph.use_cquery,
                fine_grained_external_repos: self.config.fine_grained_external_repos.clone(),
                ignored_attrs: self.config.ignored_attrs.clone(),
                source_hasher: self.source_hasher.clone(),
                source_digests: &mut source_digests,
                rule_digests: &mut rule_digests,
                seed_hash: self.config.seed_hash.clone(),
                track_dep_edges: self.config.track_dep_edges,
            };

            for target in graph.targets.iter() {
                match target {
                    BazelTarget::Rule(rule) => {
                        let digest = rule_hasher.digest(rule, &graph.rule_map, &mut Vec::new())?;
                        let value = TargetHashValue::new(TargetKind::Rule, digest);
                        results.insert(rule.name.clone(), value);
                    }
                    BazelTarget::Generated(gen) => {
                        let digest =
                            rule_hasher.digest_generated(gen, &graph.rule_map, &mut Vec::new())?;
                        let value = TargetHashValue::new(TargetKind::GeneratedFile, digest);
                        results.insert(gen.name.clone(), value);
                    }
                    BazelTarget::Source(_) => {}
                }
            }
        }

        for source in &graph.sources {
            let digest = target_digest_from_source(
                source_digests
                    .get(&source.name)
                    .ok_or_else(|| anyhow!("missing source digest for {}", source.name))?,
                &self.config.seed_hash,
            );
            let value = TargetHashValue::new(TargetKind::SourceFile, digest);
            results.insert(source.name.clone(), value);
        }

        // Apply target type filtering, if requested.
        if let Some(filter) = &self.config.target_types {
            results.retain(|_, v| filter.contains(v.kind.as_str()));
        }

        let mut hashes = BTreeMap::new();
        let mut dep_edges = BTreeMap::new();
        for (label, value) in results {
            hashes.insert(label.clone(), value.render(self.config.include_target_type));
            if let Some(deps) = value.deps {
                dep_edges.insert(label, Some(deps));
            }
        }

        Ok(GenerateHashesResult { hashes, dep_edges })
    }
}

fn seed_for_source(source: &BazelSource) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(source.name.as_bytes());
    for sub in &source.subincludes {
        hasher.update(sub.as_bytes());
    }
    hasher.finalize().to_vec()
}

fn target_digest_from_source(source_digest: &[u8], seed_hash: &[u8]) -> TargetDigest {
    let mut hasher = Sha256::new();
    hasher.update(source_digest);
    hasher.update(seed_hash);
    let digest = hasher.finalize().to_vec();
    TargetDigest {
        overall: digest.clone(),
        direct: digest,
        deps: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    Rule,
    GeneratedFile,
    SourceFile,
}

impl TargetKind {
    fn as_str(&self) -> &'static str {
        match self {
            TargetKind::Rule => "Rule",
            TargetKind::GeneratedFile => "GeneratedFile",
            TargetKind::SourceFile => "SourceFile",
        }
    }
}

#[derive(Debug, Clone)]
struct TargetHashValue {
    kind: TargetKind,
    overall: Vec<u8>,
    direct: Vec<u8>,
    deps: Option<Vec<String>>,
}

impl TargetHashValue {
    fn new(kind: TargetKind, digest: TargetDigest) -> Self {
        Self {
            kind,
            overall: digest.overall,
            direct: digest.direct,
            deps: digest.deps,
        }
    }

    fn render(&self, include_kind: bool) -> String {
        let total = format!("{}~{}", hex_encode(&self.overall), hex_encode(&self.direct));
        if include_kind {
            format!("{}#{}", self.kind.as_str(), total)
        } else {
            total
        }
    }
}

#[derive(Debug, Clone)]
struct TargetDigest {
    overall: Vec<u8>,
    direct: Vec<u8>,
    deps: Option<Vec<String>>,
}

impl TargetDigest {
    fn clone_with_deps(&self, deps: Option<Vec<String>>) -> Self {
        Self {
            overall: self.overall.clone(),
            direct: self.direct.clone(),
            deps,
        }
    }
}

struct DigestBuilder {
    direct: Sha256,
    overall: Sha256,
    deps: Option<Vec<String>>,
}

impl DigestBuilder {
    fn new(track_deps: bool) -> Self {
        Self {
            direct: Sha256::new(),
            overall: Sha256::new(),
            deps: track_deps.then(Vec::new),
        }
    }

    fn put_direct(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.direct.update(bytes);
        }
    }

    fn put_transitive(&mut self, label: &str, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.overall.update(bytes);
        }
        if let Some(deps) = &mut self.deps {
            deps.push(label.to_string());
        }
    }

    fn finish(mut self) -> TargetDigest {
        let direct_bytes = self.direct.finalize().to_vec();
        self.overall.update(&direct_bytes);
        let overall_bytes = self.overall.finalize().to_vec();
        TargetDigest {
            overall: overall_bytes,
            direct: direct_bytes,
            deps: self.deps,
        }
    }
}

#[derive(Debug)]
struct BazelGraph {
    targets: Vec<BazelTarget>,
    rule_map: HashMap<String, BazelRule>,
    sources: Vec<BazelSource>,
    use_cquery: bool,
}

impl BazelGraph {
    async fn load(
        opts: &BazelOptions,
        fine_grained_repos: &HashSet<String>,
        exclude_external: bool,
    ) -> Result<Self> {
        let mut collected: HashMap<String, Target> = HashMap::new();
        if opts.use_cquery {
            let main_targets = run_cquery(opts, "deps(//...:all-targets)").await?;
            for t in main_targets {
                if let Some(label) = target_label(&t) {
                    collected.entry(label.to_string()).or_insert(t);
                }
            }
            if !exclude_external {
                let external = run_query(opts, "'//external:all-targets'").await?;
                for t in external {
                    if let Some(label) = target_label(&t) {
                        collected.entry(label.to_string()).or_insert(t);
                    }
                }
            }
        } else {
            let mut patterns = vec!["//...:all-targets".to_string()];
            if !exclude_external {
                patterns.push("//external:all-targets".to_string());
            }
            for repo in fine_grained_repos {
                patterns.push(format!("{repo}//...:all-targets"));
            }
            let expr = build_query_expression(&patterns);
            let targets = run_query(opts, &expr).await?;
            for t in targets {
                if let Some(label) = target_label(&t) {
                    collected.entry(label.to_string()).or_insert(t);
                }
            }
        }

        if exclude_external {
            collected.retain(|label, _| !label.starts_with('@'));
        }

        let mut targets = Vec::new();
        let mut rule_map = HashMap::new();
        let mut sources = Vec::new();
        for target in collected.into_values() {
            if let Some(wrapped) = BazelTarget::from_proto(target.clone()) {
                match &wrapped {
                    BazelTarget::Rule(rule) => {
                        rule_map.insert(rule.name.clone(), rule.clone());
                    }
                    BazelTarget::Source(source) => sources.push(source.clone()),
                    BazelTarget::Generated(_) => {}
                }
                targets.push(wrapped);
            }
        }

        Ok(Self {
            targets,
            rule_map,
            sources,
            use_cquery: opts.use_cquery,
        })
    }
}

#[derive(Debug, Clone)]
struct BazelRule {
    name: String,
    rule_class: String,
    skylark_environment_hash_code: Option<String>,
    attributes: Vec<Attribute>,
    rule_inputs: Vec<String>,
    configured_rule_inputs: Vec<String>,
}

impl BazelRule {
    fn from_proto(rule: &Rule) -> Self {
        Self {
            name: rule.name.clone(),
            rule_class: rule.rule_class.clone(),
            skylark_environment_hash_code: rule.skylark_environment_hash_code.clone(),
            attributes: rule.attribute.clone(),
            rule_inputs: rule.rule_input.clone(),
            configured_rule_inputs: rule
                .configured_rule_input
                .iter()
                .filter_map(|c| c.label.clone())
                .collect(),
        }
    }

    fn digest(&self, ignored_attrs: &HashSet<String>) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(self.rule_class.as_bytes());
        hasher.update(self.name.as_bytes());
        if let Some(env) = &self.skylark_environment_hash_code {
            hasher.update(env.as_bytes());
        }
        if self.name.contains("dep_lib") {
            let attr_names: Vec<_> = self.attributes.iter().map(|a| a.name.clone()).collect();
            debug!(rule = %self.name, attrs = ?attr_names, "attributes for rule");
        }
        for attr in &self.attributes {
            if ignored_attrs.contains(&attr.name) {
                continue;
            }
            let mut buf = Vec::new();
            attr.encode(&mut buf).unwrap_or_default();
            hasher.update(&buf);
        }
        hasher.finalize().to_vec()
    }

    fn rule_inputs(&self, use_cquery: bool, fine_grained_repos: &HashSet<String>) -> Vec<String> {
        if use_cquery {
            let mut seen = HashSet::new();
            let mut combined = Vec::new();
            for ri in &self.configured_rule_inputs {
                if seen.insert(ri.clone()) {
                    combined.push(ri.clone());
                }
            }
            for ri in self
                .rule_inputs
                .iter()
                .map(|ri| transform_rule_input(ri, fine_grained_repos))
            {
                if seen.insert(ri.clone()) {
                    combined.push(ri);
                }
            }
            combined
        } else {
            self.rule_inputs.clone()
        }
    }
}

#[derive(Debug, Clone)]
struct BazelSource {
    name: String,
    subincludes: Vec<String>,
}

#[derive(Debug, Clone)]
struct BazelGenerated {
    name: String,
    generating_rule: String,
}

#[derive(Debug, Clone)]
enum BazelTarget {
    Rule(BazelRule),
    Source(BazelSource),
    Generated(BazelGenerated),
}

impl BazelTarget {
    fn from_proto(target: Target) -> Option<Self> {
        if let Some(rule) = target.rule {
            return Some(BazelTarget::Rule(BazelRule::from_proto(&rule)));
        }
        if let Some(source) = target.source_file {
            return Some(BazelTarget::Source(BazelSource {
                name: source.name,
                subincludes: source.subinclude,
            }));
        }
        if let Some(gen) = target.generated_file {
            return Some(BazelTarget::Generated(BazelGenerated {
                name: gen.name,
                generating_rule: gen.generating_rule,
            }));
        }
        warn!("Skipping unsupported target");
        None
    }
}

#[derive(Clone)]
struct SourceFileHasher {
    resolver: ExternalRepoResolver,
    content_hashes: Option<HashMap<String, String>>,
    fine_grained_external_repos: HashSet<String>,
    modified_filepaths: HashSet<PathBuf>,
}

impl SourceFileHasher {
    fn new(
        resolver: ExternalRepoResolver,
        content_hashes: Option<HashMap<String, String>>,
        fine_grained_external_repos: HashSet<String>,
        modified_filepaths: HashSet<PathBuf>,
    ) -> Self {
        Self {
            resolver,
            content_hashes,
            fine_grained_external_repos,
            modified_filepaths,
        }
    }

    fn digest(&self, label: &str, seed: &[u8]) -> Result<Vec<u8>> {
        let mut hasher = Sha256::new();
        if let Some((repo, _)) = split_external_label(label) {
            if trim_repo_name(repo).ends_with('+') {
                return Ok(hasher.finalize().to_vec());
            }
        }
        let Some(path) = self.resolve_label(label)? else {
            return Ok(hasher.finalize().to_vec());
        };

        let relative_key = path.workspace_relative.clone();
        if let Some(map) = &self.content_hashes {
            if let Some(content_hash) = map.get(&relative_key) {
                hasher.update(content_hash.as_bytes());
                hasher.update([0x01]);
                hasher.update(seed);
                hasher.update(label.as_bytes());
                return Ok(hasher.finalize().to_vec());
            }
        }

        if path.absolute.exists() {
            if path.absolute.is_file() {
                if self.modified_filepaths.is_empty()
                    || self
                        .modified_filepaths
                        .iter()
                        .any(|p| self.resolver.workspace.join(p) == path.absolute)
                {
                    let data = std::fs::read(&path.absolute).with_context(|| {
                        format!("failed to read file {}", path.absolute.display())
                    })?;
                    hasher.update(&data);
                }
                hasher.update([0x01]);
            }
        } else {
            warn!("File {} not found", path.absolute.display());
            hasher.update([0x00]);
        }

        hasher.update(seed);
        hasher.update(label.as_bytes());
        Ok(hasher.finalize().to_vec())
    }

    fn soft_digest(&self, label: &str, seed: &[u8]) -> Result<Option<Vec<u8>>> {
        if label.starts_with('@') {
            return Ok(None);
        }
        let Some(path) = self.resolve_label(label)? else {
            return Ok(None);
        };
        if !path.absolute.exists() || !path.absolute.is_file() {
            return Ok(None);
        }
        self.digest(label, seed).map(Some)
    }

    fn resolve_label(&self, label: &str) -> Result<Option<ResolvedPath>> {
        if let Some(path) = resolve_main_repo(label, &self.resolver.workspace) {
            return Ok(Some(path));
        }

        if let Some((repo, rel)) = split_external_label(label) {
            let normalized_repo = normalize_repo(repo);
            if !self
                .fine_grained_external_repos
                .iter()
                .any(|r| normalized_repo == *r)
            {
                return Ok(None);
            }

            let repo_root = self.resolver.resolve(&normalized_repo)?;
            let absolute = repo_root.join(rel.clone());
            let workspace_relative =
                format!("external/{}/{}", normalized_repo, rel.to_string_lossy());
            return Ok(Some(ResolvedPath {
                absolute,
                workspace_relative,
            }));
        }

        Ok(None)
    }
}

#[derive(Clone)]
struct ResolvedPath {
    absolute: PathBuf,
    workspace_relative: String,
}

#[derive(Clone, Debug)]
struct ExternalRepoResolver {
    workspace: PathBuf,
    bazel_path: PathBuf,
    startup_options: Vec<String>,
    output_base: PathBuf,
}

impl ExternalRepoResolver {
    fn resolve(&self, repo: &str) -> Result<PathBuf> {
        let external_root = self.output_base.join("external");
        for candidate in [repo.to_string(), format!("{repo}+")] {
            let path = external_root.join(&candidate);
            if path.exists() {
                return Ok(path);
            }
        }

        if let Some(path) = self.resolve_bzlmod_path(repo, &external_root)? {
            return Ok(path);
        }

        Ok(external_root.join(repo))
    }

    fn resolve_bzlmod_path(&self, repo: &str, external_root: &Path) -> Result<Option<PathBuf>> {
        let mut cmd = std::process::Command::new(&self.bazel_path);
        cmd.args(&self.startup_options);
        cmd.arg("query");
        cmd.arg(format!("@{repo}//..."));
        cmd.arg("--keep_going");
        cmd.arg("--output");
        cmd.arg("location");
        cmd.current_dir(&self.workspace);

        let output = cmd
            .output()
            .context("failed to run bazel query for repo mapping")?;
        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = stdout.lines().next() {
            let path_part = line.split(": ").next().unwrap_or(line);
            let path = PathBuf::from(path_part);
            if let Ok(rel) = path.strip_prefix(external_root) {
                if let Some(component) = rel.components().next() {
                    let repo_dir = external_root.join(component.as_os_str());
                    return Ok(Some(repo_dir));
                }
            }
        }
        Ok(None)
    }
}

struct RuleHasher<'a> {
    use_cquery: bool,
    fine_grained_external_repos: HashSet<String>,
    ignored_attrs: HashSet<String>,
    source_hasher: SourceFileHasher,
    source_digests: &'a mut HashMap<String, Vec<u8>>,
    rule_digests: &'a mut HashMap<String, TargetDigest>,
    seed_hash: Vec<u8>,
    track_dep_edges: bool,
}

impl<'a> RuleHasher<'a> {
    fn digest(
        &mut self,
        rule: &BazelRule,
        all_rules: &HashMap<String, BazelRule>,
        stack: &mut Vec<String>,
    ) -> Result<TargetDigest> {
        if let Some(existing) = self.rule_digests.get(&rule.name) {
            return Ok(existing.clone());
        }

        if stack.contains(&rule.name) {
            bail!("Circular dependency detected: {}", stack.join(" -> "));
        }
        stack.push(rule.name.clone());

        let mut builder = DigestBuilder::new(self.track_dep_edges);
        let rule_digest = rule.digest(&self.ignored_attrs);
        if cfg!(debug_assertions) {
            debug!(
                rule = %rule.name,
                rule_digest = %hex_encode(&rule_digest),
                seed_hash = %hex_encode(&self.seed_hash),
                "rule digest inputs"
            );
        }

        builder.put_direct(&rule_digest);
        builder.put_direct(&self.seed_hash);

        let seed = Vec::new();

        let inputs = rule.rule_inputs(self.use_cquery, &self.fine_grained_external_repos);
        debug!(rule = %rule.name, inputs = ?inputs, "hashing rule");

        for input in inputs {
            builder.put_direct(input.as_bytes());
            if let Some(dep_rule) = all_rules.get(&input) {
                if dep_rule.name != rule.name {
                    let dep_digest = self.digest(dep_rule, all_rules, stack)?;
                    builder.put_transitive(&input, &dep_digest.overall);
                }
            } else if let Some(source_digest) = self.source_digests.get(&input) {
                builder.put_direct(source_digest);
            } else if let Some(heuristic) = self.source_hasher.soft_digest(&input, &seed)? {
                let adjusted = if input.starts_with("@@") && input.contains('+') {
                    target_digest_from_source(&heuristic, &self.seed_hash).overall
                } else {
                    heuristic.clone()
                };
                self.source_digests.insert(input.clone(), adjusted.clone());
                builder.put_direct(&adjusted);
            } else {
                warn!(
                    "Unable to calculate digest for input {} of rule {}",
                    input, rule.name
                );
            }
        }

        stack.pop();

        let digest = builder.finish();
        if rule.name.contains("pkg:core") || rule.name.contains("pkg:tool") {
            debug!(
                rule = %rule.name,
                direct = %hex_encode(&digest.direct),
                overall = %hex_encode(&digest.overall),
                deps = ?digest.deps,
                "rule digest result"
            );
        }
        self.rule_digests.insert(rule.name.clone(), digest.clone());
        Ok(digest)
    }

    fn digest_generated(
        &mut self,
        generated: &BazelGenerated,
        all_rules: &HashMap<String, BazelRule>,
        stack: &mut Vec<String>,
    ) -> Result<TargetDigest> {
        let rule = all_rules.get(&generated.generating_rule).ok_or_else(|| {
            anyhow!(
                "Missing generating rule {} for {}",
                generated.generating_rule,
                generated.name
            )
        })?;
        let digest = self.digest(rule, all_rules, stack)?;
        Ok(digest.clone_with_deps(Some(vec![generated.generating_rule.clone()])))
    }
}

fn transform_rule_input(input: &str, fine_grained: &HashSet<String>) -> String {
    let trimmed = input.trim_start_matches('@');
    if is_not_main_repo(trimmed) {
        let mut parts = trimmed.splitn(2, "//");
        if let Some(repo_part) = parts.next() {
            let normalized = normalize_repo(repo_part);
            if fine_grained.contains(&normalized) {
                let remainder = parts.next().unwrap_or_default();
                let canonical_repo = if repo_part.ends_with('+') {
                    repo_part.to_string()
                } else {
                    format!("{repo_part}+")
                };
                return format!("@@{canonical_repo}//{remainder}");
            } else {
                return format!("//external:{repo_part}");
            }
        }
    }
    input.to_string()
}

fn is_not_main_repo(input: &str) -> bool {
    !(input.starts_with("//") || input.starts_with("@//") || input.starts_with("@@//"))
}

fn resolve_main_repo(label: &str, workspace: &Path) -> Option<ResolvedPath> {
    let prefix_len = if label.starts_with("//") {
        2
    } else if label.starts_with("@//") {
        3
    } else if label.starts_with("@@//") {
        4
    } else {
        return None;
    };

    let trimmed = &label[prefix_len..];
    let normalized = trimmed.trim_start_matches(':');
    let relative = normalized.replace(':', "/");
    let abs = workspace.join(&relative);
    Some(ResolvedPath {
        absolute: abs,
        workspace_relative: relative,
    })
}

fn split_external_label(label: &str) -> Option<(&str, PathBuf)> {
    if !label.starts_with('@') {
        return None;
    }
    let trimmed = label.trim_start_matches('@');
    let mut parts = trimmed.splitn(2, "//");
    let repo = parts.next()?;
    let rest = parts.next().unwrap_or_default();
    let rel = if rest.starts_with(':') {
        PathBuf::from(rest.trim_start_matches(':'))
    } else {
        PathBuf::from(rest.replace(':', "/"))
    };
    Some((repo, rel))
}

fn trim_repo_name(repo: &str) -> String {
    repo.trim_start_matches('@').to_string()
}

fn normalize_repo(repo: &str) -> String {
    trim_repo_name(repo)
}

fn target_label(target: &Target) -> Option<&str> {
    target
        .rule
        .as_ref()
        .map(|r| r.name.as_str())
        .or_else(|| target.source_file.as_ref().map(|s| s.name.as_str()))
        .or_else(|| target.generated_file.as_ref().map(|g| g.name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use seq_macro::seq;
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn spec_normalize_repo(input: &str) -> String {
        input.trim_start_matches('@').to_string()
    }

    fn spec_transform_rule_input(input: &str, fine_grained: &HashSet<String>) -> String {
        let trimmed = input.trim_start_matches('@');
        if trimmed.starts_with("//") || trimmed.starts_with("@//") || trimmed.starts_with("@@//") {
            return input.to_string();
        }

        let mut parts = trimmed.splitn(2, "//");
        let repo_part = parts.next().unwrap_or_default();
        if repo_part.is_empty() {
            return input.to_string();
        }

        let normalized = repo_part.trim_start_matches('@');
        if fine_grained.contains(normalized) {
            let remainder = parts.next().unwrap_or_default();
            let canonical = if repo_part.ends_with('+') {
                repo_part.to_string()
            } else {
                format!("{repo_part}+")
            };
            format!("@@{canonical}//{remainder}")
        } else {
            format!("//external:{repo_part}")
        }
    }

    fn spec_split_external_label(label: &str) -> Option<(String, PathBuf)> {
        if !label.starts_with('@') {
            return None;
        }
        let trimmed = label.trim_start_matches('@');
        let mut parts = trimmed.splitn(2, "//");
        let repo = parts.next()?.to_string();
        let rest = parts.next().unwrap_or_default();
        let rel = if rest.starts_with(':') {
            PathBuf::from(rest.trim_start_matches(':'))
        } else {
            PathBuf::from(rest.replace(':', "/"))
        };
        Some((repo, rel))
    }

    seq!(N in 0..200 {
        #[test]
        fn normalize_repo_variants_~N() {
            let n = N;
            let prefix = if n % 4 == 0 { "@@" } else { "@" };
            let suffix = match n % 3 {
                0 => "+",
                1 => "++",
                _ => "",
            };
            let repo = format!("{prefix}dep{n}{suffix}");
            assert_eq!(normalize_repo(&repo), spec_normalize_repo(&repo));
        }
    });

    seq!(N in 0..220 {
        #[test]
        fn transform_rule_input_variants_~N() {
            let n = N;
            let repo = format!("depmod{n}{}", if n % 7 == 0 { "+" } else { "" });
            let remainder = if n % 5 == 0 {
                format!("pkg{n}:lib{n}")
            } else {
                format!("pkg{n}/nested:target{n}")
            };
            let prefix = if n % 4 == 0 { "@@" } else { "@" };
            let input = format!("{prefix}{repo}//{remainder}");

            let mut fine = HashSet::new();
            if n % 3 == 0 {
                fine.insert(repo.clone());
            }

            let expected = spec_transform_rule_input(&input, &fine);
            let actual = transform_rule_input(&input, &fine);
            assert_eq!(actual, expected);
        }
    });

    seq!(N in 0..160 {
        #[test]
        fn split_external_label_variants_~N() {
            let n = N;
            if n % 4 == 0 {
                let label = format!("//pkg{n}:target{n}");
                assert!(split_external_label(&label).is_none());
                return;
            }

            let repo = if n % 2 == 0 {
                format!("toolchain{n}+")
            } else {
                format!("toolchain{n}")
            };
            let target = if n % 5 == 0 {
                format!(":file{n}.bzl")
            } else {
                format!("pkg{n}:lib{n}")
            };
            let label = format!("@{repo}//{target}");

            let expected = spec_split_external_label(&label);
            let actual = split_external_label(&label).map(|(r, p)| (r.to_string(), p));
            assert_eq!(actual, expected);
        }
    });

    #[test]
    fn transform_rule_input_main_repo_is_identity() {
        let fine = HashSet::new();
        for label in ["//pkg:lib", "@//pkg:lib", "@@//pkg:lib"] {
            assert_eq!(transform_rule_input(label, &fine), label);
        }
    }

    #[test]
    fn soft_digest_skips_all_external_labels() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let resolver = ExternalRepoResolver {
            workspace: tmp.path().to_path_buf(),
            bazel_path: PathBuf::from("bazel"),
            startup_options: Vec::new(),
            output_base: tmp.path().join("out"),
        };
        let hasher = SourceFileHasher::new(
            resolver,
            None,
            HashSet::from(["extrepo".to_string()]),
            HashSet::new(),
        );
        let seed = b"seed";
        for label in [
            "@extrepo//:file",
            "@@extrepo+//:file",
            "@depmod+//pkg:target",
        ] {
            assert!(hasher.soft_digest(label, seed)?.is_none());
        }
        Ok(())
    }

    #[test]
    fn soft_digest_hashes_main_repo_files() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path();
        let file_path = workspace.join("hello.txt");
        std::fs::write(&file_path, b"hello bazel-differrous")?;

        let resolver = ExternalRepoResolver {
            workspace: workspace.to_path_buf(),
            bazel_path: PathBuf::from("bazel"),
            startup_options: Vec::new(),
            output_base: workspace.join("out"),
        };
        std::fs::create_dir_all(&resolver.output_base)?;

        let hasher = SourceFileHasher::new(resolver, None, HashSet::new(), HashSet::new());
        let digest = hasher.soft_digest("//hello.txt", b"seed")?;
        assert!(digest.is_some());
        assert!(!digest.unwrap().is_empty());
        Ok(())
    }
}
