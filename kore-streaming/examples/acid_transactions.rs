//! ACID Transactions Example
//!
//! Demonstrates ACID properties with snapshot isolation

use kore_streaming::acid::{AcidWriter, AcidReader, ChangeType, InMemoryAcidStore};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore ACID Transactions Example ===\n");

    let store = InMemoryAcidStore::new();

    // Example 1: Simple insert transaction
    println!("Example 1: Insert Transaction");
    insert_transaction(&store).await?;

    // Example 2: Update transaction
    println!("\nExample 2: Update Transaction");
    update_transaction(&store).await?;

    // Example 3: Delete transaction
    println!("\nExample 3: Delete Transaction");
    delete_transaction(&store).await?;

    // Example 4: Concurrent transactions
    println!("\nExample 4: Concurrent Transactions");
    concurrent_transactions(&store).await?;

    // Example 5: Transaction rollback
    println!("\nExample 5: Transaction Rollback");
    rollback_transaction(&store).await?;

    println!("\n✓ ACID examples completed");
    Ok(())
}

async fn insert_transaction(store: &InMemoryAcidStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Starting insert transaction...");

    let txn_id = store.begin_transaction().await?;
    println!("  Transaction ID: {}", txn_id.as_u64());

    // Insert multiple records
    store
        .write(txn_id, ChangeType::Insert, b"Record 1".to_vec())
        .await?;
    store
        .write(txn_id, ChangeType::Insert, b"Record 2".to_vec())
        .await?;
    store
        .write(txn_id, ChangeType::Insert, b"Record 3".to_vec())
        .await?;

    let version = store.commit(txn_id).await?;
    println!("  Committed at version: {}", version);
    println!("  Current version: {}", store.current_version());

    Ok(())
}

async fn update_transaction(store: &InMemoryAcidStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Starting update transaction...");

    let txn_id = store.begin_transaction().await?;

    // Update records (in real system, would read first)
    store
        .write(txn_id, ChangeType::Update, b"Updated Record 1".to_vec())
        .await?;
    store
        .write(txn_id, ChangeType::Update, b"Updated Record 2".to_vec())
        .await?;

    let version = store.commit(txn_id).await?;
    println!("  Updated 2 records, committed at version: {}", version);

    Ok(())
}

async fn delete_transaction(store: &InMemoryAcidStore) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Starting delete transaction...");

    let txn_id = store.begin_transaction().await?;

    // Mark records as deleted (tombstones)
    store
        .write(txn_id, ChangeType::Delete, Vec::new())
        .await?;

    let version = store.commit(txn_id).await?;
    println!("  Deleted 1 record, committed at version: {}", version);

    Ok(())
}

async fn concurrent_transactions(
    store: &InMemoryAcidStore,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Starting concurrent transactions...");

    // Start two transactions
    let txn1 = store.begin_transaction().await?;
    let txn2 = store.begin_transaction().await?;

    println!("  Transaction 1 ID: {}", txn1.as_u64());
    println!("  Transaction 2 ID: {}", txn2.as_u64());

    // Write in parallel (simulated)
    store
        .write(txn1, ChangeType::Insert, b"TXN1 Record".to_vec())
        .await?;
    store
        .write(txn2, ChangeType::Insert, b"TXN2 Record".to_vec())
        .await?;

    // Commit both
    let v1 = store.commit(txn1).await?;
    let v2 = store.commit(txn2).await?;

    println!("  Transaction 1 committed at version: {}", v1);
    println!("  Transaction 2 committed at version: {}", v2);
    println!("  Final version: {}", store.current_version());

    // Snapshot isolation: each transaction saw consistent state
    println!("  Both transactions were isolated and committed successfully");

    Ok(())
}

async fn rollback_transaction(
    store: &InMemoryAcidStore,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Starting transaction (will rollback)...");

    let txn_id = store.begin_transaction().await?;

    store
        .write(txn_id, ChangeType::Insert, b"This will be rolled back".to_vec())
        .await?;

    println!("  Rolling back transaction...");
    store.abort(txn_id).await?;

    println!("  Transaction aborted");
    println!("  Final version (unchanged): {}", store.current_version());

    Ok(())
}
