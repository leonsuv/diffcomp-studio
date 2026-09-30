// =============================================================================
// dc_license/error - Error Types for License Operations
// =============================================================================

use thiserror::Error;

/// Errors that can occur during license operations.
#[derive(Error, Debug)]
pub enum LicenseError {
    /// License key has invalid format (not valid Base64, wrong structure)
    #[error("Invalid license key format: {reason}")]
    InvalidFormat {
        /// The reason for the invalid format
        reason: String,
    },

    /// License signature verification failed
    #[error("License signature verification failed")]
    SignatureInvalid,

    /// License is for a different machine
    #[error("License HWID mismatch: expected '{expected}', got '{actual}'")]
    HwidMismatch {
        /// The expected hardware ID
        expected: String,
        /// The actual hardware ID
        actual: String,
    },

    /// License has expired
    #[error("License expired on {expiry}")]
    Expired {
        /// The expiration date string
        expiry: String,
    },

    /// License doesn't include required feature
    #[error("License doesn't include feature: {feature}")]
    FeatureNotLicensed {
        /// The missing feature name
        feature: String,
    },

    /// Failed to generate hardware ID
    #[error("Failed to generate hardware ID: {reason}")]
    HwidGenerationFailed {
        /// The reason for generation failure
        reason: String,
    },

    /// Failed to decode Base64
    #[error("Failed to decode license key: {reason}")]
    Base64DecodeError {
        /// The reason for decoding failure
        reason: String,
    },

    /// Failed to parse JSON payload
    #[error("Failed to parse license payload: {reason}")]
    JsonParseError {
        /// The reason for parsing failure
        reason: String,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error: {reason}")]
    CryptoError {
        /// The reason for cryptographic failure
        reason: String,
    },
}

/// Result type for license operations
pub type LicenseResult<T> = Result<T, LicenseError>;
