// =============================================================================
// dc_license - License Verification Module
// =============================================================================
// Provides cryptographic license verification for DiffComp Studio.
//
// ## Security Model
//
// This module implements an OFFLINE license verification system using
// Ed25519 digital signatures. The workflow is:
//
// 1. **Key Generation** (done by vendor, NEVER distributed):
//    - Generate Ed25519 keypair (private + public)
//    - Store private key in secure keygen tool
//    - Embed public key in application binary
//
// 2. **License Generation** (done by vendor):
//    - Collect customer's Hardware ID (HWID)
//    - Create license payload: { hwid, expiry, features }
//    - Sign payload with private key
//    - Send signed license key to customer
//
// 3. **License Verification** (done by application):
//    - Decode the license key (Base64)
//    - Verify signature using embedded public key
//    - Check HWID matches local machine
//    - Check expiry hasn't passed
//    - Enable licensed features
//
// ## Why Ed25519?
//
// - Fast: Signature verification is ~70,000 ops/second
// - Secure: 128-bit security level, resistant to known attacks
// - Small: 64-byte signatures, 32-byte public keys
// - Deterministic: Same input always produces same output
// - No padding oracles or side-channel vulnerabilities
//
// ## IMPORTANT SECURITY NOTES
//
// 1. The PUBLIC_KEY in this file is just a placeholder!
//    Replace it with your actual public key before distribution.
//
// 2. NEVER include the private key in application code.
//
// 3. Hardware ID generation should be consistent across app restarts
//    but unique per machine.
// =============================================================================

#![deny(unsafe_code)]
#![warn(missing_docs)]

//! # dc_license - License Verification Library
//!
//! This crate provides offline license verification using Ed25519 signatures.
//!
//! ## Quick Start
//!
//! ```ignore
//! use dc_license::{LicenseVerifier, LicenseStatus};
//!
//! let verifier = LicenseVerifier::new();
//! match verifier.verify("YOUR-LICENSE-KEY") {
//!     Ok(status) => {
//!         if status.is_valid() {
//!             println!("License valid! Features: {:?}", status.features);
//!         }
//!     }
//!     Err(e) => println!("License verification failed: {}", e),
//! }
//! ```

mod error;
mod hwid;
mod payload;
mod verifier;

pub use error::{LicenseError, LicenseResult};
pub use hwid::HardwareId;
pub use payload::{FeatureFlag, LicensePayload};
pub use verifier::{LicenseStatus, LicenseVerifier};

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_exists() {
        assert!(!VERSION.is_empty());
    }
}
