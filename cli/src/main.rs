//! `twin` — the binary that wires the pipeline together.
//!
//!   twin ingest <file.ifc> [--neo4j-uri ...]   parse -> graph -> Cypher
//!   twin serve  [--addr 127.0.0.1:8080]        query API over an ingested graph
//!
//! Each subcommand is a thin composition of the edge crates around the neutral
//! `SemanticGraph`. The dependency direction is one-way: cli -> {ingest, store,
//! api} -> core. No edge crate knows about any other.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use twin_api::{build_router, AppState};
use twin_core::SemanticGraph;
use twin_ingest_ifc4::{ingest_many, ingest_sample, IngestError, IngestReport};
use twin_store_neo4j::{write_graph, Neo4jConfig, StoreError};

#[derive(Parser)]
#[command(name = "twin", version, about = "IFC/graph digital-twin backbone")]
struct Cli {
    /// Log output format. Text is human-readable; json emits one structured
    /// object per line for log aggregators.
    #[arg(long, value_enum, default_value_t = LogFormat::Text, global = true)]
    log_format: LogFormat,
    #[command(subcommand)]
    command: Command,
}

/// Verbosity is controlled separately via the standard `RUST_LOG` env var
/// (e.g. `RUST_LOG=debug` or `RUST_LOG=twin_ingest_ifc4=trace`); defaults to info.
#[derive(Copy, Clone, Debug, ValueEnum)]
enum LogFormat {
    Text,
    Json,
}

#[derive(Subcommand)]
enum Command {
    /// Parse an IFC file into the semantic graph and emit the Cypher to persist it.
    /// Requires the `real-parser` feature; without it this fails (no `twin ingest`
    /// of a file returns fabricated data). For a zero-dependency run, use `demo`.
    Ingest {
        /// Path to the .ifc file.
        file: PathBuf,
        /// Neo4j URI. Used as the print target label, and as the connection
        /// target when `--execute` is given.
        #[arg(long, default_value = "bolt://127.0.0.1:7687")]
        neo4j_uri: String,
        /// Run the Cypher against a live Neo4j over Bolt instead of printing it.
        /// Requires the `bolt` build feature (`cargo run -p cli --features bolt`).
        #[arg(long)]
        execute: bool,
        /// Neo4j username (only used with `--execute`).
        #[arg(long, default_value = "neo4j")]
        neo4j_user: String,
        /// Neo4j password (only used with `--execute`).
        #[arg(long, default_value = "password")]
        neo4j_password: String,
    },
    /// Build the built-in sample graph (no file, no parser) and emit its Cypher.
    /// The honest replacement for the old mock-as-default behaviour: clearly
    /// labeled sample data, runnable with zero external dependencies.
    Demo {
        /// Neo4j URI. Print target label by default; connection target with
        /// `--execute`.
        #[arg(long, default_value = "bolt://127.0.0.1:7687")]
        neo4j_uri: String,
        /// Write the sample graph to a live Neo4j over Bolt instead of printing.
        /// Requires the `bolt` build feature. The zero-parser smoke test.
        #[arg(long)]
        execute: bool,
        /// Neo4j username (only used with `--execute`).
        #[arg(long, default_value = "neo4j")]
        neo4j_user: String,
        /// Neo4j password (only used with `--execute`).
        #[arg(long, default_value = "password")]
        neo4j_password: String,
    },
    /// Ingest file(s) (or start empty) and serve the read-only query API.
    Serve {
        /// IFC file(s) to load before serving. Repeat `--file` to federate
        /// several discipline models into one graph, e.g.
        /// `--file Building-Architecture.ifc --file Building-Structural.ifc`.
        /// Each file's nodes are tagged with a `model` label from its name.
        #[arg(long)]
        file: Vec<PathBuf>,
        /// Address to bind.
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: SocketAddr,
        /// Optionally emit (print) the Cypher for the same graph being served, so
        /// the persisted model and the queried model are one build. Prints rather
        /// than connecting; live write-through can follow the `ingest --execute`
        /// path when wanted.
        #[arg(long)]
        neo4j_uri: Option<String>,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.log_format);

    match run(cli.command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "command failed");
            ExitCode::FAILURE
        }
    }
}

/// Install the global tracing subscriber. Filter comes from `RUST_LOG`
/// (default `info`); the formatter is chosen by `--log-format`.
fn init_tracing(format: LogFormat) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    // The two formatters have different types, so each branch calls `.init()`.
    match format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder.json().init(),
    }
}

/// The single graph-building path. Both subcommands go through here, so the
/// write model (Neo4j) and the read model (the API) are never built two
/// different ways. No files yields an empty graph (serve with nothing loaded);
/// one or many go through the federated [`ingest_many`], each tagged with a
/// `model` label. The [`IngestReport`] rides along so callers can surface
/// dropped relationships.
fn load_graph(files: &[PathBuf]) -> Result<(SemanticGraph, IngestReport), IngestError> {
    if files.is_empty() {
        return Ok((SemanticGraph::new(), IngestReport::default()));
    }
    let inputs: Vec<(String, PathBuf)> =
        files.iter().map(|p| (model_label(p), p.clone())).collect();
    ingest_many(&inputs)
}

/// Derive a short model/discipline label from a file name, e.g.
/// `Building-Architecture.ifc` → `architecture`, `Infra-Rail.ifc` → `rail`.
/// Just the last hyphen-separated chunk of the stem, lowercased — enough to make
/// federated queries (`?model=structural`) read naturally.
fn model_label(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("model");
    stem.rsplit('-').next().unwrap_or(stem).to_ascii_lowercase()
}

/// The offline persistence path: emit `graph` as Cypher to stdout, one
/// statement at a time as produced. Returns the count for the summary. To write
/// to a live database instead, see [`persist_bolt`].
fn persist(graph: &SemanticGraph, uri: String) -> Result<usize, StoreError> {
    let cfg = Neo4jConfig {
        uri,
        ..Neo4jConfig::default()
    };
    let mut count = 0usize;
    write_graph(graph, &cfg, |stmt| {
        println!("{};", stmt.query);
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// The live persistence path: run the same statements against Neo4j over Bolt.
/// Shares `store-neo4j`'s builders with [`persist`]; only the consumer differs.
/// Compiled only under the `bolt` feature.
#[cfg(feature = "bolt")]
async fn persist_bolt(
    graph: &SemanticGraph,
    uri: String,
    user: String,
    password: String,
) -> Result<usize, StoreError> {
    let cfg = Neo4jConfig {
        uri,
        user,
        password,
        ..Neo4jConfig::default()
    };
    twin_store_neo4j::execute_graph(graph, &cfg).await
}

/// Without the `bolt` feature there is no driver linked, so `--execute` fails
/// loudly rather than silently printing — mirroring how `ingest` refuses to
/// fabricate data without `real-parser`.
#[cfg(not(feature = "bolt"))]
async fn persist_bolt(
    _graph: &SemanticGraph,
    _uri: String,
    _user: String,
    _password: String,
) -> Result<usize, StoreError> {
    Err(StoreError::Connection(
        "this build has no Bolt driver; rebuild with `--features bolt` to use --execute".into(),
    ))
}

async fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Ingest {
            file,
            neo4j_uri,
            execute,
            neo4j_user,
            neo4j_password,
        } => {
            let (graph, report) = load_graph(std::slice::from_ref(&file))?;
            tracing::info!(
                nodes = report.nodes,
                edges = report.edges,
                dropped = report.dropped(),
                "ingested"
            );
            if execute {
                let count =
                    persist_bolt(&graph, neo4j_uri, neo4j_user, neo4j_password).await?;
                println!("-- {count} statements committed to Neo4j --");
            } else {
                let count = persist(&graph, neo4j_uri)?;
                println!("-- {count} Cypher statements --");
            }
            // Surface the report so dropped relationships aren't a silent loss.
            println!("-- {report} --");
            Ok(())
        }

        Command::Demo {
            neo4j_uri,
            execute,
            neo4j_user,
            neo4j_password,
        } => {
            let (graph, report) = ingest_sample()?;
            tracing::info!(
                nodes = report.nodes,
                edges = report.edges,
                "demo (sample data — not a parsed file)"
            );
            if execute {
                let count =
                    persist_bolt(&graph, neo4j_uri, neo4j_user, neo4j_password).await?;
                println!("-- {count} statements committed to Neo4j (SAMPLE DATA) --");
            } else {
                let count = persist(&graph, neo4j_uri)?;
                println!("-- {count} Cypher statements --");
                println!("-- {report} (SAMPLE DATA) --");
            }
            Ok(())
        }

        Command::Serve {
            file,
            addr,
            neo4j_uri,
        } => {
            let (graph, report) = load_graph(&file)?;
            tracing::info!(
                nodes = report.nodes,
                edges = report.edges,
                dropped = report.dropped(),
                "loaded"
            );
            // Write-through the *same* in-memory graph we're about to serve, so
            // the persisted model and the queried model are one object, not two
            // independent builds.
            if let Some(uri) = neo4j_uri {
                let count = persist(&graph, uri)?;
                tracing::info!(statements = count, "persisted before serving");
            }
            tracing::info!(nodes = graph.node_count(), %addr, "serving");

            let app = build_router(AppState::new(graph));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
            Ok(())
        }
    }
}
