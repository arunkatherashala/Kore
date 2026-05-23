use std::path::PathBuf;
use anyhow::Result;
use comfy_table::Table;
use serde_json::json;
use tracing::info;
use std::fs;

/// Show file diff and changes
pub async fn diff_files(
    file1: PathBuf,
    file2: PathBuf,
    detailed: bool,
    stats_only: bool,
) -> Result<()> {
    info!("Comparing: {} <-> {}", file1.display(), file2.display());

    let data1 = fs::read(&file1)?;
    let data2 = fs::read(&file2)?;

    let mut stats = DiffStats::new(&data1, &data2);

    if stats_only {
        stats.print_table();
        return Ok(());
    }

    // Calculate diff
    if detailed {
        println!("Detailed Binary Diff:");
        print_binary_diff(&data1, &data2)?;
    } else {
        stats.print_table();
    }

    Ok(())
}

struct DiffStats {
    size1: u64,
    size2: u64,
    different_bytes: u64,
    similarity: f64,
}

impl DiffStats {
    fn new(data1: &[u8], data2: &[u8]) -> Self {
        let size1 = data1.len() as u64;
        let size2 = data2.len() as u64;

        let mut different_bytes = 0u64;
        let min_len = std::cmp::min(data1.len(), data2.len());

        for i in 0..min_len {
            if data1[i] != data2[i] {
                different_bytes += 1;
            }
        }

        // Account for size differences
        let size_diff = (size1 as i64 - size2 as i64).abs() as u64;
        different_bytes += size_diff;

        let max_size = std::cmp::max(size1, size2) as f64;
        let similarity = if max_size > 0.0 {
            ((max_size - different_bytes as f64) / max_size) * 100.0
        } else {
            100.0
        };

        Self {
            size1,
            size2,
            different_bytes,
            similarity,
        }
    }

    fn print_table(&self) {
        let mut table = Table::new();
        table.add_row(vec!["Metric", "File 1", "File 2", "Difference"]);
        table.add_row(vec![
            "Size",
            &format!("{} B", self.size1),
            &format!("{} B", self.size2),
            &format!("{} B", (self.size1 as i64 - self.size2 as i64).abs()),
        ]);
        table.add_row(vec![
            "Different Bytes",
            "",
            "",
            &format!("{} ({:.2}%)", self.different_bytes, (self.different_bytes as f64 / std::cmp::max(self.size1, self.size2) as f64) * 100.0),
        ]);
        table.add_row(vec![
            "Similarity",
            "",
            "",
            &format!("{:.2}%", self.similarity),
        ]);

        println!("{table}");
    }
}

fn print_binary_diff(data1: &[u8], data2: &[u8]) -> Result<()> {
    println!("\n{'Offset':<10} {'File1':<20} {'File2':<20} {'Status':<10}");
    println!("{}", "=".repeat(60));

    let min_len = std::cmp::min(data1.len(), data2.len());
    let mut diff_count = 0;

    for i in (0..min_len).step_by(16) {
        let chunk1 = &data1[i..std::cmp::min(i + 16, data1.len())];
        let chunk2 = &data2[i..std::cmp::min(i + 16, data2.len())];

        if chunk1 != chunk2 {
            let hex1 = hex::encode(chunk1);
            let hex2 = hex::encode(chunk2);

            println!(
                "{:<10x} {:<20} {:<20} DIFF",
                i, hex1, hex2
            );

            diff_count += 1;

            if diff_count >= 10 {
                println!("... (showing first 10 differences)");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_identical() {
        let data = b"Hello, World!";
        let stats = DiffStats::new(data, data);
        assert_eq!(stats.similarity, 100.0);
    }

    #[test]
    fn test_diff_completely_different() {
        let data1 = b"AAAAAAAAAA";
        let data2 = b"BBBBBBBBBB";
        let stats = DiffStats::new(data1, data2);
        assert!(stats.similarity < 100.0);
    }
}
