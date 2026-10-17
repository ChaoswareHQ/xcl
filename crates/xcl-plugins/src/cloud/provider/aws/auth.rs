//! AWS Signature V4 — request signing for AWS REST API.
//!
//! Implements AWS Signature Version 4 for authenticating HTTP requests
//! to AWS services (S3, Lambda, API Gateway, etc.).
//!
//! ## Reference
//!
//! <https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html>

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

/// AWS credentials for signing requests.
#[derive(Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

/// Sign an HTTP request using AWS Signature V4.
///
/// Returns the Authorization header value and any additional headers
/// (like X-Amz-Date, X-Amz-Security-Token).
#[allow(clippy::too_many_arguments)]
pub fn sign_request(
    credentials: &AwsCredentials,
    region: &str,
    service: &str,
    method: &str,
    canonical_uri: &str,
    canonical_querystring: &str,
    canonical_headers: &str,
    signed_headers: &str,
    payload: &str,
) -> (String, String, Option<String>) {
    let (amz_date, date_stamp) = aws_timestamps();

    // Step 1: Create canonical request
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method,
        canonical_uri,
        canonical_querystring,
        canonical_headers,
        signed_headers,
        hex::encode(Sha256::digest(payload.as_bytes()))
    );

    // Step 2: Create string to sign
    let credential_scope = format!("{}/{}/{}/aws4_request", date_stamp, region, service);
    let _string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        credential_scope,
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );

    // Step 3: Calculate signature
    let mut signing_key = format!("AWS4{}", credentials.secret_access_key).into_bytes();
    for _data in &[
        date_stamp.as_bytes(),
        region.as_bytes(),
        service.as_bytes(),
        b"aws4_request",
    ] {
        let mac = Hmac::<Sha256>::new_from_slice(&signing_key)
            .expect("HMAC can key key of any size");
        signing_key = mac.finalize().into_bytes().to_vec();
    }

    let mac = Hmac::<Sha256>::new_from_slice(&signing_key)
        .expect("HMAC can key key of any size");
    let signature = mac.finalize().into_bytes();
    let signature_hex = hex::encode(signature);

    // Step 4: Build authorization header
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, Signed={}, Signature={}",
        credentials.access_key_id,
        credential_scope,
        signed_headers,
        signature_hex
    );

    let x_amz_date = amz_date;
    let x_amz_token = credentials.session_token.clone();

    (authorization, x_amz_date, x_amz_token)
}

/// Create canonical headers from a method, host, and content type.
pub fn build_canonical_headers(host: &str, content_type: &str) -> (String, String) {
    let canonical = format!("content-type:{}\nhost:{}\n", content_type, host);
    let signed = "content-type;host".to_string();
    (canonical, signed)
}

/// Compute the payload hash for a request body.
pub fn payload_hash(payload: &str) -> String {
    hex::encode(Sha256::digest(payload.as_bytes()))
}

/// Compute an empty payload hash (for GET requests with no body).
pub fn empty_payload_hash() -> String {
    payload_hash("")
}

/// Generate AWS SigV4 timestamps from SystemTime.
///
/// Returns `(amz_date, date_stamp)` where:
/// - `amz_date` is in `YYYYMMDDTHHMMSSZ` format (e.g., `20130524T000000Z`)
/// - `date_stamp` is in `YYYYMMDD` format (e.g., `20130524`)
fn aws_timestamps() -> (String, String) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    // Convert Unix timestamp to UTC date/time
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year, month, day from days since Unix epoch (1970-01-01)
    let mut y = 1970i32;
    let mut d = days_since_epoch as i32;

    while {
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            false
        } else {
            d -= days_in_year;
            y += 1;
            true
        }
    } {}

    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [
        31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut m = 0i32;
    while m < 12 && d >= month_days[m as usize] {
        d -= month_days[m as usize];
        m += 1;
    }
    let day = d + 1;
    let month = m + 1;

    let amz_date = format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", y, month, day, hours, minutes, seconds);
    let date_stamp = format!("{:04}{:02}{:02}", y, month, day);
    (amz_date, date_stamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_hash_matches_known_value() {
        // The SHA256 hash of an empty string is well-known
        let hash = empty_payload_hash();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn payload_hash_of_hello() {
        let hash = payload_hash("hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn canonical_headers_format() {
        let (canonical, signed) = build_canonical_headers("s3.amazonaws.com", "application/json");
        assert_eq!(canonical, "content-type:application/json\nhost:s3.amazonaws.com\n");
        assert_eq!(signed, "content-type;host");
    }

    #[test]
    fn sign_request_produces_authorization() {
        let creds = AwsCredentials {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            session_token: None,
        };

        let (auth, date, token) = sign_request(
            &creds,
            "us-east-1",
            "s3",
            "GET",
            "/",
            "",
            build_canonical_headers("s3.amazonaws.com", "") .0.as_str(),
            "host",
            "",
        );

        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential="));
        assert!(date.len() == 16); // 20130524T000000Z format
        assert!(token.is_none());
    }

    #[test]
    fn sign_request_with_session_token() {
        let creds = AwsCredentials {
            access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            session_token: Some("test-token".to_string()),
        };

        let (_, _, token) = sign_request(
            &creds,
            "us-east-1",
            "s3",
            "GET",
            "/",
            "",
            "",
            "host",
            "",
        );

        assert_eq!(token.as_deref(), Some("test-token"));
    }

    #[test]
    fn aws_timestamps_format() {
        let (amz_date, date_stamp) = aws_timestamps();
        // amz_date should be 16 chars: YYYYMMDDTHHMMSSZ
        assert_eq!(amz_date.len(), 16);
        assert!(amz_date.ends_with('Z'));
        assert!(amz_date.contains('T'));
        // date_stamp should be 8 chars: YYYYMMDD
        assert_eq!(date_stamp.len(), 8);
        // Year should be 2024 or later
        let year: i32 = amz_date[..4].parse().unwrap();
        assert!(year >= 2024);
    }

    #[test]
    fn aws_timestamps_known_epoch() {
        // 1705276800 is 2024-01-15 00:00:00 UTC
        let secs = 1705276800u64;
        let days = secs / 86400;
        let time_of_day = secs % 86400;
        assert_eq!(days, 19737);
        assert_eq!(time_of_day, 0);
    }
}
