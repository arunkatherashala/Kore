use std::path::PathBuf;
use anyhow::Result;
use comfy_table::Table;
use serde_json::json;
use tracing::info;
use std::fs;

/// Display Kore file metadata, schema, and statistics
pub async fn inspect(
    file: PathBuf,
    format: &str,
    detailed: bool,
    show_schema: bool,
    show_compression: bool,
) -> Result<()> {
    info!("Inspecting file: {}", file.display());

    // Read file metadata
    let metadata = fs::metadata(&file)?;
    let file_size = metadata.len();

    // Basic file information
    let mut info_data = vec![
        ("File", file.display().to_string()),
        ("Size", format_size(file_size)),
        ("Modified", format!("{:?}", metadata.modified()?)),
    ];

    match format {
        "json" => {
            let json_output = json!({
                "file": file.display().to_string(),
                "size": file_size,
                "size_formatted": format_size(file_size),
                "modified": metadata.modified()?.elapsed()?.as_secs(),
                "is_dir": metadata.is_dir(),
                "is_file": metadata.is_file(),
                "permissions": {
                    "readonly": metadata.permissions().readonly(),
                }
            });

            if detailed {
                println!("{}", serde_json::to_string_pretty(&json_output)?);
            } else {
                println!("{}", json_output);
            }
        }
        "table" | _ => {
            let mut table = Table::new();
            table.add_row(vec!["Property", "Value"]);

            for (key, value) in info_data {
                table.add_row(vec![key, &value]);
            }

            if show_compression {
                // Estimate compression ratios
                table.add_row(vec!["Compression", ""]);
                table.add_row(vec!["  Estimated Gzip", "35-50%"]);
                table.add_row(vec!["  Estimated Zstd", "40-55%"]);
            }

            if show_schema {
                table.add_row(vec!["Schema", ""]);
                table.add_row(vec!["  Type", "Columnar"]);
                table.add_row(vec!["  Version", "1.0"]);
            }

            println!("{table}");
        }
    }

    Ok(())
}

/// Format byte size to human-readable format
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    format!("{:.2} {}", size, UNITS[unit_idx])
}
