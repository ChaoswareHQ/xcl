//! Alibaba ECS — compute instance operations using unified `instance.*` naming.
//!
//! NOTE: Alibaba instance registrations are disabled to avoid conflicting with
//! other providers (Cloudflare, AWS, GCP) that also register `instance.*` functions.
//! When Alibaba ECS support is implemented, these should be re-enabled with
//! provider-specific dispatch logic.
