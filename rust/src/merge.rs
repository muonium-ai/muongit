//! Three-way merge with conflict detection
//! Parity: libgit2 src/libgit2/merge.c

/// A region in the merge result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRegion {
    /// Lines from base that neither side changed.
    Clean(Vec<String>),
    /// Lines changed by one or both sides without conflict.
    Resolved(Vec<String>),
    /// Conflicting changes from both sides.
    Conflict {
        base: Vec<String>,
        ours: Vec<String>,
        theirs: Vec<String>,
    },
}

/// Result of a three-way merge.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub regions: Vec<MergeRegion>,
    pub has_conflicts: bool,
}

impl MergeResult {
    /// Produce the merged text. Conflicts are rendered with markers.
    pub fn to_string_with_markers(&self) -> String {
        let mut out = String::new();
        for region in &self.regions {
            match region {
                MergeRegion::Clean(lines) | MergeRegion::Resolved(lines) => {
                    for line in lines {
                        out.push_str(line);
                        out.push('\n');
                    }
                }
                MergeRegion::Conflict { ours, base: _, theirs } => {
                    out.push_str("<<<<<<< ours\n");
                    for line in ours {
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push_str("=======\n");
                    for line in theirs {
                        out.push_str(line);
                        out.push('\n');
                    }
                    out.push_str(">>>>>>> theirs\n");
                }
            }
        }
        out
    }

    /// Produce clean merged text. Returns None if there are conflicts.
    pub fn to_clean_string(&self) -> Option<String> {
        if self.has_conflicts {
            return None;
        }
        Some(self.to_string_with_markers())
    }
}

/// Perform a three-way merge of text content.
///
/// Given a common base, "ours" and "theirs" versions, produces a merged result
/// with conflict detection.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> MergeResult {
    let base_lines: Vec<&str> = split_lines(base);
    let ours_lines: Vec<&str> = split_lines(ours);
    let theirs_lines: Vec<&str> = split_lines(theirs);

    // Compute diffs from base to each side
    let diff_ours = diff3_segments(&base_lines, &ours_lines);
    let diff_theirs = diff3_segments(&base_lines, &theirs_lines);

    // Walk the base, applying changes from both sides
    let mut regions = Vec::new();
    let mut has_conflicts = false;
    let mut base_pos = 0;

    // Collect change hunks from both sides
    let ours_changes = collect_changes(&diff_ours);
    let theirs_changes = collect_changes(&diff_theirs);

    // Merge by walking through base positions
    let mut oi = 0;
    let mut ti = 0;

    loop {
        // Find next change from either side
        let next_ours = if oi < ours_changes.len() {
            Some(ours_changes[oi].0)
        } else {
            None
        };
        let next_theirs = if ti < theirs_changes.len() {
            Some(theirs_changes[ti].0)
        } else {
            None
        };

        let next = match (next_ours, next_theirs) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };

        // Emit unchanged lines before this change
        if next > base_pos {
            let clean: Vec<String> = base_lines[base_pos..next]
                .iter()
                .map(|s| s.to_string())
                .collect();
            if !clean.is_empty() {
                regions.push(MergeRegion::Clean(clean));
            }
            base_pos = next;
        }

        // Check if both sides have changes at this position
        let ours_here = if oi < ours_changes.len() && ours_changes[oi].0 == base_pos {
            Some(&ours_changes[oi])
        } else {
            None
        };
        let theirs_here = if ti < theirs_changes.len() && theirs_changes[ti].0 == base_pos {
            Some(&theirs_changes[ti])
        } else {
            None
        };

        match (ours_here, theirs_here) {
            (Some(o), Some(t)) => {
                // Both sides changed — check overlap
                let o_end = o.0 + o.1;
                let t_end = t.0 + t.1;
                let max_end = o_end.max(t_end);

                if o.2 == t.2 {
                    // Same change on both sides — no conflict
                    regions.push(MergeRegion::Resolved(
                        o.2.iter().map(|s| s.to_string()).collect(),
                    ));
                } else {
                    // Conflict
                    has_conflicts = true;
                    let base_region: Vec<String> = base_lines
                        [base_pos..max_end.min(base_lines.len())]
                        .iter()
                        .map(|s| s.to_string())
                        .collect();
                    regions.push(MergeRegion::Conflict {
                        base: base_region,
                        ours: o.2.iter().map(|s| s.to_string()).collect(),
                        theirs: t.2.iter().map(|s| s.to_string()).collect(),
                    });
                }
                base_pos = max_end;
                oi += 1;
                ti += 1;
            }
            (Some(o), None) => {
                regions.push(MergeRegion::Resolved(
                    o.2.iter().map(|s| s.to_string()).collect(),
                ));
                base_pos = o.0 + o.1;
                oi += 1;
            }
            (None, Some(t)) => {
                regions.push(MergeRegion::Resolved(
                    t.2.iter().map(|s| s.to_string()).collect(),
                ));
                base_pos = t.0 + t.1;
                ti += 1;
            }
            (None, None) => unreachable!(),
        }
    }

    // Emit remaining unchanged lines
    if base_pos < base_lines.len() {
        let clean: Vec<String> = base_lines[base_pos..]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if !clean.is_empty() {
            regions.push(MergeRegion::Clean(clean));
        }
    }

    MergeResult {
        regions,
        has_conflicts,
    }
}

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.lines().collect()
    }
}

/// Segment types from diffing base to a side.
#[derive(Debug, Clone)]
enum Segment<'a> {
    Equal,
    Delete,
    Insert(&'a str),
}

/// Compute diff segments between base and modified.
fn diff3_segments<'a>(base: &[&'a str], modified: &[&'a str]) -> Vec<Segment<'a>> {
    let lcs = lcs_table(base, modified);
    let mut i = base.len();
    let mut j = modified.len();

    let mut result = Vec::new();
    while i > 0 && j > 0 {
        if base[i - 1] == modified[j - 1] {
            result.push(Segment::Equal);
            i -= 1;
            j -= 1;
        } else if lcs[i - 1][j] >= lcs[i][j - 1] {
            result.push(Segment::Delete);
            i -= 1;
        } else {
            result.push(Segment::Insert(modified[j - 1]));
            j -= 1;
        }
    }
    while i > 0 {
        result.push(Segment::Delete);
        i -= 1;
    }
    while j > 0 {
        result.push(Segment::Insert(modified[j - 1]));
        j -= 1;
    }
    result.reverse();
    result
}

/// Compute LCS table.
fn lcs_table(a: &[&str], b: &[&str]) -> Vec<Vec<usize>> {
    let n = a.len();
    let m = b.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    dp
}

/// Collect change hunks from segments.
/// Returns Vec<(base_start, base_count, replacement_lines)>.
fn collect_changes<'a>(segments: &[Segment<'a>]) -> Vec<(usize, usize, Vec<&'a str>)> {
    let mut changes = Vec::new();
    let mut base_pos = 0;
    let mut i = 0;

    while i < segments.len() {
        match &segments[i] {
            Segment::Equal => {
                base_pos += 1;
                i += 1;
            }
            _ => {
                // Collect contiguous change
                let start = base_pos;
                let mut deleted = 0;
                let mut inserted = Vec::new();

                while i < segments.len() {
                    match &segments[i] {
                        Segment::Delete => {
                            deleted += 1;
                            base_pos += 1;
                            i += 1;
                        }
                        Segment::Insert(line) => {
                            inserted.push(*line);
                            i += 1;
                        }
                        Segment::Equal => break,
                    }
                }

                changes.push((start, deleted, inserted));
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_changes() {
        let base = "line1\nline2\nline3";
        let result = merge3(base, base, base);
        assert!(!result.has_conflicts);
        assert_eq!(result.to_clean_string().unwrap(), "line1\nline2\nline3\n");
    }

    #[test]
    fn test_ours_only_change() {
        let base = "line1\nline2\nline3";
        let ours = "line1\nmodified\nline3";
        let result = merge3(base, ours, base);
        assert!(!result.has_conflicts);
        assert_eq!(
            result.to_clean_string().unwrap(),
            "line1\nmodified\nline3\n"
        );
    }

    #[test]
    fn test_theirs_only_change() {
        let base = "line1\nline2\nline3";
        let theirs = "line1\nline2\nchanged";
        let result = merge3(base, base, theirs);
        assert!(!result.has_conflicts);
        assert_eq!(
            result.to_clean_string().unwrap(),
            "line1\nline2\nchanged\n"
        );
    }

    #[test]
    fn test_both_different_regions() {
        let base = "line1\nline2\nline3";
        let ours = "changed1\nline2\nline3";
        let theirs = "line1\nline2\nchanged3";
        let result = merge3(base, ours, theirs);
        assert!(!result.has_conflicts);
        assert_eq!(
            result.to_clean_string().unwrap(),
            "changed1\nline2\nchanged3\n"
        );
    }

    #[test]
    fn test_same_change_both_sides() {
        let base = "line1\nline2\nline3";
        let both = "line1\nSAME\nline3";
        let result = merge3(base, both, both);
        assert!(!result.has_conflicts);
        assert_eq!(
            result.to_clean_string().unwrap(),
            "line1\nSAME\nline3\n"
        );
    }

    #[test]
    fn test_conflict() {
        let base = "line1\nline2\nline3";
        let ours = "line1\nours\nline3";
        let theirs = "line1\ntheirs\nline3";
        let result = merge3(base, ours, theirs);
        assert!(result.has_conflicts);
        assert!(result.to_clean_string().is_none());

        let text = result.to_string_with_markers();
        assert!(text.contains("<<<<<<< ours"));
        assert!(text.contains("ours"));
        assert!(text.contains("======="));
        assert!(text.contains("theirs"));
        assert!(text.contains(">>>>>>> theirs"));
    }

    #[test]
    fn test_ours_adds_lines() {
        let base = "line1\nline3";
        let ours = "line1\nline2\nline3";
        let result = merge3(base, ours, base);
        assert!(!result.has_conflicts);
        assert_eq!(
            result.to_clean_string().unwrap(),
            "line1\nline2\nline3\n"
        );
    }

    #[test]
    fn test_theirs_deletes_lines() {
        let base = "line1\nline2\nline3";
        let theirs = "line1\nline3";
        let result = merge3(base, base, theirs);
        assert!(!result.has_conflicts);
        assert_eq!(result.to_clean_string().unwrap(), "line1\nline3\n");
    }

    #[test]
    fn test_empty_base() {
        let result = merge3("", "added", "");
        assert!(!result.has_conflicts);
        assert_eq!(result.to_clean_string().unwrap(), "added\n");
    }
}
