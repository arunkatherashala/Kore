// Example: Basic CLI usage patterns for kore-cli
//
// Run with: cargo run --example basic_usage

use std::process::Command;

fn main() {
    println!("🔧 Kore CLI - Basic Usage Examples\n");

    // Example 1: Inspect a file
    println!("1️⃣  Inspect file metadata:");
    println!("   $ kore inspect data.kore");
    println!("   $ kore inspect data.kore --detailed --schema --compression\n");

    // Example 2: Validate file
    println!("2️⃣  Validate file integrity:");
    println!("   $ kore validate data.kore");
    println!("   $ kore validate data.kore --checksum --encryption --schema\n");

    // Example 3: Convert format
    println!("3️⃣  Convert between formats:");
    println!("   $ kore convert input.kore output.kore --format parquet --compression zstd");
    println!("   $ kore convert data.kore data.json --format json");
    println!("   $ kore convert data.kore encrypted.kore --encrypt mykey\n");

    // Example 4: Analyze performance
    println!("4️⃣  Analyze file performance:");
    println!("   $ kore analyze data.kore --analysis performance");
    println!("   $ kore analyze data.kore --analysis compression --recommendations");
    println!("   $ kore analyze data.kore --analysis all --format json\n");

    // Example 5: Batch processing
    println!("5️⃣  Batch process multiple files:");
    println!("   $ kore batch '*.kore' --operation validate --parallel 8");
    println!("   $ kore batch 'data/*.kore' --operation inspect --output results/\n");

    // Example 6: Diff files
    println!("6️⃣  Compare files:");
    println!("   $ kore diff original.kore modified.kore");
    println!("   $ kore diff old.kore new.kore --detailed\n");

    // Example 7: Generate report
    println!("7️⃣  Generate report:");
    println!("   $ kore report data.kore --report-type summary");
    println!("   $ kore report data.kore --report-type compliance --recommendations\n");

    println!("✅ Try running these commands to see kore-cli in action!");
}
