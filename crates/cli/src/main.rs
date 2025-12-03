use anyhow::{bail, Context, Result};
use bazel_differrous_core as core;
use clap::{ArgAction, Args, Parser, Subcommand};
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "bazel-differrous",
    about = "Rust rewrite of bazel-diff (scaffolding stub)",
    version,
    author,
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(short = 'v', long, global = true, action = ArgAction::SetTrue)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Generate target hashes from Bazel query/cquery outputs.
    GenerateHashes(GenerateHashesArgs),
    /// Compute impacted targets between two hash sets.
    GetImpactedTargets(GetImpactedTargetsArgs),
}

#[derive(Args, Debug)]
struct GenerateHashesArgs {
    /// Path to the Bazel workspace to inspect.
    #[arg(
        short = 'w',
        long = "workspacePath",
        alias = "workspace-path",
        value_name = "DIR",
        required = true
    )]
    workspace_path: PathBuf,
    /// Output JSON path (STDOUT if omitted).
    #[arg(value_name = "OUTPUT")]
    output_path: Option<PathBuf>,
    /// Optional Bazel binary to invoke.
    #[arg(short = 'b', long = "bazelPath", alias = "bazel-path")]
    bazel_path: Option<PathBuf>,
    /// Additional Bazel startup options (before command).
    #[arg(
        long = "bazelStartupOptions",
        alias = "bazel-startup-options",
        value_delimiter = ' ',
        num_args = 0..
    )]
    bazel_startup_options: Vec<String>,
    /// Additional Bazel command options.
    #[arg(
        long = "bazelCommandOptions",
        alias = "bazel-command-options",
        value_delimiter = ' ',
        num_args = 0..
    )]
    bazel_command_options: Vec<String>,
    /// Additional Bazel cquery command options (only when --useCquery is set).
    #[arg(
        long = "cqueryCommandOptions",
        alias = "cquery-command-options",
        value_delimiter = ' ',
        num_args = 0..
    )]
    bazel_cquery_options: Vec<String>,
    /// Prefer cquery over query when generating the graph.
    #[arg(long = "useCquery", action = ArgAction::SetTrue)]
    use_cquery: bool,
    /// Whether to keep going on Bazel errors (mirrors upstream default=true).
    #[arg(short = 'k', long = "keep_going", default_value_t = true)]
    keep_going: bool,
    /// Include target type prefix (Rule/GeneratedFile/SourceFile) in hash values.
    #[arg(
        long = "includeTargetType",
        alias = "include-target-type",
        action = ArgAction::SetTrue
    )]
    include_target_type: bool,
    /// Placeholder for content hash map support (accepted for compatibility).
    #[arg(long = "contentHashPath", alias = "content-hash-path")]
    content_hash_path: Option<PathBuf>,
    /// Attributes to ignore when hashing rules.
    #[arg(
        long = "ignoredRuleHashingAttributes",
        alias = "ignored-rule-hashing-attributes",
        value_delimiter = ','
    )]
    ignored_attrs: Vec<String>,
    /// Whether to exclude external targets.
    #[arg(
        long = "excludeExternalTargets",
        alias = "exclude-external-targets",
        action = ArgAction::SetTrue
    )]
    exclude_external_targets: bool,
    /// Optional list of external repos to hash fine-grained targets for.
    #[arg(
        long = "fineGrainedHashExternalRepos",
        alias = "fine-grained-hash-external-repos",
        value_delimiter = ','
    )]
    fine_grained_external_repos: Vec<String>,
    /// File containing newline-separated external repos for fine-grained hashing.
    #[arg(
        long = "fineGrainedHashExternalReposFile",
        alias = "fine-grained-hash-external-repos-file"
    )]
    fine_grained_external_repos_file: Option<PathBuf>,
    /// Seed filepaths list; contents are hashed and mixed into all digests.
    #[arg(short = 's', long = "seed-filepaths")]
    seed_filepaths: Option<PathBuf>,
    /// Modified filepaths list; restricts which source files contribute content bytes.
    #[arg(short = 'm', long = "modified-filepaths")]
    modified_filepaths: Option<PathBuf>,
    /// Target types to keep in the output.
    #[arg(
        short = 't',
        long = "targetType",
        alias = "target-type",
        value_delimiter = ',',
        num_args = 1..
    )]
    target_types: Option<Vec<String>>,
    /// Optional dep edges output file.
    #[arg(
        short = 'd',
        long = "depEdgesFile",
        alias = "dep-edges-file",
        value_name = "FILE"
    )]
    dep_edges_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct GetImpactedTargetsArgs {
    /// Path to the baseline hash JSON.
    #[arg(
        short = 's',
        long = "startingHashes",
        value_name = "FILE",
        required = true
    )]
    start_hashes: PathBuf,
    /// Path to the updated hash JSON.
    #[arg(
        short = 'f',
        long = "finalHashes",
        value_name = "FILE",
        required = true
    )]
    final_hashes: PathBuf,
    /// Optional dependency edges JSON file.
    #[arg(short = 'd', long = "depEdgesFile", value_name = "FILE")]
    dep_edges: Option<PathBuf>,
    /// Target types to filter (requires hashes generated with --includeTargetType).
    #[arg(short = 't', long = "targetType", value_delimiter = ',', num_args = 1..)]
    target_types: Option<Vec<String>>,
    /// Optional output path (stdout if omitted).
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse_from(normalize_args(env::args_os()));
    init_tracing(cli.verbose);

    if let Err(err) = run(cli).await {
        error!(error = %err, "command failed");
        eprintln!("{err}");
        process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::GenerateHashes(args) => handle_generate_hashes(args).await,
        Commands::GetImpactedTargets(args) => handle_get_impacted_targets(args),
    }
}

async fn handle_generate_hashes(args: GenerateHashesArgs) -> Result<()> {
    if let Some(path) = &args.content_hash_path {
        if !path.is_file() {
            bail!("Incorrect contentHashFilePath: file doesn't exist or can't be read.");
        }
    }
    if args.fine_grained_external_repos_file.is_some()
        && !args.fine_grained_external_repos.is_empty()
    {
        bail!(
            "fineGrainedHashExternalReposFile and fineGrainedHashExternalRepos are mutually exclusive"
        );
    }

    let config = core::hash::GenerateHashesConfig {
        workspace: args.workspace_path.clone(),
        include_target_type: args.include_target_type,
        use_cquery: args.use_cquery,
        keep_going: args.keep_going,
        bazel_path: args.bazel_path.unwrap_or_default(),
        startup_options: args.bazel_startup_options.clone(),
        command_options: args.bazel_command_options.clone(),
        cquery_options: args.bazel_cquery_options.clone(),
        exclude_external_targets: args.exclude_external_targets,
        ignored_attrs: args.ignored_attrs.clone(),
        fine_grained_external_repos: args.fine_grained_external_repos.clone(),
        fine_grained_external_repos_file: args.fine_grained_external_repos_file.clone(),
        content_hash_path: args.content_hash_path.clone(),
        seed_filepaths: args.seed_filepaths.clone(),
        modified_filepaths: args.modified_filepaths.clone(),
        target_types: args.target_types.clone(),
        track_dep_edges: args.dep_edges_file.is_some(),
    };

    let result = core::hash::generate_hashes(&config).await?;

    let writer: Box<dyn Write> = match args.output_path {
        Some(path) => {
            Box::new(BufWriter::new(File::create(&path).with_context(|| {
                format!("failed to create output file {}", path.display())
            })?))
        }
        None => Box::new(BufWriter::new(std::io::stdout())),
    };

    serde_json::to_writer(writer, &result.hashes).context("failed to write hash JSON")?;

    if let Some(dep_path) = args.dep_edges_file {
        let mut file =
            BufWriter::new(File::create(&dep_path).with_context(|| {
                format!("failed to create dep edges file {}", dep_path.display())
            })?);
        serde_json::to_writer(&mut file, &result.dep_edges)
            .context("failed to write dep edges JSON")?;
        file.flush().context("failed to flush dep edges output")?;
    }

    info!(count = result.hashes.len(), "finished generate-hashes",);
    Ok(())
}

fn handle_get_impacted_targets(args: GetImpactedTargetsArgs) -> Result<()> {
    info!(
        start = %args.start_hashes.display(),
        final = %args.final_hashes.display(),
        dep_edges = args.dep_edges.as_ref().map(|p| p.display().to_string()),
        "computing impacted targets"
    );

    let result = core::get_impacted_targets(
        &args.start_hashes,
        &args.final_hashes,
        args.dep_edges.as_ref(),
        args.target_types,
    )?;

    let mut writer: Box<dyn Write> = match &args.output {
        Some(path) => {
            Box::new(BufWriter::new(File::create(path).with_context(|| {
                format!("failed to create output file {}", path.display())
            })?))
        }
        None => Box::new(BufWriter::new(std::io::stdout())),
    };

    let impacted_count = result.impacted.len();

    if let Some(distances) = result.distances {
        serde_json::to_writer_pretty(&mut writer, &distances)
            .context("failed to write impacted targets JSON")?;
    } else {
        for label in &result.impacted {
            writeln!(writer, "{}", label).context("failed to write impacted target")?;
        }
    }

    writer.flush().context("failed to flush output")?;
    info!(
        count = impacted_count,
        "finished computing impacted targets"
    );
    Ok(())
}

fn init_tracing(verbose: bool) {
    let default_level = if verbose { "debug" } else { "info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}

fn normalize_args<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter()
        .map(|arg| {
            let s = arg.to_string_lossy();

            normalize_flag(&s, "-sh", "--startingHashes")
                .or_else(|| normalize_flag(&s, "-fh", "--finalHashes"))
                .or_else(|| normalize_flag(&s, "-so", "--bazelStartupOptions"))
                .or_else(|| normalize_flag(&s, "-co", "--bazelCommandOptions"))
                .or_else(|| normalize_flag(&s, "-tt", "--targetType"))
                .unwrap_or_else(|| OsString::from(s.into_owned()))
        })
        .collect()
}

fn normalize_flag(input: &str, short: &str, long: &str) -> Option<OsString> {
    input.strip_prefix(short).and_then(|rest| {
        if rest.is_empty() || rest.starts_with('=') {
            Some(OsString::from(format!("{long}{rest}")))
        } else {
            None
        }
    })
}
