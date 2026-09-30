//! DiffComp Studio License Key Generator
//!
//! # ⚠️ WARNING: INTERNAL USE ONLY ⚠️
//!
//! This tool contains the PRIVATE SIGNING KEY and must NEVER be:
//! - Committed to public repositories
//! - Distributed with the application
//! - Shared with unauthorized personnel
//!
//! ## Usage
//!
//! Generate a new keypair:
//! ```bash
//! dc-keygen generate-keypair
//! ```
//!
//! Create a license key:
//! ```bash
//! dc-keygen create \
//!     --hwid "abc123def456" \
//!     --licensee "ACME Engineering" \
//!     --expiry "2025-12-31" \
//!     --features pro,batch,export
//! ```
//!
//! Verify a license key:
//! ```bash
//! dc-keygen verify --key "BASE64_LICENSE_KEY"
//! ```

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{NaiveDate, Utc};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// =============================================================================
// CLI Structure
// =============================================================================

#[derive(Parser)]
#[command(
    name = "dc-keygen",
    about = "DiffComp Studio License Key Generator",
    version,
    author
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new Ed25519 keypair
    GenerateKeypair {
        /// Output directory for keypair files
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
    },

    /// Create a signed license key
    Create {
        /// Hardware ID to bind the license to
        #[arg(long)]
        hwid: String,

        /// Licensee name (company or individual)
        #[arg(long)]
        licensee: String,

        /// License expiry date (YYYY-MM-DD)
        #[arg(long)]
        expiry: String,

        /// Feature flags (comma-separated: basic,pro,enterprise,batch,export,cloud,api)
        #[arg(long, default_value = "basic")]
        features: String,

        /// Path to private key file
        #[arg(long, default_value = "private.key")]
        private_key: PathBuf,
    },

    /// Verify a license key using the public key
    Verify {
        /// The Base64-encoded license key
        #[arg(long)]
        key: String,

        /// Path to public key file
        #[arg(long, default_value = "public.key")]
        public_key: PathBuf,
    },

    /// Generate a demo/trial license (30-day, basic features)
    Demo {
        /// Hardware ID to bind the license to
        #[arg(long)]
        hwid: String,

        /// Path to private key file
        #[arg(long, default_value = "private.key")]
        private_key: PathBuf,
    },
}

// =============================================================================
// License Structures (must match dc_license::payload exactly!)
// =============================================================================

/// Feature flags available in licenses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureFlag {
    Basic,
    Pro,
    Enterprise,
    Batch,
    Export,
    Cloud,
    Api,
}

impl std::str::FromStr for FeatureFlag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "basic" => Ok(FeatureFlag::Basic),
            "pro" => Ok(FeatureFlag::Pro),
            "enterprise" => Ok(FeatureFlag::Enterprise),
            "batch" => Ok(FeatureFlag::Batch),
            "export" => Ok(FeatureFlag::Export),
            "cloud" => Ok(FeatureFlag::Cloud),
            "api" => Ok(FeatureFlag::Api),
            _ => Err(anyhow!("Unknown feature flag: {}", s)),
        }
    }
}

/// License payload that gets signed - MUST MATCH dc_license::payload::LicensePayload
#[derive(Debug, Serialize, Deserialize)]
pub struct LicensePayload {
    /// Payload format version (for forward compatibility)
    pub version: u32,
    /// SHA-256 hash of the licensed machine's hardware ID
    pub hardware_id: String,
    /// Timestamp when the license was issued (ISO 8601)
    pub issued_at: chrono::DateTime<Utc>,
    /// Timestamp when the license expires (None = perpetual)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<chrono::DateTime<Utc>>,
    /// Name of the licensee (person or company)
    pub licensee: String,
    /// Email address of the licensee
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Licensed features
    pub features: Vec<FeatureFlag>,
    /// Maximum number of seats (for enterprise licenses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_seats: Option<u32>,
    /// Internal notes (not displayed to user)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

// =============================================================================
// Main Entry Point
// =============================================================================

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::GenerateKeypair { output } => generate_keypair(&output),
        Command::Create {
            hwid,
            licensee,
            expiry,
            features,
            private_key,
        } => create_license(&hwid, &licensee, &expiry, &features, &private_key),
        Command::Verify { key, public_key } => verify_license(&key, &public_key),
        Command::Demo { hwid, private_key } => create_demo_license(&hwid, &private_key),
    }
}

// =============================================================================
// Command Implementations
// =============================================================================

/// Generate a new Ed25519 keypair and save to files
fn generate_keypair(output: &PathBuf) -> Result<()> {
    println!("🔐 Generating new Ed25519 keypair...\n");

    // Generate keypair using OS random number generator
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    // Get raw bytes
    let private_bytes = signing_key.to_bytes();
    let public_bytes = verifying_key.to_bytes();

    // Create output directory if needed
    fs::create_dir_all(output).context("Failed to create output directory")?;

    // Save private key (KEEP SECRET!)
    let private_path = output.join("private.key");
    fs::write(&private_path, private_bytes).context("Failed to write private key")?;
    println!("✅ Private key saved to: {}", private_path.display());
    println!("   ⚠️  KEEP THIS FILE SECRET! Never commit or share it!\n");

    // Save public key (embed this in dc_license)
    let public_path = output.join("public.key");
    fs::write(&public_path, public_bytes).context("Failed to write public key")?;
    println!("✅ Public key saved to: {}", public_path.display());

    // Print public key as Rust array for embedding
    println!("\n📋 Public key for embedding in dc_license/src/verifier.rs:");
    println!("─────────────────────────────────────────────────────────");
    print!("const PUBLIC_KEY: [u8; 32] = [");
    for (i, byte) in public_bytes.iter().enumerate() {
        if i % 8 == 0 {
            print!("\n    ");
        }
        print!("0x{:02x}, ", byte);
    }
    println!("\n];");
    println!("─────────────────────────────────────────────────────────\n");

    // Print hex and base64 versions too
    println!("📋 Public key (hex): {}", hex::encode(public_bytes));
    println!("📋 Public key (base64): {}", BASE64.encode(public_bytes));

    Ok(())
}

/// Create a signed license key
fn create_license(
    hwid: &str,
    licensee: &str,
    expiry: &str,
    features_str: &str,
    private_key_path: &PathBuf,
) -> Result<()> {
    println!("🔑 Creating license key...\n");

    // Load private key
    let private_bytes = fs::read(private_key_path).context("Failed to read private key file")?;

    let private_array: [u8; 32] = private_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid private key length"))?;

    let signing_key = SigningKey::from_bytes(&private_array);

    // Parse expiry date
    let expiry_date = NaiveDate::parse_from_str(expiry, "%Y-%m-%d")
        .context("Invalid expiry date format (expected YYYY-MM-DD)")?;

    let expires_at_dt = expiry_date
        .and_hms_opt(23, 59, 59)
        .ok_or_else(|| anyhow!("Invalid date"))?
        .and_utc();

    // Parse feature flags
    let features: Vec<FeatureFlag> = features_str
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<Result<Vec<_>>>()?;

    // Build payload - must match dc_license::payload::LicensePayload exactly!
    let payload = LicensePayload {
        version: 1,
        hardware_id: hwid.to_string(),
        issued_at: Utc::now(),
        expires_at: Some(expires_at_dt),
        licensee: licensee.to_string(),
        email: None,
        features,
        max_seats: None,
        notes: None,
    };

    // Serialize payload to JSON bytes
    let payload_json = serde_json::to_vec(&payload)?;

    // Sign the payload JSON bytes
    let signature = signing_key.sign(&payload_json);
    let signature_bytes = signature.to_bytes();

    // Create license key: signature (64 bytes) || payload_json
    // Then base64 encode the whole thing
    let mut license_bytes = Vec::with_capacity(64 + payload_json.len());
    license_bytes.extend_from_slice(&signature_bytes);
    license_bytes.extend_from_slice(&payload_json);

    let license_key = BASE64.encode(&license_bytes);

    // Print license details
    println!("📋 License Details:");
    println!("─────────────────────────────────────────────────────────");
    println!("   Licensee:  {}", licensee);
    println!("   HWID:      {}", hwid);
    println!("   Expires:   {}", expiry);
    println!("   Features:  {}", features_str);
    println!("─────────────────────────────────────────────────────────\n");

    println!("🔐 License Key (Base64):");
    println!("─────────────────────────────────────────────────────────");
    println!("{}", license_key);
    println!("─────────────────────────────────────────────────────────\n");

    // Also save to file
    let output_filename = format!("license_{}.key", hwid.chars().take(8).collect::<String>());
    fs::write(&output_filename, &license_key)?;
    println!("✅ License key saved to: {}", output_filename);

    Ok(())
}

/// Verify a license key
fn verify_license(license_key: &str, public_key_path: &PathBuf) -> Result<()> {
    println!("🔍 Verifying license key...\n");

    // Load public key
    let public_bytes = fs::read(public_key_path).context("Failed to read public key file")?;

    let public_array: [u8; 32] = public_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid public key length"))?;

    let verifying_key = VerifyingKey::from_bytes(&public_array)
        .map_err(|e| anyhow!("Invalid public key: {}", e))?;

    // Decode license key (base64 -> signature || payload_json)
    let license_bytes = BASE64
        .decode(license_key.trim())
        .context("Invalid Base64 encoding")?;

    if license_bytes.len() < 65 {
        return Err(anyhow!("License key too short"));
    }

    // Extract signature (first 64 bytes) and payload (rest)
    let signature_bytes: [u8; 64] = license_bytes[..64]
        .try_into()
        .map_err(|_| anyhow!("Invalid signature length"))?;
    let payload_bytes = &license_bytes[64..];

    let signature = ed25519_dalek::Signature::from_bytes(&signature_bytes);

    // Verify signature
    match verifying_key.verify_strict(payload_bytes, &signature) {
        Ok(()) => {
            println!("✅ Signature VALID\n");

            // Parse the payload
            let payload: LicensePayload =
                serde_json::from_slice(payload_bytes).context("Invalid license JSON structure")?;

            println!("📋 License Details:");
            println!("─────────────────────────────────────────────────────────");
            println!("   Licensee:  {}", payload.licensee);
            println!("   HWID:      {}", payload.hardware_id);

            if let Some(expires_at) = payload.expires_at {
                println!("   Expires:   {}", expires_at.format("%Y-%m-%d"));

                // Check if expired
                if Utc::now() > expires_at {
                    println!("   Status:    ⚠️ EXPIRED");
                } else {
                    let days_remaining = (expires_at - Utc::now()).num_days();
                    println!("   Status:    ✅ Valid ({} days remaining)", days_remaining);
                }
            } else {
                println!("   Expires:   NEVER (perpetual)");
                println!("   Status:    ✅ Valid (perpetual)");
            }

            println!("   Features:  {:?}", payload.features);
            println!("─────────────────────────────────────────────────────────");
        }
        Err(e) => {
            println!("❌ Signature INVALID: {}", e);
            return Err(anyhow!("License signature verification failed"));
        }
    }

    Ok(())
}

/// Create a demo/trial license (30 days, basic features only)
fn create_demo_license(hwid: &str, private_key_path: &PathBuf) -> Result<()> {
    let expiry = (Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();

    create_license(hwid, "Demo/Trial User", &expiry, "basic", private_key_path)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation_and_signing() -> Result<()> {
        // Generate keypair
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        // Create a test payload
        let payload = LicensePayload {
            version: 1,
            hardware_id: "test-hwid-12345".to_string(),
            issued_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::days(365)),
            licensee: "Test Company".to_string(),
            email: None,
            features: vec![FeatureFlag::Pro, FeatureFlag::Batch],
            max_seats: None,
            notes: None,
        };

        // Sign
        let payload_json = serde_json::to_vec(&payload)?;
        let signature = signing_key.sign(&payload_json);

        // Verify
        assert!(verifying_key
            .verify_strict(&payload_json, &signature)
            .is_ok());

        Ok(())
    }

    #[test]
    fn test_feature_flag_parsing() {
        assert_eq!("pro".parse::<FeatureFlag>().unwrap(), FeatureFlag::Pro);
        assert_eq!("BATCH".parse::<FeatureFlag>().unwrap(), FeatureFlag::Batch);
        assert!("invalid".parse::<FeatureFlag>().is_err());
    }

    #[test]
    fn test_license_format_roundtrip() -> Result<()> {
        // Generate keypair
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        // Create payload
        let payload = LicensePayload {
            version: 1,
            hardware_id: "test-hwid".to_string(),
            issued_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::days(30)),
            licensee: "Test".to_string(),
            email: None,
            features: vec![FeatureFlag::Enterprise],
            max_seats: None,
            notes: None,
        };

        // Create license key in the same format as create_license
        let payload_json = serde_json::to_vec(&payload)?;
        let signature = signing_key.sign(&payload_json);

        let mut license_bytes = Vec::with_capacity(64 + payload_json.len());
        license_bytes.extend_from_slice(&signature.to_bytes());
        license_bytes.extend_from_slice(&payload_json);

        let license_key = BASE64.encode(&license_bytes);

        // Verify - same as verify_license
        let decoded = BASE64.decode(&license_key)?;
        let sig_bytes: [u8; 64] = decoded[..64].try_into()?;
        let payload_bytes = &decoded[64..];

        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        assert!(verifying_key.verify_strict(payload_bytes, &sig).is_ok());

        let decoded_payload: LicensePayload = serde_json::from_slice(payload_bytes)?;
        assert_eq!(decoded_payload.hardware_id, "test-hwid");
        assert_eq!(decoded_payload.licensee, "Test");

        Ok(())
    }
}
