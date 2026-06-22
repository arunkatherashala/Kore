use std::process::Command;
use std::env;
use serde_json::Value;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: kore-sample <kore-file>");
        std::process::exit(2);
    }
    let file = &args[1];
    let kore_exe = "target/release/kore.exe";

    let output = Command::new(kore_exe)
        .arg("analyze")
        .arg("--analysis")
        .arg("all")
        .arg("--samples")
        .arg("10")
        .arg("--format")
        .arg("json")
        .arg(file)
        .output()?;

    if !output.status.success() {
        eprintln!("kore analyze failed: {}", String::from_utf8_lossy(&output.stderr));
        std::process::exit(3);
    }

    let s = String::from_utf8_lossy(&output.stdout).to_string();
    // The kore CLI may print logs before the JSON blob; find the first '{' and parse from there.
    let json_start = s.find('{').ok_or_else(|| anyhow::anyhow!("no JSON found in kore output"))?;
    let json_str = &s[json_start..];
    let v: Value = serde_json::from_str(json_str)?;

    println!("Analysis summary:");
    println!("  File Size: {}", v.get("file_size").unwrap_or(&Value::String("n/a".into())));
    println!("  Compression Ratio: {}", v.get("compression_ratio").unwrap_or(&Value::String("n/a".into())));
    println!("  Throughput (MB/s): {}", v.get("throughput_mbps").unwrap_or(&Value::String("n/a".into())));
    println!("  Compressible: {}", v.get("compressible").unwrap_or(&Value::String("n/a".into())));

    if let Some(samples) = v.get("samples") {
        println!("\nSample rows:");
        println!("{}", samples);
    } else {
        println!("\nNo sample rows present in analysis output.");
    }

    if let Some(cols) = v.get("columns") {
        println!("\nPer-column stats:");
        println!("{}", cols);
    } else {
        println!("\nNo per-column stats present in analysis output.");
    }

    Ok(())
}
