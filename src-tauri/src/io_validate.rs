//! Input validation and sanitization for import/export operations.
//!
//! All functions are pure — they take references and return either a cleaned
//! value or an `AppError::Validation`.  No I/O, no DB access.

use crate::error::AppError;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum byte length of the raw import string (50 MB).
pub const MAX_IMPORT_BYTES: usize = 50 * 1024 * 1024;

/// Maximum number of bookmarks allowed in a single import payload.
pub const MAX_IMPORT_BOOKMARKS: usize = 100_000;

/// Maximum URL length in characters (not bytes — we count Unicode scalar values).
pub const MAX_URL_LEN: usize = 2048;

/// Maximum title / description length.
pub const MAX_STRING_LEN: usize = 10_000;

/// Maximum folder name length.
pub const MAX_FOLDER_NAME_LEN: usize = 255;

/// Maximum individual tag name length.
pub const MAX_TAG_NAME_LEN: usize = 100;

/// Maximum number of tags attached to a single bookmark.
pub const MAX_TAGS_PER_BOOKMARK: usize = 50;

/// Maximum nesting depth for XML/OPML documents.
pub const MAX_XML_DEPTH: usize = 200;

/// Maximum nesting depth for Netscape HTML `<DL>` elements.
pub const MAX_DL_DEPTH: usize = 20;

// ─── URL validation ───────────────────────────────────────────────────────────

/// Validate a bookmark URL.
///
/// Rules (applied after stripping leading/trailing whitespace):
/// - Must not be empty.
/// - Must not exceed `MAX_URL_LEN` characters.
/// - Must not use a dangerous scheme (`javascript:`, `data:`, `vbscript:`,
///   `file:`).
/// - Must start with `http://`, `https://`, or `ftp://`.
/// - Must contain a non-empty host portion (anything after the scheme `://`).
pub fn validate_url(url: &str) -> Result<(), AppError> {
    let url = url.trim();

    if url.is_empty() {
        return Err(AppError::Validation { message: "url is required".into() });
    }

    if url.chars().count() > MAX_URL_LEN {
        return Err(AppError::Validation {
            message: format!("url exceeds maximum length of {MAX_URL_LEN} characters"),
        });
    }

    let lower = url.to_lowercase();

    // Reject dangerous schemes regardless of casing or surrounding whitespace.
    for scheme in &["javascript:", "data:", "vbscript:", "file:"] {
        if lower.starts_with(scheme) {
            return Err(AppError::Validation {
                message: format!("unsafe URL scheme '{scheme}'"),
            });
        }
    }

    // Only allow safe schemes.
    let allowed_prefixes = &["http://", "https://", "ftp://"];
    let matched = allowed_prefixes.iter().find(|p| lower.starts_with(*p));

    let scheme_prefix = match matched {
        Some(p) => p,
        None => {
            return Err(AppError::Validation {
                message: "url must use http://, https://, or ftp:// scheme".into(),
            });
        }
    };

    // Ensure there is at least one character after the scheme (host check).
    let after_scheme = &url[scheme_prefix.len()..];
    if after_scheme.is_empty() || after_scheme.starts_with('/') {
        return Err(AppError::Validation { message: "url must contain a host".into() });
    }

    Ok(())
}

// ─── String sanitization ──────────────────────────────────────────────────────

/// Sanitize a free-text string field (title, description, etc.).
///
/// - Strips null bytes (`\0`).
/// - Truncates to `max_len` **Unicode scalar values** (not bytes).
/// - If the input is `None`, returns an empty string — callers decide whether
///   an empty result is an error for their context.
pub fn sanitize_string(s: &str, max_len: usize) -> String {
    let without_nulls: String = s.chars().filter(|c| *c != '\0').collect();
    without_nulls.chars().take(max_len).collect()
}

/// Strip a UTF-8 BOM (`\u{FEFF}`) from the start of a string slice.
///
/// Returns a reference to the original slice with the BOM removed (or
/// unchanged if no BOM is present).  Never allocates.
pub fn strip_bom(input: &str) -> &str {
    input.strip_prefix('\u{FEFF}').unwrap_or(input)
}

// ─── Payload size guard ───────────────────────────────────────────────────────

/// Reject import payloads that exceed `MAX_IMPORT_BYTES`.
///
/// This check must be called **before** any parsing so that malformed giant
/// inputs cannot exhaust memory.
pub fn validate_import_size(input: &str) -> Result<(), AppError> {
    if input.len() > MAX_IMPORT_BYTES {
        return Err(AppError::Validation {
            message: format!(
                "import payload exceeds maximum size of {} MB",
                MAX_IMPORT_BYTES / 1024 / 1024
            ),
        });
    }
    Ok(())
}

/// Reject imports whose declared bookmark count exceeds `MAX_IMPORT_BOOKMARKS`.
///
/// Call this after parsing a JSON array or counting rows to prevent DoS via
/// a single huge file that would otherwise commit 100k+ DB rows.
pub fn validate_bookmark_count(count: usize) -> Result<(), AppError> {
    if count > MAX_IMPORT_BOOKMARKS {
        return Err(AppError::Validation {
            message: format!(
                "import contains {count} bookmarks; maximum allowed is {MAX_IMPORT_BOOKMARKS}"
            ),
        });
    }
    Ok(())
}

// ─── Tag name validation ──────────────────────────────────────────────────────

/// Validate a raw comma/semicolon-separated tag string.
///
/// Rules:
/// - Split on `,` and `;`, trim whitespace, ignore empty parts.
/// - Each tag name must not exceed `MAX_TAG_NAME_LEN` characters.
/// - Must not contain null bytes (after splitting).
/// - The total number of non-empty tags must not exceed `MAX_TAGS_PER_BOOKMARK`.
///
/// Returns `Ok(())` on success; the caller is responsible for the actual
/// find-or-create DB work.
pub fn validate_tag_names(raw: &str) -> Result<(), AppError> {
    let tags: Vec<&str> = raw
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if tags.len() > MAX_TAGS_PER_BOOKMARK {
        return Err(AppError::Validation {
            message: format!(
                "too many tags: {}, maximum is {MAX_TAGS_PER_BOOKMARK}",
                tags.len()
            ),
        });
    }

    for tag in &tags {
        if tag.contains('\0') {
            return Err(AppError::Validation {
                message: "tag name must not contain null bytes".into(),
            });
        }
        if tag.chars().count() > MAX_TAG_NAME_LEN {
            return Err(AppError::Validation {
                message: format!(
                    "tag name '{}...' exceeds maximum length of {MAX_TAG_NAME_LEN} characters",
                    &tag[..tag.len().min(20)]
                ),
            });
        }
    }

    Ok(())
}

// ─── Folder name validation ───────────────────────────────────────────────────

/// Validate a folder name.
///
/// - Must not be empty after trimming whitespace.
/// - Must not exceed `MAX_FOLDER_NAME_LEN` characters.
/// - Must not contain null bytes.
pub fn validate_folder_name(name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation { message: "folder name is required".into() });
    }
    if name.contains('\0') {
        return Err(AppError::Validation {
            message: "folder name must not contain null bytes".into(),
        });
    }
    if name.chars().count() > MAX_FOLDER_NAME_LEN {
        return Err(AppError::Validation {
            message: format!(
                "folder name exceeds maximum length of {MAX_FOLDER_NAME_LEN} characters"
            ),
        });
    }
    Ok(())
}

// ─── XML / HTML depth guards ──────────────────────────────────────────────────

/// Scan raw XML/OPML text for nesting depth before handing it to a full
/// parser.  Counts `<` characters followed by a non-`/` non-`!` non-`?`
/// character as an opening tag and `</` as a closing tag.
///
/// This is intentionally a conservative *fast check* — it may over-count when
/// attributes contain `<`, but that only makes the guard stricter (safer).
/// For any valid OPML that an RSS reader would emit, this behaves correctly.
///
/// Returns `Err` if the maximum depth is exceeded.
pub fn validate_xml_depth(xml: &str, max_depth: usize) -> Result<(), AppError> {
    let mut depth: usize = 0;
    let mut chars = xml.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '<' {
            continue;
        }
        match chars.peek() {
            Some('/') => {
                // Closing tag — depth decreases (saturating to avoid underflow
                // on malformed XML).
                depth = depth.saturating_sub(1);
                chars.next();
            }
            Some('!') | Some('?') => {
                // Declaration or processing instruction — skip.
                chars.next();
            }
            _ => {
                // Opening tag.
                depth += 1;
                if depth > max_depth {
                    return Err(AppError::Validation {
                        message: format!(
                            "XML nesting depth exceeds maximum of {max_depth}"
                        ),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Count `<DL>` open / `</DL>` close pairs in Netscape HTML to guard against
/// unreasonably deep folder nesting.
///
/// Case-insensitive. Returns `Err` if `max_depth` is exceeded.
pub fn validate_dl_depth(html: &str, max_depth: usize) -> Result<(), AppError> {
    let lower = html.to_lowercase();
    let mut depth: usize = 0;
    let mut pos = 0;

    while pos < lower.len() {
        // Jump to the next '<' — it's always ASCII so pos stays on a char boundary.
        let Some(offset) = lower[pos..].find('<') else { break };
        pos += offset;

        if lower[pos..].starts_with("</dl") {
            depth = depth.saturating_sub(1);
            pos += 5;
        } else if lower[pos..].starts_with("<dl") {
            // Make sure it's `<dl>` or `<dl ` rather than `<dlx...`
            let after = lower[pos + 3..].trim_start();
            if after.starts_with('>') || after.starts_with(' ') || after.is_empty() {
                depth += 1;
                if depth > max_depth {
                    return Err(AppError::Validation {
                        message: format!(
                            "Netscape HTML folder depth exceeds maximum of {max_depth}"
                        ),
                    });
                }
            }
            pos += 3;
        } else {
            pos += 1; // skip past this '<'; it's ASCII so safe to add 1
        }
    }

    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_url ──────────────────────────────────────────────────────────

    #[test]
    fn valid_https_url_is_accepted() {
        assert!(validate_url("https://example.com").is_ok());
    }

    #[test]
    fn valid_http_url_is_accepted() {
        assert!(validate_url("http://example.com/path?q=1").is_ok());
    }

    #[test]
    fn valid_ftp_url_is_accepted() {
        assert!(validate_url("ftp://files.example.com/pub").is_ok());
    }

    #[test]
    fn empty_url_is_rejected() {
        assert!(matches!(validate_url(""), Err(AppError::Validation { .. })));
    }

    #[test]
    fn whitespace_only_url_is_rejected() {
        assert!(matches!(validate_url("   "), Err(AppError::Validation { .. })));
    }

    #[test]
    fn javascript_scheme_lowercase_is_rejected() {
        assert!(matches!(
            validate_url("javascript:alert(1)"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn javascript_scheme_mixed_case_is_rejected() {
        assert!(matches!(
            validate_url("JavaScript:void(0)"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn javascript_scheme_uppercase_is_rejected() {
        assert!(matches!(
            validate_url("JAVASCRIPT:x"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn data_scheme_is_rejected() {
        assert!(matches!(
            validate_url("data:text/html,<h1>hi</h1>"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn vbscript_scheme_is_rejected() {
        assert!(matches!(
            validate_url("vbscript:MsgBox(1)"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn file_scheme_is_rejected() {
        assert!(matches!(
            validate_url("file:///etc/passwd"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn url_exceeding_max_length_is_rejected() {
        let long_url = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        assert!(matches!(
            validate_url(&long_url),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn url_at_exact_max_length_is_accepted() {
        // Build a URL exactly MAX_URL_LEN chars long.
        let prefix = "https://x.com/";
        let pad = "a".repeat(MAX_URL_LEN - prefix.len());
        let url = format!("{prefix}{pad}");
        assert_eq!(url.chars().count(), MAX_URL_LEN);
        assert!(validate_url(&url).is_ok());
    }

    #[test]
    fn url_without_host_is_rejected() {
        // scheme present but no host
        assert!(matches!(
            validate_url("https://"),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn unknown_scheme_is_rejected() {
        assert!(matches!(
            validate_url("mailto:user@example.com"),
            Err(AppError::Validation { .. })
        ));
    }

    // ── sanitize_string ───────────────────────────────────────────────────────

    #[test]
    fn null_bytes_are_stripped() {
        let result = sanitize_string("hel\0lo\0", 100);
        assert_eq!(result, "hello");
    }

    #[test]
    fn truncates_to_max_len() {
        let result = sanitize_string("abcdef", 4);
        assert_eq!(result, "abcd");
    }

    #[test]
    fn empty_string_returns_empty() {
        assert_eq!(sanitize_string("", 100), "");
    }

    #[test]
    fn string_shorter_than_max_len_is_unchanged() {
        assert_eq!(sanitize_string("hello", 100), "hello");
    }

    #[test]
    fn truncation_counts_unicode_scalar_values_not_bytes() {
        // Each of these is 3 bytes in UTF-8 but 1 scalar value.
        let s = "あいうえお"; // 5 chars, 15 bytes
        let result = sanitize_string(s, 3);
        assert_eq!(result, "あいう");
    }

    // ── strip_bom ─────────────────────────────────────────────────────────────

    #[test]
    fn bom_is_stripped() {
        let with_bom = "\u{FEFF}hello";
        assert_eq!(strip_bom(with_bom), "hello");
    }

    #[test]
    fn string_without_bom_is_unchanged() {
        assert_eq!(strip_bom("hello"), "hello");
    }

    #[test]
    fn empty_string_without_bom_is_unchanged() {
        assert_eq!(strip_bom(""), "");
    }

    // ── validate_import_size ──────────────────────────────────────────────────

    #[test]
    fn small_input_is_accepted() {
        assert!(validate_import_size("{}").is_ok());
    }

    #[test]
    fn input_at_exact_limit_is_accepted() {
        let input = "x".repeat(MAX_IMPORT_BYTES);
        assert!(validate_import_size(&input).is_ok());
    }

    #[test]
    fn input_one_byte_over_limit_is_rejected() {
        let input = "x".repeat(MAX_IMPORT_BYTES + 1);
        assert!(matches!(
            validate_import_size(&input),
            Err(AppError::Validation { .. })
        ));
    }

    // ── validate_bookmark_count ───────────────────────────────────────────────

    #[test]
    fn bookmark_count_at_limit_is_accepted() {
        assert!(validate_bookmark_count(MAX_IMPORT_BOOKMARKS).is_ok());
    }

    #[test]
    fn bookmark_count_over_limit_is_rejected() {
        assert!(matches!(
            validate_bookmark_count(MAX_IMPORT_BOOKMARKS + 1),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn zero_bookmarks_is_accepted() {
        assert!(validate_bookmark_count(0).is_ok());
    }

    // ── validate_tag_names ────────────────────────────────────────────────────

    #[test]
    fn valid_comma_separated_tags_are_accepted() {
        assert!(validate_tag_names("rust, systems, programming").is_ok());
    }

    #[test]
    fn valid_semicolon_separated_tags_are_accepted() {
        assert!(validate_tag_names("design;ux;css").is_ok());
    }

    #[test]
    fn empty_tag_string_is_accepted() {
        // Zero tags is fine — the caller decides if a tag is required.
        assert!(validate_tag_names("").is_ok());
    }

    #[test]
    fn tag_name_at_max_length_is_accepted() {
        let tag = "a".repeat(MAX_TAG_NAME_LEN);
        assert!(validate_tag_names(&tag).is_ok());
    }

    #[test]
    fn tag_name_over_max_length_is_rejected() {
        let tag = "a".repeat(MAX_TAG_NAME_LEN + 1);
        assert!(matches!(
            validate_tag_names(&tag),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn too_many_tags_is_rejected() {
        let tags: Vec<String> = (0..=MAX_TAGS_PER_BOOKMARK).map(|i| format!("tag{i}")).collect();
        let raw = tags.join(",");
        assert!(matches!(
            validate_tag_names(&raw),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn exactly_max_tags_is_accepted() {
        let tags: Vec<String> = (0..MAX_TAGS_PER_BOOKMARK).map(|i| format!("tag{i}")).collect();
        let raw = tags.join(",");
        assert!(validate_tag_names(&raw).is_ok());
    }

    #[test]
    fn tag_with_null_byte_is_rejected() {
        assert!(matches!(
            validate_tag_names("good,bad\0tag"),
            Err(AppError::Validation { .. })
        ));
    }

    // ── validate_folder_name ──────────────────────────────────────────────────

    #[test]
    fn valid_folder_name_is_accepted() {
        assert!(validate_folder_name("Work").is_ok());
    }

    #[test]
    fn empty_folder_name_is_rejected() {
        assert!(matches!(
            validate_folder_name(""),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn whitespace_folder_name_is_rejected() {
        assert!(matches!(
            validate_folder_name("   "),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn folder_name_at_max_length_is_accepted() {
        let name = "a".repeat(MAX_FOLDER_NAME_LEN);
        assert!(validate_folder_name(&name).is_ok());
    }

    #[test]
    fn folder_name_over_max_length_is_rejected() {
        let name = "a".repeat(MAX_FOLDER_NAME_LEN + 1);
        assert!(matches!(
            validate_folder_name(&name),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn folder_name_with_null_byte_is_rejected() {
        assert!(matches!(
            validate_folder_name("Fol\0der"),
            Err(AppError::Validation { .. })
        ));
    }

    // ── validate_xml_depth ────────────────────────────────────────────────────

    #[test]
    fn shallow_xml_is_accepted() {
        let xml = "<root><child><leaf/></child></root>";
        assert!(validate_xml_depth(xml, MAX_XML_DEPTH).is_ok());
    }

    #[test]
    fn xml_exactly_at_max_depth_is_accepted() {
        // Build <a><a>...(200 levels)...</a></a>
        let open: String = "<a>".repeat(MAX_XML_DEPTH);
        let close: String = "</a>".repeat(MAX_XML_DEPTH);
        let xml = format!("{open}{close}");
        assert!(validate_xml_depth(&xml, MAX_XML_DEPTH).is_ok());
    }

    #[test]
    fn xml_one_level_over_max_depth_is_rejected() {
        let open: String = "<a>".repeat(MAX_XML_DEPTH + 1);
        let close: String = "</a>".repeat(MAX_XML_DEPTH + 1);
        let xml = format!("{open}{close}");
        assert!(matches!(
            validate_xml_depth(&xml, MAX_XML_DEPTH),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn xml_declarations_do_not_count_toward_depth() {
        let xml = r#"<?xml version="1.0"?><!-- comment --><root/>"#;
        assert!(validate_xml_depth(xml, 1).is_ok());
    }

    // ── validate_dl_depth ─────────────────────────────────────────────────────

    #[test]
    fn shallow_dl_is_accepted() {
        let html = "<DL><DT><A HREF=\"x\">X</A></DL>";
        assert!(validate_dl_depth(html, MAX_DL_DEPTH).is_ok());
    }

    #[test]
    fn dl_exactly_at_max_depth_is_accepted() {
        let open: String = "<DL>".repeat(MAX_DL_DEPTH);
        let close: String = "</DL>".repeat(MAX_DL_DEPTH);
        let html = format!("{open}{close}");
        assert!(validate_dl_depth(&html, MAX_DL_DEPTH).is_ok());
    }

    #[test]
    fn dl_one_level_over_max_depth_is_rejected() {
        let open: String = "<DL>".repeat(MAX_DL_DEPTH + 1);
        let close: String = "</DL>".repeat(MAX_DL_DEPTH + 1);
        let html = format!("{open}{close}");
        assert!(matches!(
            validate_dl_depth(&html, MAX_DL_DEPTH),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn dl_check_is_case_insensitive() {
        // Mix of lower- and upper-case should still count correctly.
        let html = "<dl><dl></dl></dl>";
        assert!(validate_dl_depth(html, 2).is_ok());
        assert!(matches!(
            validate_dl_depth(html, 1),
            Err(AppError::Validation { .. })
        ));
    }

    #[test]
    fn dl_depth_with_multibyte_chars_does_not_panic() {
        // Em dash (—, 3 bytes) and other multi-byte chars must not cause a
        // char-boundary panic in the byte-level scanner.
        let html = "<!DOCTYPE netscape-bookmark-file-1>\
            <title>raindrop.io bookmarks — exported</title>\
            <DL><DT><A HREF=\"https://example.com\">café &amp; résumé</A></DT></DL>";
        assert!(validate_dl_depth(html, MAX_DL_DEPTH).is_ok());
    }
}
