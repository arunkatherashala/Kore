mod commands;
mod error;
mod metadata;
mod utils;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "kore")]
#[command(about = "Kore File Format CLI - Inspect, validate, convert, and analyze Kore files", long_about = None)]
#[command(version)]
#[command(author = "Arun Kather Ashala <arun@kore.dev>")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(global = true, short, long)]
    verbose: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(global = true, long, default_value = "info")]
    log_level: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Display Kore file metadata, schema, and statistics
    Inspect {
        /// Path to Kore file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Output format (table, json, yaml)
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Show detailed metadata
        #[arg(short, long)]
        detailed: bool,

        /// Show schema information
        #[arg(long)]
        schema: bool,

        /// Show compression statistics
        #[arg(long)]
        compression: bool,
    },

    /// Verify Kore file integrity, checksums, and encryption
    Validate {
        /// Path to Kore file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Verify checksums
        #[arg(short, long)]
        checksum: bool,

        /// Verify encryption
        #[arg(short, long)]
        encryption: bool,

        /// Verify schema consistency
        #[arg(long)]
        schema: bool,

        /// Generate repair suggestions
        #[arg(long)]
        repair: bool,

        /// Output format (table, json)
        #[arg(short, long, default_value = "table")]
        format: String,
    },

    /// Transform between Kore formats and versions
    Convert {
        /// Input file path
        #[arg(value_name = "INPUT")]
        input: PathBuf,

        /// Output file path
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,

        /// Target format (kore1, kore2, parquet, arrow, json)
        #[arg(short, long)]
        format: String,

        /// Compression algorithm (none, gzip, zstd)
        #[arg(long, default_value = "zstd")]
        compression: String,

        /// Encryption key (optional)
        #[arg(long)]
        encrypt: Option<String>,

        /// Show progress bar
        #[arg(long)]
        progress: bool,
    },

    /// Performance profiling, compression analysis, and optimization
    Analyze {
        /// Path to Kore file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Analysis type (performance, compression, schema, all)
        #[arg(short, long, default_value = "all")]
        analysis: String,

        /// Output format (table, json, html)
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Sample size for analysis (0 = full)
        #[arg(long, default_value = "10000")]
        samples: usize,

        /// Generate optimization recommendations
        #[arg(long)]
        recommendations: bool,
    },

    /// Batch process multiple files
    Batch {
        /// Input directory pattern
        #[arg(value_name = "PATTERN")]
        pattern: String,

        /// Operation (inspect, validate, convert)
        #[arg(short, long)]
        operation: String,

        /// Output directory for results
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Number of parallel jobs
        #[arg(short, long, default_value = "4")]
        parallel: usize,
    },

    /// Show file diff and changes
    Diff {
        /// First file path
        #[arg(value_name = "FILE1")]
        file1: PathBuf,

        /// Second file path
        #[arg(value_name = "FILE2")]
        file2: PathBuf,

        /// Show detailed diff
        #[arg(short, long)]
        detailed: bool,

        /// Statistics only
        #[arg(long)]
        stats_only: bool,
    },

    /// Generate comprehensive report
    Report {
        /// Input file path
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Report type (summary, detailed, compliance)
        #[arg(short, long, default_value = "summary")]
        report_type: String,

        /// Output file (optional, defaults to stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Include recommendations
        #[arg(long)]
        recommendations: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let log_level = if cli.verbose {
        "debug"
    } else {
        &cli.log_level
    };

    tracing_subscriber::fmt()
        .with_max_level(
            log_level
                .parse()
                .unwrap_or(tracing::Level::INFO),
        )
        .init();

    // Route to subcommand
    match cli.command {
        Commands::Inspect {
            file,
            format,
            detailed,
            schema,
            compression,
        } => {
            commands::inspect::inspect(
                file,
                &format,
                detailed,
                schema,
                compression,
            )
            .await?;
        }
        Commands::Validate {
            file,
            checksum,
            encryption,
            schema,
            repair,
            format,
        } => {
            commands::validate::validate(
                file,
                checksum,
                encryption,
                schema,
                repair,
                &format,
            )
            .await?;
        }
        Commands::Convert {
            input,
            output,
            format,
            compression,
            encrypt,
            progress,
        } => {
            commands::convert::convert(
                input,
                output,
                &format,
                &compression,
                encrypt,
                progress,
            )
            .await?;
        }
        Commands::Analyze {
            file,
            analysis,
            format,
            samples,
            recommendations,
        } => {
            commands::analyze::analyze(
                file,
                &analysis,
                &format,
                samples,
                recommendations,
            )
            .await?;
        }
        Commands::Batch {
            pattern,
            operation,
            output,
            parallel,
        } => {
            commands::batch::batch_process(
                &pattern,
                &operation,
                output,
                parallel,
            )
            .await?;
        }
        Commands::Diff {
            file1,
            file2,
            detailed,
            stats_only,
        } => {
            commands::diff::diff_files(
                file1,
                file2,
                detailed,
                stats_only,
            )
            .await?;
        }
        Commands::Report {
            file,
            report_type,
            output,
            recommendations,
        } => {
            commands::report::generate_report(
                file,
                &report_type,
                output,
                recommendations,
            )
            .await?;
        }
    }

    Ok(())
}
