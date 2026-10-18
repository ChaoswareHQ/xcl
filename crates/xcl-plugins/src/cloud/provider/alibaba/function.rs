//! Alibaba FC — serverless function operations using unified `function.*` naming.
//!
//! NOTE: Alibaba function registrations are disabled to avoid conflicting with
//! other providers (Cloudflare, AWS, GCP) that also register `function.*` functions.
//! When Alibaba FC support is implemented, these should be re-enabled with
//! provider-specific dispatch logic.
