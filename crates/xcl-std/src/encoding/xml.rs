//! Simple XML parsing utilities.
//!
//! Provides lightweight XML value extraction for parsing configuration files
//! like Maven pom.xml without requiring a full XML parser dependency.
//!
//! # Usage
//!
//! ```ignore
//! use xcl_std::encoding::xml;
//!
//! let value = xml::extract_value("<groupId>test</groupId>", "groupId");
//! assert_eq!(value, Some("test".to_string()));
//! ```

/// Extract the text content from a simple XML element on a single line.
///
/// # Arguments
/// - `line` — A single line of XML text
/// - `tag` — The tag name to extract (e.g., "groupId")
///
/// # Returns
/// The trimmed text content if the tag matches, otherwise `None`.
///
/// # Examples
///
/// ```
/// use xcl_std::encoding::xml::extract_value;
///
/// assert_eq!(
///     extract_value("<groupId>com.example</groupId>", "groupId"),
///     Some("com.example".to_string())
/// );
/// assert_eq!(
///     extract_value("<artifactId>test</artifactId>", "groupId"),
///     None
/// );
/// ```
pub fn extract_value(line: &str, tag: &str) -> Option<String> {
    let trimmed = line.trim();
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    if trimmed.starts_with(&open) && trimmed.ends_with(&close) {
        let value = &trimmed[open.len()..trimmed.len() - close.len()];
        Some(value.trim().to_string())
    } else {
        None
    }
}

/// Parse a simple XML document and extract all values for a given tag.
///
/// Scans all lines and collects values for matching tags.
///
/// # Examples
///
/// ```
/// use xcl_std::encoding::xml::extract_all;
///
/// let xml = r#"<root>
///   <item>first</item>
///   <item>second</item>
/// </root>"#;
///
/// let items = extract_all(xml, "item");
/// assert_eq!(items, vec!["first".to_string(), "second".to_string()]);
/// ```
pub fn extract_all(content: &str, tag: &str) -> Vec<String> {
    content.lines()
        .filter_map(|line| extract_value(line.trim(), tag))
        .collect()
}

/// Parse a block of XML elements between open/close tags.
///
/// Returns the lines between the opening and closing tags.
///
/// # Examples
///
/// ```
/// use xcl_std::encoding::xml::extract_block;
///
/// let xml = r#"<dependencies>
///   <dep>foo</dep>
/// </dependencies>"#;
///
/// let block = extract_block(xml, "dependencies").unwrap();
/// assert!(block.contains("foo"));
/// ```
pub fn extract_block(content: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    let mut start = None;
    let mut end = None;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed == open && start.is_none() {
            start = Some(i);
        }
        if trimmed == close {
            end = Some(i);
            break;
        }
    }

    match (start, end) {
        (Some(s), Some(e)) => {
            let lines: Vec<&str> = content.lines().skip(s + 1).take(e - s - 1).collect();
            Some(lines.join("\n"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_value() {
        assert_eq!(
            extract_value("<groupId>com.example</groupId>", "groupId"),
            Some("com.example".to_string())
        );
        assert_eq!(
            extract_value("<artifactId>test</artifactId>", "groupId"),
            None
        );
    }

    #[test]
    fn test_extract_value_with_whitespace() {
        assert_eq!(
            extract_value("    <version>1.0</version>  ", "version"),
            Some("1.0".to_string())
        );
    }

    #[test]
    fn test_extract_all() {
        let xml = "<root>\n<item>first</item>\n<item>second</item>\n</root>";
        let items = extract_all(xml, "item");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], "first");
        assert_eq!(items[1], "second");
    }

    #[test]
    fn test_extract_block() {
        let xml = "<deps>\n  <dep>foo</dep>\n</deps>";
        let block = extract_block(xml, "deps").unwrap();
        assert!(block.contains("foo"));
    }

    #[test]
    fn test_extract_block_missing() {
        let xml = "<other>content</other>";
        assert!(extract_block(xml, "missing").is_none());
    }
}
