/// Snowflake Job Example
/// Demonstrates loading KORE data into Snowflake and exporting back

use kore_snowflake::{SnowflakeConfig, SnowflakeDataLoader, SnowflakeExporter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    // Configure Snowflake connection
    let config = SnowflakeConfig {
        account_identifier: "xy12345.us-east-1".to_string(),
        warehouse: "COMPUTE_WH".to_string(),
        database: "KORE_DB".to_string(),
        schema: "PUBLIC".to_string(),
        role: "ACCOUNTADMIN".to_string(),
        timeout_secs: 300,
        max_connections: 10,
        auto_commit: true,
    };

    println!("═══════════════════════════════════════════════════════════════");
    println!("KORE Snowflake Data Loader Example");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Example 1: Load KORE file into Snowflake
    println!("Example 1: Load KORE file into Snowflake");
    println!("─────────────────────────────────────────");
    let mut loader = SnowflakeDataLoader::new(config.clone());

    if let Err(e) = loader.connect().await {
        eprintln!("Connection failed: {}", e);
        return Err(e);
    }
    println!("✓ Connected to Snowflake warehouse: {}", config.warehouse);

    // Load KORE file as table
    let table_name = "kore_data_sample";
    match loader.load_kore_table("sample_10mb.kore", table_name).await {
        Ok(schema) => {
            println!("✓ Loaded KORE file into table: {}", table_name);
            println!("  Rows: {}", schema.row_count);
            println!("  Size: {} bytes", schema.size_bytes);
            println!("  Columns: {}", schema.columns.len());
            for col in &schema.columns {
                println!(
                    "    - {}: {} (nullable: {})",
                    col.name, col.data_type, col.nullable
                );
            }
        }
        Err(e) => eprintln!("Failed to load KORE file: {}", e),
    }

    // Example 2: Stream KORE data with batches
    println!("\nExample 2: Stream KORE data to Snowflake");
    println!("─────────────────────────────────────────");
    match loader
        .stream_kore_data("sample_10mb.kore", "kore_data_streamed", 100_000)
        .await
    {
        Ok(rows_processed) => {
            println!("✓ Streamed {} rows to Snowflake", rows_processed);
        }
        Err(e) => eprintln!("Streaming failed: {}", e),
    }

    // Example 3: Execute query
    println!("\nExample 3: Execute Snowflake Query");
    println!("─────────────────────────────────────");
    let query = "SELECT COUNT(*) as total_rows FROM kore_data_sample";
    match loader.execute_query(query).await {
        Ok(results) => {
            println!("✓ Query executed: {}", query);
            println!("  Results: {} rows", results.len());
        }
        Err(e) => eprintln!("Query failed: {}", e),
    }

    // Example 4: Export Snowflake table to KORE
    println!("\nExample 4: Export Snowflake table to KORE");
    println!("──────────────────────────────────────────");
    let mut exporter = SnowflakeExporter::new(config.clone());

    if let Err(e) = exporter.connect().await {
        eprintln!("Exporter connection failed: {}", e);
        return Err(e);
    }
    println!("✓ Exporter connected to Snowflake");

    match exporter
        .export_to_kore("kore_data_sample", "exported_data.kore")
        .await
    {
        Ok(rows_exported) => {
            println!("✓ Exported {} rows from Snowflake to KORE", rows_exported);
        }
        Err(e) => eprintln!("Export failed: {}", e),
    }

    // Cleanup
    loader.disconnect().await?;
    println!("\n✓ Disconnected from Snowflake\n");

    println!("═══════════════════════════════════════════════════════════════");
    println!("Example completed successfully!");
    println!("═══════════════════════════════════════════════════════════════");

    Ok(())
}
