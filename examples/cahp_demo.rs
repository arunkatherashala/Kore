// examples/cahp_demo.rs
// Demonstration of Context-Aware Hybrid Predictor (CAHP)
// Shows real-world compression improvements

use kore_fileformat::compression::CAHPCompressor;

fn main() {
    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║  CAHP - Context-Aware Hybrid Predictor Demo               ║");
    println!("║  Novel Compression Algorithm for Kore v1.2.9             ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    // Demo 1: Repetitive Data (High Compression Potential)
    demo_repetitive_data();
    
    // Demo 2: Categorical Data (Medium Compression Potential)
    demo_categorical_data();
    
    // Demo 3: Time Series Data (Delta + CAHP)
    demo_timeseries_data();
    
    // Demo 4: Real-World CSV Sample
    demo_realworld_data();

    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║  Ready for Production: CAHP v1.0                         ║");
    println!("║  Deploy with: git tag v1.2.9 && git push origin v1.2.9  ║");
    println!("╚════════════════════════════════════════════════════════════╝");
}

fn demo_repetitive_data() {
    println!("\n📊 Demo 1: Highly Repetitive Data");
    println!("─────────────────────────────────");
    
    let data = b"aaabbbcccdddeeefffggghhh";
    let mut cahp = CAHPCompressor::new();
    
    let (compressed, stats) = cahp.compress(data);
    
    println!("Original:        {} bytes", stats.original_size);
    println!("After Predict:   {} bytes", stats.after_prediction);
    println!("Final Size:      {} bytes", stats.final_size);
    println!("Savings:         {:.1}%", (1.0 - stats.final_size as f32 / stats.original_size as f32) * 100.0);
    println!("Patterns Found:  {}", stats.patterns_learned);
    println!("Substitutions:   {}", stats.substitutions_made);
    println!("Accuracy:        {:.1}%", stats.prediction_accuracy * 100.0);
}

fn demo_categorical_data() {
    println!("\n📊 Demo 2: Categorical Data (Status Codes)");
    println!("──────────────────────────────────────────");
    
    let data = b"active inactive pending active active inactive active pending inactive active";
    let mut cahp = CAHPCompressor::new();
    
    let (compressed, stats) = cahp.compress(data);
    
    println!("Original:        {} bytes", stats.original_size);
    println!("After Predict:   {} bytes", stats.after_prediction);
    println!("Final Size:      {} bytes", stats.final_size);
    println!("Savings:         {:.1}%", (1.0 - stats.final_size as f32 / stats.original_size as f32) * 100.0);
    println!("Patterns Found:  {}", stats.patterns_learned);
    println!("Substitutions:   {}", stats.substitutions_made);
}

fn demo_timeseries_data() {
    println!("\n📊 Demo 3: Time Series (Temperature Readings)");
    println!("─────────────────────────────────────────────");
    
    // Simulated temperature data (smooth progression)
    let mut data = Vec::new();
    for i in 0..100 {
        let temp = 20.0 + ((i as f32 / 100.0) * 10.0);
        data.extend_from_slice(&temp.to_le_bytes());
    }
    
    let mut cahp = CAHPCompressor::new();
    let (compressed, stats) = cahp.compress(&data);
    
    println!("Original:        {} bytes", stats.original_size);
    println!("After Predict:   {} bytes", stats.after_prediction);
    println!("Final Size:      {} bytes", stats.final_size);
    println!("Savings:         {:.1}%", (1.0 - stats.final_size as f32 / stats.original_size as f32) * 100.0);
    println!("Patterns Found:  {}", stats.patterns_learned);
    println!("Note:            Smooth progression = highly predictable!");
}

fn demo_realworld_data() {
    println!("\n📊 Demo 4: Real-World CSV Row (Customer Data)");
    println!("──────────────────────────────────────────────");
    
    // Simulated customer data row
    let data = b"john.doe@example.com,active,2024-05-28,5432.10,USA,Premium,john.doe@example.com,active,2024-05-28";
    let mut cahp = CAHPCompressor::new();
    
    let (compressed, stats) = cahp.compress(data);
    
    println!("Original:        {} bytes", stats.original_size);
    println!("After Predict:   {} bytes", stats.after_prediction);
    println!("Final Size:      {} bytes", stats.final_size);
    println!("Savings:         {:.1}%", (1.0 - stats.final_size as f32 / stats.original_size as f32) * 100.0);
    println!("Patterns Found:  {}", stats.patterns_learned);
    println!("Substitutions:   {}", stats.substitutions_made);
    println!("Prediction Acc:  {:.1}%", stats.prediction_accuracy * 100.0);
    println!("\n💡 Tip: Duplicate fields (email, status) = patterns found!");
}
