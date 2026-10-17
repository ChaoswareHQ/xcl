//! Cloud provider implementations.
//!
//! Each submodule is a provider (AWS, GCP, Azure, etc.). New providers
//! follow the same structure: `provider/<name>/mod.rs` with domain wrapper,
//! then service submodules (s3, lambda, ec2, etc.).
//!
//! ## Adding a new provider
//!
//! 1. Create `provider/<name>/mod.rs` with the domain wrapper
//! 2. Add service modules (storage, compute, networking, etc.)
//! 3. Register in this file: `pub mod <name>;`

pub mod alibaba;
pub mod aws;
pub mod azure;
pub mod cloudflare;
pub mod gcp;
pub mod github;
pub mod ibm;
pub mod oci;
