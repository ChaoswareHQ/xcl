//! Alibaba OSS — object storage operations using unified `bucket.*` naming.
//!
//! NOTE: Alibaba bucket registrations are disabled to avoid conflicting with
//! other providers (Cloudflare, AWS, GCP) that also register `bucket.*` functions.
//! When Alibaba OSS support is implemented, these should be re-enabled with
//! provider-specific dispatch logic.
