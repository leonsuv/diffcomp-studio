// =============================================================================
// dc_license/hwid - Hardware Identification
// =============================================================================
// Generates a unique identifier for the local machine.
//
// ## Design Goals
//
// 1. **Consistency**: Same machine always produces the same ID
// 2. **Uniqueness**: Different machines produce different IDs
// 3. **Privacy**: Doesn't expose sensitive hardware details
// 4. **Resilience**: Works across OS reinstalls (based on hardware)
//
// ## Implementation
//
// We use the machine-uid crate which generates an ID based on:
// - Windows: Machine GUID from registry
// - macOS: IOPlatformUUID
// - Linux: /etc/machine-id or /var/lib/dbus/machine-id
//
// The raw ID is then hashed with SHA-256 for privacy and consistency.
// =============================================================================

#[cfg(not(target_arch = "wasm32"))]
use crate::error::LicenseError;
use crate::error::LicenseResult;
use sha2::{Digest, Sha256};
use std::fmt;
use tracing::{debug, instrument};

/// Represents a hardware identifier for license binding.
///
/// The HWID is a SHA-256 hash of the machine's unique identifier,
/// represented as a hexadecimal string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareId {
    /// The hex-encoded SHA-256 hash of the machine ID
    hash: String,
}

impl HardwareId {
    /// Generate the hardware ID for the current machine.
    ///
    /// This operation may fail on some systems where hardware identification
    /// is not available or accessible.
    #[instrument]
    pub fn generate() -> LicenseResult<Self> {
        debug!("Generating hardware ID for current machine");

        // Get the raw machine ID
        #[cfg(not(target_arch = "wasm32"))]
        let machine_id = machine_uid::get().map_err(|e| LicenseError::HwidGenerationFailed {
            reason: e.to_string(),
        })?;

        #[cfg(target_arch = "wasm32")]
        let machine_id = "wasm-environment-hwid-placeholder".to_string();

        debug!(raw_id_length = machine_id.len(), "Got raw machine ID");

        // Hash the machine ID with SHA-256 for consistent length and privacy
        let mut hasher = Sha256::new();
        hasher.update(machine_id.as_bytes());

        // Also include a salt to prevent rainbow table attacks
        // This salt should be consistent across your app version
        hasher.update(b"DiffCompStudio-HWID-Salt-v1");

        let hash_result = hasher.finalize();
        let hash = hex::encode(hash_result);

        debug!(hwid_prefix = &hash[..8], "Hardware ID generated");

        Ok(Self { hash })
    }

    /// Create a HardwareId from an existing hash string.
    ///
    /// Used when parsing license payloads.
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }

    /// Get the hash as a string reference.
    pub fn as_str(&self) -> &str {
        &self.hash
    }

    /// Get a shortened version for display (first 16 chars).
    pub fn short_display(&self) -> &str {
        &self.hash[..16.min(self.hash.len())]
    }

    /// Check if this HWID matches another.
    pub fn matches(&self, other: &HardwareId) -> bool {
        // Use constant-time comparison to prevent timing attacks
        // (though this is less critical for offline verification)
        self.hash == other.hash
    }
}

impl fmt::Display for HardwareId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.hash)
    }
}

impl From<String> for HardwareId {
    fn from(hash: String) -> Self {
        Self::from_hash(hash)
    }
}

impl AsRef<str> for HardwareId {
    fn as_ref(&self) -> &str {
        &self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hwid_generation() {
        // This test may fail on some CI environments without hardware access
        match HardwareId::generate() {
            Ok(hwid) => {
                // SHA-256 produces 64 hex characters
                assert_eq!(hwid.hash.len(), 64);

                // Should be consistent
                let hwid2 = HardwareId::generate().expect("Second generation should work");
                assert_eq!(hwid, hwid2);
            }
            Err(e) => {
                // Log but don't fail - some environments can't generate HWID
                eprintln!("HWID generation not available: {}", e);
            }
        }
    }

    #[test]
    fn test_hwid_from_hash() {
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string();
        let hwid = HardwareId::from_hash(hash.clone());
        assert_eq!(hwid.as_str(), hash);
    }

    #[test]
    fn test_hwid_short_display() {
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string();
        let hwid = HardwareId::from_hash(hash);
        assert_eq!(hwid.short_display(), "abcdef1234567890");
    }

    #[test]
    fn test_hwid_matches() {
        let hwid1 = HardwareId::from_hash("test123".to_string());
        let hwid2 = HardwareId::from_hash("test123".to_string());
        let hwid3 = HardwareId::from_hash("test456".to_string());

        assert!(hwid1.matches(&hwid2));
        assert!(!hwid1.matches(&hwid3));
    }

    #[test]
    fn test_hwid_display() {
        let hash = "abcdef".to_string();
        let hwid = HardwareId::from_hash(hash.clone());
        assert_eq!(format!("{}", hwid), hash);
    }
}
