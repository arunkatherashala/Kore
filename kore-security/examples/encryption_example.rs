//! AES-256 Encryption Example
//!
//! Demonstrates end-to-end encryption with key derivation

use kore_security::encryption::{EncryptionKey, EncryptionCipher, AesGcmCipher};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kore Encryption Example ===\n");

    // Example 1: Random key generation
    println!("Example 1: Random Key Generation");
    random_key_example().await?;

    // Example 2: Password-derived key
    println!("\nExample 2: Password-Derived Key");
    password_key_example().await?;

    // Example 3: Encryption with additional authenticated data
    println!("\nExample 3: Encryption with AAD");
    aad_encryption_example().await?;

    println!("\n✓ Encryption examples completed");
    Ok(())
}

async fn random_key_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  Generating random encryption key...");
    let key = EncryptionKey::generate_random()?;
    println!("  Key material: {} bytes", key.key_material.len());
    println!("  Salt: {} bytes", key.salt.len());

    let cipher = AesGcmCipher::new(key)?;

    let plaintext = b"Sensitive data that needs protection";
    println!("  Plaintext: {:?}", std::str::from_utf8(plaintext)?);

    let ciphertext = cipher.encrypt(plaintext).await?;
    println!("  Ciphertext: {} bytes (encrypted)", ciphertext.len());

    let decrypted = cipher.decrypt(&ciphertext).await?;
    println!("  Decrypted: {:?}", std::str::from_utf8(&decrypted)?);

    assert_eq!(plaintext, decrypted.as_slice());
    println!("  ✓ Encryption/decryption successful");

    Ok(())
}

async fn password_key_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  Deriving key from password...");
    let password = "MySecurePassword123!";

    let key = EncryptionKey::derive_from_password(password)?;
    println!("  Key derived using Argon2");
    println!("  Key material: {} bytes", key.key_material.len());

    let cipher = AesGcmCipher::new(key)?;

    let data = b"User confidential information";
    let encrypted = cipher.encrypt(data).await?;
    let decrypted = cipher.decrypt(&encrypted).await?;

    assert_eq!(data, decrypted.as_slice());
    println!("  ✓ Password-based encryption works");

    Ok(())
}

async fn aad_encryption_example() -> Result<(), Box<dyn std::error::Error>> {
    println!("  Encrypting with additional authenticated data...");

    let key = EncryptionKey::generate_random()?;
    let cipher = AesGcmCipher::new(key)?;

    let plaintext = b"Secret content";
    let aad = b"metadata:user:123:file:secret.txt"; // Public metadata

    println!("  Plaintext: {:?}", std::str::from_utf8(plaintext)?);
    println!("  AAD: {:?}", std::str::from_utf8(aad)?);

    let ciphertext = cipher
        .encrypt_with_aad(plaintext, aad)
        .await?;

    println!("  Ciphertext: {} bytes", ciphertext.len());

    // Successful decryption with correct AAD
    let decrypted = cipher
        .decrypt_with_aad(&ciphertext, aad)
        .await?;

    assert_eq!(plaintext, decrypted.as_slice());
    println!("  ✓ Decryption with correct AAD successful");

    // Try with wrong AAD - should fail
    let wrong_aad = b"metadata:user:456:file:other.txt";
    let result = cipher.decrypt_with_aad(&ciphertext, wrong_aad).await;

    match result {
        Err(_) => println!("  ✓ Decryption with wrong AAD failed (expected)"),
        Ok(_) => println!("  ✗ ERROR: Should have failed with wrong AAD"),
    }

    Ok(())
}
