//! Markdown title extraction / replacement and filename sanitization.
//!
//! Moved verbatim from `src-tauri/src/refinement.rs` (where they were untested)
//! so they gain unit coverage and can be reused by the import + integration
//! paths without pulling in the Tauri crate.

/// Return the text of the first `# ` heading, if any.
pub fn extract_title(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        if let Some(title) = line.strip_prefix("# ") {
            let title = title.trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}

/// Replace the first `# ` heading with `title`, or insert one (after any
/// frontmatter) if the document has no H1.
pub fn apply_title(markdown: &str, title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return markdown.to_string();
    }

    let mut out = Vec::new();
    let mut replaced = false;

    for line in markdown.lines() {
        if !replaced && line.starts_with("# ") {
            out.push(format!("# {}", title));
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }

    if replaced {
        return out.join("\n");
    }

    if markdown.starts_with("---") {
        let mut lines: Vec<String> = markdown.lines().map(ToString::to_string).collect();
        if let Some(end_idx) = lines.iter().skip(1).position(|line| line == "---") {
            let insert_at = end_idx + 2;
            lines.insert(insert_at, String::new());
            lines.insert(insert_at + 1, format!("# {}", title));
            lines.insert(insert_at + 2, String::new());
            return lines.join("\n");
        }
    }

    format!("# {}\n\n{}", title, markdown.trim_start())
}

/// Strip characters invalid in Windows filenames, trim surrounding dots and
/// whitespace, and cap the length. Never returns an empty string.
pub fn sanitize_filename(title: &str) -> String {
    const MAX_FILENAME_TITLE_CHARS: usize = 120;

    let sanitized = title
        .chars()
        .filter(|c| !r#"\/:*?"<>|"#.contains(*c))
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .chars()
        .take(MAX_FILENAME_TITLE_CHARS)
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if sanitized.is_empty() {
        "Untitled Meeting".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_h1() {
        assert_eq!(
            extract_title("---\nx: 1\n---\n# Hello World\n\nbody"),
            Some("Hello World".to_string())
        );
        assert_eq!(extract_title("no heading here"), None);
        assert_eq!(extract_title("## sub\n# real"), Some("real".to_string()));
    }

    #[test]
    fn apply_title_replaces_existing_h1() {
        let md = "# Old Title\n\nbody";
        assert_eq!(apply_title(md, "New Title"), "# New Title\n\nbody");
    }

    #[test]
    fn apply_title_inserts_after_frontmatter_when_no_h1() {
        let md = "---\nk: v\n---\nbody text";
        let out = apply_title(md, "Fresh");
        assert!(out.contains("---\nk: v\n---"));
        assert!(out.contains("# Fresh"));
        // The heading comes after the closing frontmatter fence.
        let fm_end = out.find("---\n").and_then(|_| out[3..].find("---")).unwrap();
        assert!(out.find("# Fresh").unwrap() > fm_end);
    }

    #[test]
    fn apply_title_prepends_when_no_h1_no_frontmatter() {
        assert_eq!(apply_title("just body", "T"), "# T\n\njust body");
    }

    #[test]
    fn apply_empty_title_is_noop() {
        assert_eq!(apply_title("# Keep", "   "), "# Keep");
    }

    #[test]
    fn sanitize_strips_windows_illegal_chars() {
        assert_eq!(
            sanitize_filename(r#"Q3: Review / Plan? <draft>"#),
            "Q3 Review  Plan draft"
        );
    }

    #[test]
    fn sanitize_empty_falls_back() {
        assert_eq!(sanitize_filename("***"), "Untitled Meeting");
        assert_eq!(sanitize_filename("..."), "Untitled Meeting");
    }

    #[test]
    fn sanitize_caps_length() {
        let long = "a".repeat(500);
        assert_eq!(sanitize_filename(&long).chars().count(), 120);
    }
}
