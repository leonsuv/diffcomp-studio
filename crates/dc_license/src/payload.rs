// =============================================================================
// dc_license/payload - License Payload Structure
// =============================================================================
// Defines the structure of the signed license data.
//
// ## Payload Structure (JSON)
//
// {
//     "hardware_id": "sha256-hash-of-machine-id",
//     "issued_at": "2024-01-15T10:30:00Z",
//     "expires_at": "2025-01-15T10:30:00Z",
//     "licensee": "Company Name",
//     "features": ["pro", "batch", "api"],
//     "version": 1
// }
//
// ## Versioning
//
// The payload version field allows future format changes while maintaining
// backwards compatibility. Verifiers should check this field.
// =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Current payload format version
pub const PAYLOAD_VERSION: u32 = 1;

/// Features that can be licensed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureFlag {
    /// Basic comparison features (included in all licenses)
    Basic,
    /// Professional features: batch processing, advanced diff modes
    Pro,
    /// Enterprise features: API access, scripting
    Enterprise,
    /// Batch processing mode
    Batch,
    /// Export to various formats
    Export,
    /// Cloud sync features
    Cloud,
    /// API/SDK access
    Api,
}

impl FeatureFlag {
    /// Get all available features
    pub fn all() -> &'static [FeatureFlag] {
        &[
            FeatureFlag::Basic,
            FeatureFlag::Pro,
            FeatureFlag::Enterprise,
            FeatureFlag::Batch,
            FeatureFlag::Export,
            FeatureFlag::Cloud,
            FeatureFlag::Api,
        ]
    }

    /// Get human-readable name
    pub fn display_name(&self) -> &'static str {
        match self {
            FeatureFlag::Basic => "Basic",
            FeatureFlag::Pro => "Professional",
            FeatureFlag::Enterprise => "Enterprise",
            FeatureFlag::Batch => "Batch Processing",
            FeatureFlag::Export => "Export",
            FeatureFlag::Cloud => "Cloud Sync",
            FeatureFlag::Api => "API Access",
        }
    }

    /// Get short code for serialization
    pub fn code(&self) -> &'static str {
        match self {
            FeatureFlag::Basic => "basic",
            FeatureFlag::Pro => "pro",
            FeatureFlag::Enterprise => "enterprise",
            FeatureFlag::Batch => "batch",
            FeatureFlag::Export => "export",
            FeatureFlag::Cloud => "cloud",
            FeatureFlag::Api => "api",
        }
    }
}

/// The license payload that gets signed.
///
/// This structure is serialized to JSON, signed with the private key,
/// and then the signature + payload are Base64-encoded to create the
/// final license key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    /// Payload format version (for forward compatibility)
    pub version: u32,

    /// SHA-256 hash of the licensed machine's hardware ID
    pub hardware_id: String,

    /// Timestamp when the license was issued
    pub issued_at: DateTime<Utc>,

    /// Timestamp when the license expires (None = perpetual)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,

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

    /// Custom notes/metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl LicensePayload {
    /// Create a new license payload.
    pub fn new(hardware_id: String, licensee: String, features: Vec<FeatureFlag>) -> Self {
        Self {
            version: PAYLOAD_VERSION,
            hardware_id,
            issued_at: Utc::now(),
            expires_at: None,
            licensee,
            email: None,
            features,
            max_seats: None,
            notes: None,
        }
    }

    /// Builder: Set expiration date.
    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Builder: Set expiration to N days from now.
    pub fn expires_in_days(mut self, days: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::days(days));
        self
    }

    /// Builder: Set email.
    pub fn with_email(mut self, email: String) -> Self {
        self.email = Some(email);
        self
    }

    /// Builder: Set max seats.
    pub fn with_seats(mut self, seats: u32) -> Self {
        self.max_seats = Some(seats);
        self
    }

    /// Builder: Set notes.
    pub fn with_notes(mut self, notes: String) -> Self {
        self.notes = Some(notes);
        self
    }

    /// Check if the license has expired.
    pub fn is_expired(&self) -> bool {
        match &self.expires_at {
            Some(expiry) => Utc::now() > *expiry,
            None => false, // Perpetual license
        }
    }

    /// Check if a specific feature is licensed.
    pub fn has_feature(&self, feature: FeatureFlag) -> bool {
        self.features.contains(&feature)
    }

    /// Get the number of days until expiration (negative if expired).
    pub fn days_until_expiry(&self) -> Option<i64> {
        self.expires_at.map(|expiry| {
            let duration = expiry - Utc::now();
            duration.num_days()
        })
    }

    /// Serialize the payload to JSON bytes.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from JSON bytes.
    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

impl Default for LicensePayload {
    fn default() -> Self {
        Self {
            version: PAYLOAD_VERSION,
            hardware_id: String::new(),
            issued_at: Utc::now(),
            expires_at: None,
            licensee: "Unknown".to_string(),
            email: None,
            features: vec![FeatureFlag::Basic],
            max_seats: None,
            notes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_flag_all() {
        let all = FeatureFlag::all();
        assert!(!all.is_empty());
        assert!(all.contains(&FeatureFlag::Basic));
        assert!(all.contains(&FeatureFlag::Pro));
    }

    #[test]
    fn test_license_payload_new() {
        let payload = LicensePayload::new(
            "hwid123".to_string(),
            "Test Company".to_string(),
            vec![FeatureFlag::Pro],
        );

        assert_eq!(payload.version, PAYLOAD_VERSION);
        assert_eq!(payload.hardware_id, "hwid123");
        assert_eq!(payload.licensee, "Test Company");
        assert!(payload.has_feature(FeatureFlag::Pro));
        assert!(!payload.has_feature(FeatureFlag::Enterprise));
    }

    #[test]
    fn test_license_payload_expiry() {
        // Non-expiring license
        let perpetual = LicensePayload::new(
            "hwid".to_string(),
            "Test".to_string(),
            vec![FeatureFlag::Basic],
        );
        assert!(!perpetual.is_expired());
        assert!(perpetual.days_until_expiry().is_none());

        // Future expiry
        let future = perpetual.clone().expires_in_days(30);
        assert!(!future.is_expired());
        assert!(future.days_until_expiry().unwrap() >= 29);

        // Past expiry (using with_expiry directly)
        let past = LicensePayload::new(
            "hwid".to_string(),
            "Test".to_string(),
            vec![FeatureFlag::Basic],
        )
        .with_expiry(Utc::now() - chrono::Duration::days(1));
        assert!(past.is_expired());
        assert!(past.days_until_expiry().unwrap() < 0);
    }

    #[test]
    fn test_license_payload_serialization() {
        let payload = LicensePayload::new(
            "hwid123".to_string(),
            "Test Company".to_string(),
            vec![FeatureFlag::Pro, FeatureFlag::Batch],
        )
        .with_email("test@example.com".to_string())
        .expires_in_days(365);

        // Serialize
        let json_bytes = payload.to_json_bytes().expect("Serialization should work");

        // Deserialize
        let restored =
            LicensePayload::from_json_bytes(&json_bytes).expect("Deserialization should work");

        assert_eq!(restored.hardware_id, payload.hardware_id);
        assert_eq!(restored.licensee, payload.licensee);
        assert_eq!(restored.features, payload.features);
        assert_eq!(restored.email, payload.email);
    }

    #[test]
    fn test_license_payload_builders() {
        let payload = LicensePayload::new(
            "hwid".to_string(),
            "Test".to_string(),
            vec![FeatureFlag::Basic],
        )
        .with_email("user@example.com".to_string())
        .with_seats(10)
        .with_notes("Special terms apply".to_string())
        .expires_in_days(90);

        assert_eq!(payload.email, Some("user@example.com".to_string()));
        assert_eq!(payload.max_seats, Some(10));
        assert_eq!(payload.notes, Some("Special terms apply".to_string()));
        assert!(payload.expires_at.is_some());
    }
}
