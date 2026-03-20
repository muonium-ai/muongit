//! Clean/smudge filter system
//! Parity: libgit2 src/libgit2/filter.c, crlf.c, ident.c

use std::path::Path;

use crate::attributes::{Attributes, AttrValue};
use crate::config::Config;
use crate::oid::OID;

/// Direction of filtering.
/// Parity: git_filter_mode_t
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Working directory → ODB (clean)
    ToOdb,
    /// ODB → working directory (smudge)
    ToWorktree,
}

/// Metadata about the file being filtered.
/// Parity: git_filter_source
#[derive(Debug, Clone)]
pub struct FilterSource {
    pub path: String,
    pub mode: FilterMode,
    pub oid: Option<OID>,
}

/// Result of applying a filter.
#[derive(Debug)]
pub enum FilterResult {
    Applied(Vec<u8>),
    Passthrough,
}

/// A single filter implementation.
pub trait Filter: std::fmt::Debug {
    fn name(&self) -> &str;
    /// Check if this filter applies to the given source.
    /// Returns true if the filter should be applied.
    fn check(&self, source: &FilterSource, attrs: &Attributes) -> bool;
    /// Apply the filter to the input data.
    fn apply(&self, input: &[u8], source: &FilterSource) -> FilterResult;
}

/// A chain of filters to apply to a file.
/// Parity: git_filter_list
#[derive(Debug)]
pub struct FilterList {
    filters: Vec<Box<dyn Filter>>,
    pub source: FilterSource,
}

impl FilterList {
    /// Load the applicable filters for a path in a repository.
    /// Parity: git_filter_list_load
    pub fn load(
        git_dir: &Path,
        workdir: Option<&Path>,
        path: &str,
        mode: FilterMode,
        oid: Option<OID>,
    ) -> Self {
        let attrs = Attributes::load_for_repo(git_dir, workdir);
        let source = FilterSource {
            path: path.to_string(),
            mode,
            oid,
        };
        let mut filters: Vec<Box<dyn Filter>> = Vec::new();

        // Built-in filters in priority order (lower priority first for smudge)
        let crlf = CrlfFilter::new(git_dir);
        let ident = IdentFilter;

        match mode {
            FilterMode::ToWorktree => {
                // Smudge: CRLF(0) → Ident(100)
                if crlf.check(&source, &attrs) {
                    filters.push(Box::new(crlf));
                }
                if ident.check(&source, &attrs) {
                    filters.push(Box::new(ident));
                }
            }
            FilterMode::ToOdb => {
                // Clean: Ident(100) → CRLF(0) (reverse order)
                if ident.check(&source, &attrs) {
                    filters.push(Box::new(ident));
                }
                if crlf.check(&source, &attrs) {
                    filters.push(Box::new(crlf));
                }
            }
        }

        FilterList { filters, source }
    }

    /// Apply all filters in the chain to the input data.
    /// Parity: git_filter_list_apply_to_buffer
    pub fn apply(&self, input: &[u8]) -> Vec<u8> {
        let mut data = input.to_vec();
        for filter in &self.filters {
            match filter.apply(&data, &self.source) {
                FilterResult::Applied(output) => data = output,
                FilterResult::Passthrough => {}
            }
        }
        data
    }

    /// Number of active filters.
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Whether the filter list is empty.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Check if a named filter is in the list.
    pub fn contains(&self, name: &str) -> bool {
        self.filters.iter().any(|f| f.name() == name)
    }
}

// ── CRLF Filter (priority 0) ──
// Parity: libgit2 src/libgit2/crlf.c

/// Resolved CRLF action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CrlfAction {
    None,
    CrlfToLf,
    LfToCrlf,
    Auto,
}

/// End-of-line style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EolStyle {
    Lf,
    Crlf,
    Native,
}

/// CRLF / text / eol filter.
#[derive(Debug)]
pub struct CrlfFilter {
    auto_crlf: Option<String>,
    core_eol: Option<String>,
}

impl CrlfFilter {
    pub fn new(git_dir: &Path) -> Self {
        let config_path = git_dir.join("config");
        let config = Config::load(&config_path).unwrap_or_else(|_| Config::new());
        let auto_crlf = config.get("core", "autocrlf").map(|s| s.to_string());
        let core_eol = config.get("core", "eol").map(|s| s.to_string());
        CrlfFilter { auto_crlf, core_eol }
    }

    fn resolve_action(&self, attrs: &Attributes, path: &str, mode: FilterMode) -> CrlfAction {
        let text_attr = attrs.get(path, "text");
        let crlf_attr = attrs.get(path, "crlf");
        let eol_attr = attrs.get(path, "eol");

        // text attribute takes priority
        let is_text = match text_attr {
            Some(AttrValue::Set) => Some(true),
            Some(AttrValue::Unset) => Some(false),
            Some(AttrValue::Value(v)) if v == "auto" => {
                return CrlfAction::Auto;
            }
            _ => None,
        };

        // Fall back to crlf attribute
        let is_text = is_text.or_else(|| match crlf_attr {
            Some(AttrValue::Set) => Some(true),
            Some(AttrValue::Unset) => Some(false),
            _ => None,
        });

        // Fall back to core.autocrlf config
        let is_text = is_text.or_else(|| match self.auto_crlf.as_deref() {
            Some("true") => Some(true),
            Some("input") => Some(true),
            _ => None,
        });

        let is_text = match is_text {
            Some(true) => true,
            Some(false) => return CrlfAction::None,
            None => return CrlfAction::None,
        };

        if !is_text {
            return CrlfAction::None;
        }

        // Determine output eol
        let output_eol = self.resolve_eol(eol_attr, mode);

        match mode {
            FilterMode::ToOdb => CrlfAction::CrlfToLf,
            FilterMode::ToWorktree => match output_eol {
                EolStyle::Crlf => CrlfAction::LfToCrlf,
                EolStyle::Lf => CrlfAction::None,
                EolStyle::Native => {
                    if cfg!(windows) {
                        CrlfAction::LfToCrlf
                    } else {
                        CrlfAction::None
                    }
                }
            },
        }
    }

    fn resolve_eol(&self, eol_attr: Option<&AttrValue>, mode: FilterMode) -> EolStyle {
        // eol attribute overrides everything
        if let Some(AttrValue::Value(v)) = eol_attr {
            return match v.as_str() {
                "lf" => EolStyle::Lf,
                "crlf" => EolStyle::Crlf,
                _ => EolStyle::Native,
            };
        }

        // core.autocrlf=input means always LF on clean
        if mode == FilterMode::ToOdb {
            if let Some("input") = self.auto_crlf.as_deref() {
                return EolStyle::Lf;
            }
        }

        // core.eol config
        match self.core_eol.as_deref() {
            Some("lf") => EolStyle::Lf,
            Some("crlf") => EolStyle::Crlf,
            _ => EolStyle::Native,
        }
    }
}

impl Filter for CrlfFilter {
    fn name(&self) -> &str {
        "crlf"
    }

    fn check(&self, source: &FilterSource, attrs: &Attributes) -> bool {
        let action = self.resolve_action(attrs, &source.path, source.mode);
        action != CrlfAction::None
    }

    fn apply(&self, input: &[u8], source: &FilterSource) -> FilterResult {
        // Skip binary content
        if is_binary(input) {
            return FilterResult::Passthrough;
        }

        match source.mode {
            FilterMode::ToOdb => {
                // Clean: CRLF → LF
                crlf_to_lf(input)
            }
            FilterMode::ToWorktree => {
                // Smudge: LF → CRLF (only if needed)
                lf_to_crlf(input)
            }
        }
    }
}

/// Convert CRLF to LF (clean direction).
fn crlf_to_lf(input: &[u8]) -> FilterResult {
    if !input.windows(2).any(|w| w == b"\r\n") {
        return FilterResult::Passthrough;
    }

    let mut output = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if i + 1 < input.len() && input[i] == b'\r' && input[i + 1] == b'\n' {
            output.push(b'\n');
            i += 2;
        } else {
            output.push(input[i]);
            i += 1;
        }
    }
    FilterResult::Applied(output)
}

/// Convert LF to CRLF (smudge direction).
fn lf_to_crlf(input: &[u8]) -> FilterResult {
    // Check if there's anything to do
    let has_bare_lf = input.windows(1).enumerate().any(|(i, w)| {
        w[0] == b'\n' && (i == 0 || input[i - 1] != b'\r')
    });

    if !has_bare_lf {
        return FilterResult::Passthrough;
    }

    let mut output = Vec::with_capacity(input.len() + input.len() / 10);
    for (i, &byte) in input.iter().enumerate() {
        if byte == b'\n' && (i == 0 || input[i - 1] != b'\r') {
            output.push(b'\r');
        }
        output.push(byte);
    }
    FilterResult::Applied(output)
}

/// Simple binary detection: check for NUL bytes in the first 8000 bytes.
/// Parity: libgit2 src/libgit2/filter.h GIT_FILTER_BYTES_TO_CHECK_NUL
fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8000);
    data[..check_len].contains(&0)
}

// ── Ident Filter (priority 100) ──
// Parity: libgit2 src/libgit2/ident.c

/// $Id$ expansion/contraction filter.
#[derive(Debug)]
pub struct IdentFilter;

impl Filter for IdentFilter {
    fn name(&self) -> &str {
        "ident"
    }

    fn check(&self, source: &FilterSource, attrs: &Attributes) -> bool {
        matches!(attrs.get(&source.path, "ident"), Some(AttrValue::Set))
    }

    fn apply(&self, input: &[u8], source: &FilterSource) -> FilterResult {
        if is_binary(input) {
            return FilterResult::Passthrough;
        }

        match source.mode {
            FilterMode::ToWorktree => ident_smudge(input, source.oid.as_ref()),
            FilterMode::ToOdb => ident_clean(input),
        }
    }
}

/// Smudge: Replace `$Id$` with `$Id: <hex> $`
fn ident_smudge(input: &[u8], oid: Option<&OID>) -> FilterResult {
    let oid = match oid {
        Some(o) => o,
        None => return FilterResult::Passthrough,
    };

    let needle = b"$Id$";
    let replacement = format!("$Id: {} $", oid.hex());

    if let Some(output) = replace_bytes(input, needle, replacement.as_bytes()) {
        FilterResult::Applied(output)
    } else {
        FilterResult::Passthrough
    }
}

/// Clean: Replace `$Id: <anything> $` back to `$Id$`
fn ident_clean(input: &[u8]) -> FilterResult {
    let start_marker = b"$Id:";
    let end_marker = b"$";

    let input_str = match std::str::from_utf8(input) {
        Ok(s) => s,
        Err(_) => return FilterResult::Passthrough,
    };

    if !input_str.contains("$Id:") {
        return FilterResult::Passthrough;
    }

    let mut output = String::with_capacity(input_str.len());
    let mut remaining = input_str;

    while let Some(start_pos) = remaining.find("$Id:") {
        output.push_str(&remaining[..start_pos]);

        let after_start = &remaining[start_pos + start_marker.len()..];
        if let Some(end_pos) = after_start.find(std::str::from_utf8(end_marker).unwrap()) {
            // Check that the content between $Id: and $ doesn't contain newlines
            let content = &after_start[..end_pos];
            if !content.contains('\n') {
                output.push_str("$Id$");
                remaining = &after_start[end_pos + 1..];
            } else {
                output.push_str("$Id:");
                remaining = after_start;
            }
        } else {
            output.push_str(&remaining[start_pos..]);
            remaining = "";
            break;
        }
    }

    output.push_str(remaining);

    if output.as_bytes() == input {
        FilterResult::Passthrough
    } else {
        FilterResult::Applied(output.into_bytes())
    }
}

/// Replace all occurrences of `needle` in `haystack` with `replacement`.
fn replace_bytes(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Option<Vec<u8>> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }

    let mut found = false;
    let mut output = Vec::with_capacity(haystack.len());
    let mut i = 0;

    while i <= haystack.len() - needle.len() {
        if &haystack[i..i + needle.len()] == needle {
            output.extend_from_slice(replacement);
            i += needle.len();
            found = true;
        } else {
            output.push(haystack[i]);
            i += 1;
        }
    }

    // Append remaining bytes
    output.extend_from_slice(&haystack[i..]);

    if found {
        Some(output)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn test_tmp(name: &str) -> PathBuf {
        let tmp = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp").join(name);
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join(".git/info")).unwrap();
        fs::create_dir_all(tmp.join(".git/objects")).unwrap();
        fs::create_dir_all(tmp.join(".git/refs")).unwrap();
        tmp
    }

    #[test]
    fn test_crlf_to_lf_clean() {
        let result = crlf_to_lf(b"hello\r\nworld\r\n");
        match result {
            FilterResult::Applied(data) => assert_eq!(data, b"hello\nworld\n"),
            FilterResult::Passthrough => panic!("expected Applied"),
        }
    }

    #[test]
    fn test_crlf_to_lf_no_crlf() {
        let result = crlf_to_lf(b"hello\nworld\n");
        assert!(matches!(result, FilterResult::Passthrough));
    }

    #[test]
    fn test_lf_to_crlf_smudge() {
        let result = lf_to_crlf(b"hello\nworld\n");
        match result {
            FilterResult::Applied(data) => assert_eq!(data, b"hello\r\nworld\r\n"),
            FilterResult::Passthrough => panic!("expected Applied"),
        }
    }

    #[test]
    fn test_lf_to_crlf_already_crlf() {
        let result = lf_to_crlf(b"hello\r\nworld\r\n");
        assert!(matches!(result, FilterResult::Passthrough));
    }

    #[test]
    fn test_ident_smudge() {
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let source = FilterSource {
            path: "test.txt".to_string(),
            mode: FilterMode::ToWorktree,
            oid: Some(oid.clone()),
        };
        let result = IdentFilter.apply(b"Version: $Id$\n", &source);
        match result {
            FilterResult::Applied(data) => {
                let s = std::str::from_utf8(&data).unwrap();
                assert!(s.contains("$Id: aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d $"));
            }
            FilterResult::Passthrough => panic!("expected Applied"),
        }
    }

    #[test]
    fn test_ident_clean() {
        let source = FilterSource {
            path: "test.txt".to_string(),
            mode: FilterMode::ToOdb,
            oid: None,
        };
        let input = b"Version: $Id: aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d $\n";
        let result = IdentFilter.apply(input, &source);
        match result {
            FilterResult::Applied(data) => {
                assert_eq!(data, b"Version: $Id$\n");
            }
            FilterResult::Passthrough => panic!("expected Applied"),
        }
    }

    #[test]
    fn test_ident_no_marker() {
        let source = FilterSource {
            path: "test.txt".to_string(),
            mode: FilterMode::ToWorktree,
            oid: Some(OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap()),
        };
        let result = IdentFilter.apply(b"no markers here\n", &source);
        assert!(matches!(result, FilterResult::Passthrough));
    }

    #[test]
    fn test_binary_detection() {
        assert!(is_binary(b"hello\x00world"));
        assert!(!is_binary(b"hello world"));
    }

    #[test]
    fn test_binary_skipped_by_crlf() {
        let source = FilterSource {
            path: "test.bin".to_string(),
            mode: FilterMode::ToOdb,
            oid: None,
        };
        let input = b"hello\r\n\x00world";
        let crlf = CrlfFilter {
            auto_crlf: None,
            core_eol: None,
        };
        let result = crlf.apply(input, &source);
        assert!(matches!(result, FilterResult::Passthrough));
    }

    #[test]
    fn test_filter_list_load_text_file() {
        let tmp = test_tmp("filter_list_text");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        // Create .gitattributes with text attribute
        fs::write(workdir.join(".gitattributes"), "*.txt text\n").unwrap();

        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "hello.txt",
            FilterMode::ToOdb,
            None,
        );
        assert!(list.contains("crlf"));
    }

    #[test]
    fn test_filter_list_load_binary_file() {
        let tmp = test_tmp("filter_list_binary");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        fs::write(workdir.join(".gitattributes"), "*.bin binary\n").unwrap();

        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "image.bin",
            FilterMode::ToOdb,
            None,
        );
        assert!(list.is_empty());
    }

    #[test]
    fn test_filter_list_load_ident() {
        let tmp = test_tmp("filter_list_ident");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        fs::write(workdir.join(".gitattributes"), "*.c ident\n").unwrap();

        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "main.c",
            FilterMode::ToWorktree,
            Some(OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap()),
        );
        assert!(list.contains("ident"));
    }

    #[test]
    fn test_filter_list_apply_clean() {
        let tmp = test_tmp("filter_list_apply_clean");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        fs::write(workdir.join(".gitattributes"), "*.txt text ident\n").unwrap();

        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "readme.txt",
            FilterMode::ToOdb,
            None,
        );

        let input = b"Version: $Id: abc123 $\r\nHello\r\n";
        let output = list.apply(input);

        // Both ident clean ($Id: ... $ → $Id$) and CRLF clean (CRLF → LF)
        assert_eq!(output, b"Version: $Id$\nHello\n");
    }

    #[test]
    fn test_filter_list_apply_smudge() {
        let tmp = test_tmp("filter_list_apply_smudge");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        fs::write(
            workdir.join(".gitattributes"),
            "*.txt text eol=crlf ident\n",
        )
        .unwrap();

        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "readme.txt",
            FilterMode::ToWorktree,
            Some(oid),
        );

        let input = b"Version: $Id$\nHello\n";
        let output = list.apply(input);

        // CRLF smudge (LF → CRLF) + ident smudge ($Id$ → $Id: hex $)
        let s = std::str::from_utf8(&output).unwrap();
        assert!(s.contains("$Id: aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d $"));
        assert!(s.contains("\r\n"));
    }

    #[test]
    fn test_eol_lf_attribute() {
        let tmp = test_tmp("filter_eol_lf");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        fs::write(workdir.join(".gitattributes"), "*.txt text eol=lf\n").unwrap();

        // On smudge with eol=lf, no CRLF conversion
        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "readme.txt",
            FilterMode::ToWorktree,
            None,
        );
        // eol=lf means no LF→CRLF on smudge
        assert!(!list.contains("crlf"));
    }

    #[test]
    fn test_autocrlf_config() {
        let tmp = test_tmp("filter_autocrlf");
        let workdir = &tmp;
        let git_dir = tmp.join(".git");

        // Set core.autocrlf=true in config
        fs::write(
            git_dir.join("config"),
            "[core]\n\tautocrlf = true\n",
        )
        .unwrap();
        // No gitattributes needed — autocrlf applies globally

        let list = FilterList::load(
            &git_dir,
            Some(workdir),
            "readme.txt",
            FilterMode::ToOdb,
            None,
        );
        assert!(list.contains("crlf"));
    }

    #[test]
    fn test_empty_filter_list() {
        let tmp = test_tmp("filter_empty");
        let git_dir = tmp.join(".git");

        // No .gitattributes, no config
        let list = FilterList::load(&git_dir, Some(&tmp), "test.txt", FilterMode::ToOdb, None);
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);

        // Apply should return input unchanged
        let input = b"hello world\n";
        assert_eq!(list.apply(input), input);
    }

    #[test]
    fn test_multiple_ident_markers() {
        let input = b"$Id$ and $Id$ again";
        let oid = OID::from_hex("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d").unwrap();
        let result = ident_smudge(input, Some(&oid));
        match result {
            FilterResult::Applied(data) => {
                let s = std::str::from_utf8(&data).unwrap();
                // Both markers should be expanded
                assert_eq!(
                    s.matches("$Id: aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d $").count(),
                    2
                );
            }
            FilterResult::Passthrough => panic!("expected Applied"),
        }
    }

    #[test]
    fn test_ident_clean_multiple() {
        let input = b"$Id: abc $ and $Id: def $";
        let result = ident_clean(input);
        match result {
            FilterResult::Applied(data) => {
                assert_eq!(data, b"$Id$ and $Id$");
            }
            FilterResult::Passthrough => panic!("expected Applied"),
        }
    }
}
