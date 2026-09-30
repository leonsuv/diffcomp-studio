// =============================================================================
// dc_license/verifier - License Verification Engine
// =============================================================================
// The core verification logic using Ed25519 signatures.
//
// ## License Key Format
//
// The license key is a Base64-encoded string containing:
//
// ┌──────────────────────────────────────────────────────────────────────────┐
// │  Signature (64 bytes)  │  JSON Payload (variable length)                 │
// └──────────────────────────────────────────────────────────────────────────┘
//
// 1. First 64 bytes: Ed25519 signature of the payload
// 2. Remaining bytes: JSON-encoded LicensePayload
//
// ## Verification Steps
//
// 1. Base64 decode the key
// 2. Split into signature (64 bytes) and payload
// 3. Verify signature using public key
// 4. Parse JSON payload
// 5. Check hardware ID matches
// 6. Check expiration
// 7. Return licensed features
//
// ## Security Considerations
//
// - The public key is embedded in the binary
// - Signature verification is done before any payload parsing
// - Hardware ID binding prevents license sharing
// - Expiration checking uses system time (can be bypassed by clock manipulation,
//   but that's acceptable for offline licensing)
// =============================================================================

use crate::error::{LicenseError, LicenseResult};
use crate::hwid::HardwareId;
use crate::payload::{FeatureFlag, LicensePayload};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use tracing::{debug, info, instrument, warn};

/// Size of Ed25519 signature in bytes
const SIGNATURE_SIZE: usize = 64;

/// Size of Ed25519 public key in bytes
const PUBLIC_KEY_SIZE: usize = 32;

// =============================================================================
// IMPORTANT: Replace this with your actual public key!
// =============================================================================
// Production public key - generated 2026-02-01
// Keep the private key SECURE and NEVER include it in the app
// =============================================================================
const PUBLIC_KEY_BYTES: [u8; PUBLIC_KEY_SIZE] = [
    0x0c, 0x02, 0x29, 0xf7, 0x82, 0x51, 0x0a, 0xd4, 0x10, 0xc6, 0x6f, 0xc8, 0xa9, 0x14, 0xcc, 0x68,
    0x22, 0x0f, 0x2b, 0x3d, 0xac, 0x3b, 0x30, 0x60, 0xb2, 0xba, 0x20, 0xad, 0xe3, 0x9d, 0x0a, 0xe4,
];

/// The result of a successful license verification.
#[derive(Debug, Clone)]
pub struct LicenseStatus {
    /// The verified license payload
    pub payload: LicensePayload,

    /// Whether the license is currently valid (not expired, HWID matches)
    pub is_valid: bool,

    /// Days remaining until expiration (None if perpetual)
    pub days_remaining: Option<i64>,

    /// Licensed features
    pub features: Vec<FeatureFlag>,

    /// Licensee name
    pub licensee: String,

    /// Warning messages (e.g., "License expires soon")
    pub warnings: Vec<String>,
}

impl LicenseStatus {
    /// Check if a specific feature is available.
    pub fn has_feature(&self, feature: FeatureFlag) -> bool {
        self.is_valid && self.features.contains(&feature)
    }

    /// Get a human-readable summary.
    pub fn summary(&self) -> String {
        if !self.is_valid {
            return "License is invalid".to_string();
        }

        match self.days_remaining {
            Some(days) if days < 0 => format!("License expired {} days ago", -days),
            Some(days) if days == 0 => "License expires today".to_string(),
            Some(days) if days == 1 => "License expires tomorrow".to_string(),
            Some(days) if days <= 30 => format!("License expires in {} days", days),
            Some(days) => format!("License valid for {} more days", days),
            None => "Perpetual license".to_string(),
        }
    }
}

/// License verifier using Ed25519 signatures.
///
/// This struct holds the public key and provides methods for verifying
/// license keys and checking the local hardware ID.
pub struct LicenseVerifier {
    /// The Ed25519 public key for signature verification
    public_key: VerifyingKey,

    /// Cached local hardware ID
    local_hwid: Option<HardwareId>,
}

impl LicenseVerifier {
    /// Create a new verifier with the embedded public key.
    ///
    /// # Panics
    ///
    /// Panics if the embedded public key is invalid (should never happen
    /// if the key was generated correctly).
    pub fn new() -> Self {
        let public_key = VerifyingKey::from_bytes(&PUBLIC_KEY_BYTES)
            .expect("Embedded public key is invalid - this is a build error");

        Self {
            public_key,
            local_hwid: None,
        }
    }

    /// Create a verifier with a custom public key (for testing).
    pub fn with_public_key(public_key_bytes: &[u8; PUBLIC_KEY_SIZE]) -> LicenseResult<Self> {
        let public_key =
            VerifyingKey::from_bytes(public_key_bytes).map_err(|e| LicenseError::CryptoError {
                reason: format!("Invalid public key: {}", e),
            })?;

        Ok(Self {
            public_key,
            local_hwid: None,
        })
    }

    /// Get or generate the local hardware ID.
    pub fn get_local_hwid(&mut self) -> LicenseResult<&HardwareId> {
        if self.local_hwid.is_none() {
            self.local_hwid = Some(HardwareId::generate()?);
        }
        Ok(self.local_hwid.as_ref().unwrap())
    }

    /// Get the local HWID as a string (for display to user).
    pub fn get_local_hwid_string(&mut self) -> LicenseResult<String> {
        Ok(self.get_local_hwid()?.to_string())
    }

    /// Verify a license key and return the status.
    ///
    /// # Arguments
    /// * `license_key` - The Base64-encoded license key string
    ///
    /// # Returns
    /// * `Ok(LicenseStatus)` - Verification succeeded (check `is_valid` for status)
    /// * `Err(LicenseError)` - Verification failed completely
    #[instrument(skip(self, license_key))]
    pub fn verify(&mut self, license_key: &str) -> LicenseResult<LicenseStatus> {
        info!("Verifying license key");

        // Step 1: Base64 decode
        let decoded = self.decode_license_key(license_key)?;
        debug!(decoded_length = decoded.len(), "License key decoded");

        // Step 2: Extract signature and payload
        if decoded.len() < SIGNATURE_SIZE + 1 {
            return Err(LicenseError::InvalidFormat {
                reason: format!(
                    "License key too short: {} bytes (minimum: {})",
                    decoded.len(),
                    SIGNATURE_SIZE + 1
                ),
            });
        }

        let signature_bytes = &decoded[..SIGNATURE_SIZE];
        let payload_bytes = &decoded[SIGNATURE_SIZE..];

        // Step 3: Verify signature BEFORE parsing payload
        // This is important for security - we don't want to parse untrusted data
        let signature =
            Signature::from_slice(signature_bytes).map_err(|e| LicenseError::CryptoError {
                reason: format!("Invalid signature format: {}", e),
            })?;

        self.public_key
            .verify(payload_bytes, &signature)
            .map_err(|_| LicenseError::SignatureInvalid)?;

        debug!("Signature verified successfully");

        // Step 4: Parse the payload (now safe since signature is valid)
        let payload = LicensePayload::from_json_bytes(payload_bytes).map_err(|e| {
            LicenseError::JsonParseError {
                reason: e.to_string(),
            }
        })?;

        debug!(
            licensee = %payload.licensee,
            features = ?payload.features,
            "Payload parsed"
        );

        // Step 5: Validate HWID
        let local_hwid = self.get_local_hwid()?;
        let license_hwid = HardwareId::from_hash(payload.hardware_id.clone());

        let hwid_valid = local_hwid.matches(&license_hwid);
        if !hwid_valid {
            warn!(
                local = local_hwid.short_display(),
                license = license_hwid.short_display(),
                "Hardware ID mismatch"
            );
        }

        // Step 6: Check expiration
        let is_expired = payload.is_expired();
        if is_expired {
            warn!(
                expiry = ?payload.expires_at,
                "License has expired"
            );
        }

        // Step 7: Build result
        let days_remaining = payload.days_until_expiry();
        let is_valid = hwid_valid && !is_expired;

        let mut warnings = Vec::new();
        if let Some(days) = days_remaining {
            if days <= 30 && days > 0 {
                warnings.push(format!("License expires in {} days", days));
            }
        }
        if !hwid_valid {
            warnings.push("License is for a different machine".to_string());
        }

        info!(
            is_valid,
            hwid_valid, is_expired, "License verification complete"
        );

        Ok(LicenseStatus {
            payload: payload.clone(),
            is_valid,
            days_remaining,
            features: payload.features.clone(),
            licensee: payload.licensee.clone(),
            warnings,
        })
    }

    /// Verify that a license key has valid signature (without HWID check).
    ///
    /// Useful for validating keys before deploying to target machine.
    pub fn verify_signature_only(&self, license_key: &str) -> LicenseResult<LicensePayload> {
        let decoded = self.decode_license_key(license_key)?;

        if decoded.len() < SIGNATURE_SIZE + 1 {
            return Err(LicenseError::InvalidFormat {
                reason: "License key too short".to_string(),
            });
        }

        let signature_bytes = &decoded[..SIGNATURE_SIZE];
        let payload_bytes = &decoded[SIGNATURE_SIZE..];

        let signature =
            Signature::from_slice(signature_bytes).map_err(|e| LicenseError::CryptoError {
                reason: format!("Invalid signature format: {}", e),
            })?;

        self.public_key
            .verify(payload_bytes, &signature)
            .map_err(|_| LicenseError::SignatureInvalid)?;

        LicensePayload::from_json_bytes(payload_bytes).map_err(|e| LicenseError::JsonParseError {
            reason: e.to_string(),
        })
    }

    /// Decode a Base64 license key to bytes.
    fn decode_license_key(&self, license_key: &str) -> LicenseResult<Vec<u8>> {
        // Remove whitespace that might have been added for readability
        let cleaned: String = license_key.chars().filter(|c| !c.is_whitespace()).collect();

        BASE64
            .decode(&cleaned)
            .map_err(|e| LicenseError::Base64DecodeError {
                reason: e.to_string(),
            })
    }
}

impl Default for LicenseVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if the embedded public key has been configured.
/// Returns false if it's still the placeholder zeros.
#[allow(dead_code)]
pub fn is_public_key_configured() -> bool {
    PUBLIC_KEY_BYTES.iter().any(|&b| b != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    /// Helper to create a test keypair and sign a payload
    fn create_test_license(payload: &LicensePayload) -> (String, [u8; PUBLIC_KEY_SIZE]) {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();

        let payload_bytes = payload.to_json_bytes().unwrap();
        let signature = signing_key.sign(&payload_bytes);

        let mut combined = Vec::with_capacity(SIGNATURE_SIZE + payload_bytes.len());
        combined.extend_from_slice(&signature.to_bytes());
        combined.extend_from_slice(&payload_bytes);

        let license_key = BASE64.encode(&combined);
        let public_key_bytes = verifying_key.to_bytes();

        (license_key, public_key_bytes)
    }

    #[test]
    fn test_verify_valid_license() {
        let hwid = HardwareId::generate()
            .unwrap_or_else(|_| HardwareId::from_hash("test-hwid-for-ci".to_string()));

        let payload = LicensePayload::new(
            hwid.to_string(),
            "Test User".to_string(),
            vec![FeatureFlag::Pro],
        )
        .expires_in_days(365);

        let (license_key, public_key) = create_test_license(&payload);

        let mut verifier = LicenseVerifier::with_public_key(&public_key).unwrap();

        // Override the cached HWID to match the payload
        verifier.local_hwid = Some(hwid);

        let status = verifier.verify(&license_key).unwrap();

        assert!(status.is_valid);
        assert!(status.has_feature(FeatureFlag::Pro));
        assert_eq!(status.licensee, "Test User");
    }

    #[test]
    fn test_verify_expired_license() {
        let hwid = HardwareId::from_hash("test-hwid".to_string());

        let payload = LicensePayload::new(
            hwid.to_string(),
            "Test User".to_string(),
            vec![FeatureFlag::Basic],
        )
        .with_expiry(chrono::Utc::now() - chrono::Duration::days(1));

        let (license_key, public_key) = create_test_license(&payload);

        let mut verifier = LicenseVerifier::with_public_key(&public_key).unwrap();
        verifier.local_hwid = Some(hwid);

        let status = verifier.verify(&license_key).unwrap();

        assert!(!status.is_valid); // Expired
        assert!(status.days_remaining.unwrap() < 0);
    }

    #[test]
    fn test_verify_wrong_hwid() {
        let payload = LicensePayload::new(
            "different-hwid".to_string(),
            "Test User".to_string(),
            vec![FeatureFlag::Basic],
        );

        let (license_key, public_key) = create_test_license(&payload);

        let mut verifier = LicenseVerifier::with_public_key(&public_key).unwrap();
        verifier.local_hwid = Some(HardwareId::from_hash("local-hwid".to_string()));

        let status = verifier.verify(&license_key).unwrap();

        assert!(!status.is_valid); // Wrong HWID
        assert!(!status.warnings.is_empty());
    }

    #[test]
    fn test_verify_invalid_signature() {
        // Create a license with one keypair
        let payload = LicensePayload::new(
            "hwid".to_string(),
            "Test".to_string(),
            vec![FeatureFlag::Basic],
        );
        let (license_key, _) = create_test_license(&payload);

        // Try to verify with a different public key
        let mut rng = OsRng;
        let different_key = SigningKey::generate(&mut rng);
        let wrong_public_key = different_key.verifying_key().to_bytes();

        let mut verifier = LicenseVerifier::with_public_key(&wrong_public_key).unwrap();

        let result = verifier.verify(&license_key);
        assert!(matches!(result, Err(LicenseError::SignatureInvalid)));
    }

    #[test]
    fn test_verify_malformed_key() {
        let mut verifier = LicenseVerifier::new();

        // Not valid Base64
        let result = verifier.verify("not-valid-base64!!!");
        assert!(matches!(
            result,
            Err(LicenseError::Base64DecodeError { .. })
        ));

        // Valid Base64 but too short
        let result = verifier.verify(&BASE64.encode(b"short"));
        assert!(matches!(result, Err(LicenseError::InvalidFormat { .. })));
    }

    #[test]
    fn test_license_status_summary() {
        let status = LicenseStatus {
            payload: LicensePayload::default(),
            is_valid: true,
            days_remaining: Some(15),
            features: vec![FeatureFlag::Pro],
            licensee: "Test".to_string(),
            warnings: vec![],
        };

        let summary = status.summary();
        assert!(summary.contains("15 days"));
    }

    #[test]
    fn test_is_public_key_configured() {
        // With actual key, should return true
        assert!(is_public_key_configured());
    }
}
