//! AAAK-inspired review compression.
//!
//! Compresses structured review markdown into a terse pipe-delimited format
//! optimized for AI agent token usage. The raw `.md` stays on disk for humans;
//! this module provides on-the-fly compression at read time.

use crate::models::ReviewerType;
use std::fmt;

/// Compression errors.
#[derive(Debug)]
pub enum CompressError {
    /// Could not parse enough structure to compress.
    ParseFailed(String),
}

impl fmt::Display for CompressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseFailed(reason) => write!(f, "parse failed: {reason}"),
        }
    }
}

/// Check if compact format is enabled via environment variable.
pub fn is_enabled() -> bool {
    std::env::var("REVIEW_MCP_COMPACT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Compress a review markdown into a terse pipe-delimited format.
///
/// Returns `Ok(compressed)` on success, `Err` if the content cannot be parsed.
pub fn compress_review(
    content: &str,
    reviewer_type: ReviewerType,
) -> Result<String, CompressError> {
    let stripped = strip_fenced_code(content);
    match reviewer_type {
        ReviewerType::Regular | ReviewerType::Harsh => compress_code_review(&stripped, reviewer_type),
        ReviewerType::Grounded => compress_grounded_review(&stripped),
    }
}

// ---------------------------------------------------------------------------
// Pre-processing: strip fenced code blocks
// ---------------------------------------------------------------------------

fn strip_fenced_code(content: &str) -> String {
    let mut out = Vec::new();
    let mut in_fence = false;

    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            if in_fence {
                // closing fence — skip this line too
                in_fence = false;
            } else {
                // opening fence — skip this line and subsequent content
                in_fence = true;
            }
            continue;
        }
        if !in_fence {
            out.push(line);
        }
    }

    out.join("\n")
}

// ---------------------------------------------------------------------------
// Metadata extraction helpers
// ---------------------------------------------------------------------------

fn extract_meta_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let trimmed = line.trim();
    if trimmed.starts_with(key) {
        Some(trimmed[key.len()..].trim())
    } else {
        None
    }
}

fn extract_backtick_content(s: &str) -> &str {
    if let Some(start) = s.find('`') {
        if let Some(end) = s[start + 1..].find('`') {
            return &s[start + 1..start + 1 + end];
        }
    }
    s.trim()
}

fn abbreviate_severity(sev: &str) -> &str {
    match sev.trim().to_uppercase().as_str() {
        "CRITICAL" => "CRIT",
        "MEDIUM" => "MED",
        "NITPICK" => "NIT",
        // HIGH and LOW stay as-is; unknown values pass through
        _ => return leak_or_passthrough(sev.trim()),
    }
}

// For unknown severity values, return them trimmed.
// We use a small trick: known short values are returned as static strs,
// and for the passthrough case we just return the trimmed input.
fn leak_or_passthrough(s: &str) -> &str {
    match s.to_uppercase().as_str() {
        "HIGH" => "HIGH",
        "LOW" => "LOW",
        _ => s,
    }
}

fn escape_pipes(s: &str) -> String {
    s.replace('|', "\\|")
}

fn truncate_at_word_boundary(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Find the last space before the limit
    let truncated = &s[..max];
    if let Some(pos) = truncated.rfind(' ') {
        format!("{}...", &s[..pos])
    } else {
        format!("{truncated}...")
    }
}

fn collapse_multiline(text: &str) -> String {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Code review compression (regular/harsh)
// ---------------------------------------------------------------------------

fn compress_code_review(content: &str, reviewer_type: ReviewerType) -> Result<String, CompressError> {
    let mut branch = String::new();
    let mut commit = String::new();
    let mut round_num = String::new();
    let mut recommendation = String::new();
    let mut total_findings = String::new();
    let mut counts = SeverityCounts::default();
    let mut files: Vec<String> = Vec::new();
    let mut findings: Vec<Finding> = Vec::new();
    let mut positive_count: usize = 0;
    let mut warnings: Vec<String> = Vec::new();

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut current_section = "";

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Track sections
        if trimmed.starts_with("## ") {
            current_section = trimmed[3..].trim();
        }

        // Metadata header
        if let Some(v) = extract_meta_value(trimmed, "**Branch**:") {
            branch = v.to_string();
        } else if let Some(v) = extract_meta_value(trimmed, "**Commit**:") {
            commit = v.to_string();
        } else if let Some(v) = extract_meta_value(trimmed, "**Round**:") {
            round_num = v.to_string();
        } else if let Some(v) = extract_meta_value(trimmed, "**Recommendation**:") {
            recommendation = v.trim().to_uppercase();
        } else if let Some(v) = extract_meta_value(trimmed, "**Total Findings**:") {
            total_findings = v.split_whitespace().next().unwrap_or("0").to_string();
        }

        // Severity counts
        parse_severity_count(trimmed, &mut counts);

        // Files reviewed
        if current_section.starts_with("Files Reviewed") {
            if let Some(path) = extract_file_path(trimmed) {
                files.push(path);
            }
        }

        // Positive observations
        if current_section.starts_with("Positive Observations") && trimmed.starts_with("- ") {
            positive_count += 1;
        }

        // Findings: ### ID: Title
        if trimmed.starts_with("### ") && trimmed.contains(':') {
            let heading = &trimmed[4..];
            if let Some(finding) = parse_finding(heading, &lines, i + 1, false) {
                findings.push(finding.0);
                i = finding.1;
                continue;
            }
        }

        i += 1;
    }

    // Validate we got enough structure
    if recommendation.is_empty() && total_findings.is_empty() {
        return Err(CompressError::ParseFailed(
            "no Summary section found (missing Recommendation and Total Findings)".to_string(),
        ));
    }

    // Emit compact format
    let mut out = Vec::new();
    out.push("@version 1".to_string());

    let r = if round_num.is_empty() { "r?" } else { &format!("r{round_num}") };
    out.push(format!(
        "@review {} {} | {} | {} findings",
        reviewer_type, r, recommendation, total_findings
    ));
    out.push(format!(
        "@counts C:{} H:{} M:{} L:{} N:{}",
        counts.critical, counts.high, counts.medium, counts.low, counts.nitpick
    ));

    if !branch.is_empty() || !commit.is_empty() {
        let mut meta = String::new();
        if !branch.is_empty() {
            meta.push_str(&format!("@branch {branch}"));
        }
        if !commit.is_empty() {
            if !meta.is_empty() {
                meta.push(' ');
            }
            meta.push_str(&format!("@commit {commit}"));
        }
        out.push(meta);
    }

    if !files.is_empty() {
        out.push(format!("@files {}", files.join(" ")));
    }
    if positive_count > 0 {
        out.push(format!("@positive {positive_count} items omitted"));
    }

    if !findings.is_empty() {
        out.push(String::new()); // blank line
        let parsed_count = findings.len();
        for f in &findings {
            out.push(format_code_review_finding(f));
        }
        let expected: usize = total_findings.parse().unwrap_or(0);
        if parsed_count < expected {
            let missed = expected - parsed_count;
            warnings.push(format!("{missed} findings could not be parsed"));
        }
    }

    for w in &warnings {
        out.push(format!("@warning {w}"));
    }

    Ok(out.join("\n"))
}

// ---------------------------------------------------------------------------
// Grounded review compression
// ---------------------------------------------------------------------------

fn compress_grounded_review(content: &str) -> Result<String, CompressError> {
    let mut round_num = String::new();
    let mut recommendation = String::new();
    let mut total_unified = String::new();
    let mut counts = SeverityCounts::default();
    let mut positive_count: usize = 0;
    let mut warnings: Vec<String> = Vec::new();

    // Grounded-specific
    let mut accuracy_regular = String::new();
    let mut accuracy_harsh = String::new();
    let mut input_regular = String::new();
    let mut input_harsh = String::new();
    let mut contradiction_count: usize = 0;
    let mut rejected_items: Vec<String> = Vec::new();
    let mut disputed_items: Vec<String> = Vec::new();
    let mut unified_findings: Vec<Finding> = Vec::new();

    // Track which expected sections we found
    let expected_sections = [
        "Reviewer Accuracy Assessment",
        "Input Reviews",
        "Contradictions",
        "Rejected Findings",
        "Disputed Findings",
    ];
    let mut found_sections: Vec<bool> = vec![false; expected_sections.len()];

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    let mut current_section = "";

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Track sections
        if trimmed.starts_with("## ") {
            current_section = trimmed[3..].trim();
            for (idx, expected) in expected_sections.iter().enumerate() {
                if current_section.contains(expected) {
                    found_sections[idx] = true;
                }
            }
        }

        // Metadata
        if let Some(v) = extract_meta_value(trimmed, "**Round**:") {
            round_num = v.to_string();
        } else if let Some(v) = extract_meta_value(trimmed, "**Recommendation**:") {
            recommendation = v.trim().to_uppercase();
        } else if let Some(v) = extract_meta_value(trimmed, "**Final Unified Findings**:") {
            total_unified = v.split_whitespace().next().unwrap_or("0").to_string();
        }

        parse_severity_count(trimmed, &mut counts);

        // Positive observations
        if current_section.starts_with("Positive Observations") && trimmed.starts_with("- ") {
            positive_count += 1;
        }

        // Input Reviews section
        if current_section.contains("Input Reviews") {
            if trimmed.starts_with("**Regular Review**:") || trimmed.starts_with("- Total Findings:") || trimmed.starts_with("- Recommendation:") {
                if trimmed.contains("Total Findings:") {
                    let count = trimmed.split(':').last().unwrap_or("").trim();
                    if input_regular.is_empty() {
                        input_regular = count.to_string();
                    } else if input_harsh.is_empty() {
                        input_harsh = count.to_string();
                    }
                }
                if trimmed.contains("Recommendation:") {
                    let rec = trimmed.split(':').last().unwrap_or("").trim().to_uppercase();
                    if input_regular.is_empty() || (!input_regular.contains('/') && !input_regular.is_empty()) {
                        input_regular = format!("{input_regular}/{rec}");
                    } else {
                        input_harsh = format!("{input_harsh}/{rec}");
                    }
                }
            }
        }

        // Reviewer Accuracy Assessment
        if current_section.contains("Reviewer Accuracy Assessment") {
            // Accuracy lines are inside bullet points: "- **Accuracy**: 67%"
            let check = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            if let Some(v) = extract_meta_value(check, "**Accuracy**:") {
                if accuracy_regular.is_empty() {
                    accuracy_regular = v.to_string();
                } else if accuracy_harsh.is_empty() {
                    accuracy_harsh = v.to_string();
                }
            }
        }

        // Contradictions — count ### headings
        if current_section == "Contradictions" && trimmed.starts_with("### ") {
            contradiction_count += 1;
        }

        // Rejected Findings
        if current_section.contains("Rejected Findings") && trimmed.starts_with("### ") {
            let heading = &trimmed[4..];
            // Extract ID and reason
            let id = heading.split(':').next().unwrap_or(heading).trim();
            // Look ahead for "Why Rejected" line
            let mut reason = String::new();
            for j in (i + 1)..lines.len().min(i + 10) {
                if let Some(v) = extract_meta_value(lines[j].trim(), "**Why Rejected**:") {
                    reason = truncate_at_word_boundary(&collapse_multiline(v), 80);
                    break;
                }
                if let Some(v) = extract_meta_value(lines[j].trim(), "**Original Claim**:") {
                    reason = truncate_at_word_boundary(&collapse_multiline(v), 80);
                    break;
                }
                if lines[j].trim().starts_with("### ") || lines[j].trim().starts_with("## ") {
                    break;
                }
            }
            rejected_items.push(format!("{id}:{reason}"));
        }

        // Disputed Findings
        if current_section.contains("Disputed Findings") && trimmed.starts_with("### ") {
            let heading = &trimmed[4..];
            let id = heading.split(':').next().unwrap_or(heading).trim();
            let mut reason = String::new();
            for j in (i + 1)..lines.len().min(i + 10) {
                if let Some(v) = extract_meta_value(lines[j].trim(), "**Why Disputed**:") {
                    reason = truncate_at_word_boundary(&collapse_multiline(v), 80);
                    break;
                }
                if lines[j].trim().starts_with("### ") || lines[j].trim().starts_with("## ") {
                    break;
                }
            }
            disputed_items.push(format!("{id}:{reason}"));
        }

        // Unified Findings
        if current_section.contains("Unified Findings") && trimmed.starts_with("### ") && trimmed.contains(':') {
            let heading = &trimmed[4..];
            if let Some(finding) = parse_finding(heading, &lines, i + 1, true) {
                unified_findings.push(finding.0);
                i = finding.1;
                continue;
            }
        }

        i += 1;
    }

    // Validate
    if recommendation.is_empty() && total_unified.is_empty() {
        return Err(CompressError::ParseFailed(
            "no Verification Summary section found".to_string(),
        ));
    }

    // Check for missing sections
    let mut missing: Vec<&str> = Vec::new();
    for (idx, found) in found_sections.iter().enumerate() {
        if !found {
            missing.push(expected_sections[idx]);
        }
    }

    // Emit
    let mut out = Vec::new();
    out.push("@version 1".to_string());

    let r = if round_num.is_empty() { "r?".to_string() } else { format!("r{round_num}") };
    out.push(format!(
        "@grounded {r} | {recommendation} | {total_unified} unified"
    ));
    out.push(format!(
        "@counts C:{} H:{} M:{} L:{} N:{}",
        counts.critical, counts.high, counts.medium, counts.low, counts.nitpick
    ));

    if !accuracy_regular.is_empty() || !accuracy_harsh.is_empty() {
        out.push(format!(
            "@accuracy regular={} harsh={}",
            accuracy_regular, accuracy_harsh
        ));
    }
    if !input_regular.is_empty() || !input_harsh.is_empty() {
        out.push(format!(
            "@input regular:{} harsh:{}",
            input_regular, input_harsh
        ));
    }
    if positive_count > 0 {
        out.push(format!("@positive {positive_count} items omitted"));
    }

    if !unified_findings.is_empty() {
        out.push(String::new());
        let parsed_count = unified_findings.len();
        for f in &unified_findings {
            out.push(format_grounded_finding(f));
        }
        let expected: usize = total_unified.parse().unwrap_or(0);
        if parsed_count < expected {
            let missed = expected - parsed_count;
            warnings.push(format!("{missed} findings could not be parsed"));
        }
    }

    if !rejected_items.is_empty() {
        out.push(format!("@rejected {}", rejected_items.join(" ")));
    }
    if !disputed_items.is_empty() {
        out.push(format!("@disputed {}", disputed_items.join(" ")));
    }
    if contradiction_count > 0 {
        out.push(format!("@contradictions {contradiction_count}"));
    }

    if !missing.is_empty() {
        out.push(format!("@warning missing sections: {}", missing.join(", ")));
    }
    for w in &warnings {
        out.push(format!("@warning {w}"));
    }

    Ok(out.join("\n"))
}

// ---------------------------------------------------------------------------
// Finding parser
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SeverityCounts {
    critical: u32,
    high: u32,
    medium: u32,
    low: u32,
    nitpick: u32,
}

fn parse_severity_count(line: &str, counts: &mut SeverityCounts) {
    let trimmed = line.trim();
    if trimmed.starts_with("- CRITICAL:") {
        counts.critical = extract_count(trimmed);
    } else if trimmed.starts_with("- HIGH:") {
        counts.high = extract_count(trimmed);
    } else if trimmed.starts_with("- MEDIUM:") {
        counts.medium = extract_count(trimmed);
    } else if trimmed.starts_with("- LOW:") {
        counts.low = extract_count(trimmed);
    } else if trimmed.starts_with("- NITPICK:") {
        counts.nitpick = extract_count(trimmed);
    }
}

fn extract_count(line: &str) -> u32 {
    line.split(':')
        .last()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

struct Finding {
    id: String,
    severity: String,
    location: String,
    description: String,
    recommendation: String,
    verdict: String, // empty for code reviews
}

/// Parse a finding block starting after the `### ID: Title` heading.
/// Returns the Finding and the line index to resume from.
fn parse_finding(heading: &str, lines: &[&str], start: usize, is_grounded: bool) -> Option<(Finding, usize)> {
    let id = heading.split(':').next()?.trim().to_string();

    let mut severity = String::new();
    let mut location = String::new();
    let mut description = String::new();
    let mut recommendation = String::new();
    let mut verdict = String::new();
    let mut i = start;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Stop at next finding or section
        if trimmed.starts_with("### ") || trimmed.starts_with("## ") || trimmed == "---" {
            break;
        }

        if let Some(v) = extract_meta_value(trimmed, "**Severity**:") {
            severity = v.to_string();
        } else if let Some(v) = extract_meta_value(trimmed, "**Location**:") {
            location = extract_backtick_content(v).to_string();
        } else if let Some(v) = extract_meta_value(trimmed, "**Verdict**:") {
            verdict = v.split_whitespace()
                .find(|w| matches!(*w, "CONFIRMED" | "DISPUTED" | "REJECTED"))
                .unwrap_or(v.trim())
                .to_string();
        } else if trimmed.starts_with("**Description**:") || trimmed.starts_with("**Issue**:") || trimmed.starts_with("**Issue Confirmed**:") {
            let first_line = trimmed.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
            let text = collect_multiline_field(first_line, lines, i + 1);
            description = text.0;
            i = text.1;
            continue;
        } else if trimmed.starts_with("**Recommendation**:") || trimmed.starts_with("**Recommended Fix**:") {
            let first_line = trimmed.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
            let text = collect_multiline_field(first_line, lines, i + 1);
            recommendation = text.0;
            i = text.1;
            continue;
        }

        i += 1;
    }

    // Only require severity for non-grounded or require verdict for grounded
    if severity.is_empty() && !is_grounded {
        return None;
    }

    Some((
        Finding {
            id,
            severity,
            location,
            description,
            recommendation,
            verdict,
        },
        i,
    ))
}

/// Collect multi-line field value until next **Key**: line, heading, or blank line.
fn collect_multiline_field(first_line: &str, lines: &[&str], start: usize) -> (String, usize) {
    let mut parts = vec![first_line.to_string()];
    let mut i = start;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.is_empty()
            || trimmed.starts_with("**")
            || trimmed.starts_with("### ")
            || trimmed.starts_with("## ")
            || trimmed == "---"
        {
            break;
        }
        parts.push(trimmed.to_string());
        i += 1;
    }

    let collapsed = collapse_multiline(&parts.join("\n"));
    let escaped = escape_pipes(&collapsed);
    let truncated = truncate_at_word_boundary(&escaped, 200);
    (truncated, i)
}

fn extract_file_path(line: &str) -> Option<String> {
    let trimmed = line.trim();
    // Match lines like: 1. `path/to/file.go` — description
    if trimmed.len() > 2 && trimmed.chars().next()?.is_ascii_digit() && trimmed.contains(". `") {
        let path = extract_backtick_content(trimmed);
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }
    None
}

fn format_code_review_finding(f: &Finding) -> String {
    let sev = abbreviate_severity(&f.severity);
    let desc = if f.description.is_empty() {
        "-".to_string()
    } else {
        f.description.clone()
    };
    let rec = if f.recommendation.is_empty() {
        "-".to_string()
    } else {
        f.recommendation.clone()
    };
    let loc = if f.location.is_empty() {
        "-".to_string()
    } else {
        f.location.clone()
    };
    format!("{}|{}|{}|{}|{}", f.id, sev, loc, desc, rec)
}

fn format_grounded_finding(f: &Finding) -> String {
    let sev = abbreviate_severity(&f.severity);
    let verdict = if f.verdict.is_empty() { "-" } else { &f.verdict };
    let desc = if f.description.is_empty() {
        "-".to_string()
    } else {
        f.description.clone()
    };
    let rec = if f.recommendation.is_empty() {
        "-".to_string()
    } else {
        f.recommendation.clone()
    };
    let loc = if f.location.is_empty() {
        "-".to_string()
    } else {
        f.location.clone()
    };
    format!("{}|{}|{}|{}|{}|{}", f.id, verdict, sev, loc, desc, rec)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_REGULAR: &str = r#"# Code Review — Regular Mode

**Review Mode**: regular
**Branch**: feature-x
**Commit**: abc1234
**Reviewer**: reviewer agent (v1.0)
**Timestamp**: 2026-04-08
**Round**: 1

---

## Summary

**Total Findings**: 3
- CRITICAL: 0
- HIGH: 1
- MEDIUM: 1
- LOW: 1
- NITPICK: 0

**Overall Assessment**: Code looks reasonable with minor issues.

**Recommendation**: REQUEST_CHANGES

---

## Files Reviewed

1. `src/tools.rs` — Main tool handlers
2. `src/db.rs` — Database layer

---

## Findings

### R-001: Missing error check on query result

**Severity**: HIGH
**Location**: `src/tools.rs:42`

**Description**: The error return from `db.QueryUser()` is not checked,
which could lead to nil pointer dereference if the query fails.

**Evidence**:
```go
result := db.QueryUser(id)
user := result.Data
```

**Recommendation**: Add error check before accessing result fields.

---

### R-002: Unbounded slice growth

**Severity**: MEDIUM
**Location**: `src/tools.rs:108-115`

**Description**: The slice grows without a capacity hint.

**Recommendation**: Use Vec::with_capacity for known sizes.

---

### R-003: Unused import

**Severity**: LOW
**Location**: `src/db.rs:55`

**Description**: The `fmt` import is unused.

**Recommendation**: Remove the unused import.

---

## Positive Observations

- Good error handling in session creation
- Clean separation of concerns
- Well-structured tests
"#;

    const SAMPLE_GROUNDED: &str = r#"# Grounded Review

**Branch**: feature-x
**Commit**: abc1234
**Verifier**: grounding-verifier agent (v1.0)
**Timestamp**: 2026-04-08
**Round**: 1

---

## Input Reviews

**Regular Review**: session abc, round 1
- Total Findings: 3
- Recommendation: REQUEST_CHANGES

**Harsh Review**: session abc, round 1
- Total Findings: 5
- Recommendation: BLOCK

---

## Verification Summary

**Total Findings Processed**: 8
**Verification Outcomes**:
- CONFIRMED: 4
- DISPUTED: 2
- REJECTED: 2

**Final Unified Findings**: 3
- CRITICAL: 0
- HIGH: 1
- MEDIUM: 1
- LOW: 1
- NITPICK: 0

---

## Unified Findings

### UF-001: Missing error check on query result

**Original Findings**: R-001, H-001
**Verdict**: CONFIRMED
**Severity**: HIGH
**Location**: `src/tools.rs:42`

**Issue**: The error return from db.QueryUser() is not checked.

**Impact**: Nil pointer dereference on query failure.

**Recommended Fix**: Add error guard before accessing result.

---

### UF-002: Unbounded slice growth

**Original Findings**: R-002
**Verdict**: DISPUTED
**Severity**: MEDIUM
**Location**: `src/tools.rs:108`

**Issue**: Slice grows without capacity hint but impact is debatable.

**Recommended Fix**: Use with_capacity if confirmed.

---

### UF-003: Unused import

**Original Findings**: R-003
**Verdict**: CONFIRMED
**Severity**: LOW
**Location**: `src/db.rs:55`

**Issue**: The fmt import is unused.

**Recommended Fix**: Remove import.

---

## Rejected Findings

### RJ-001: H-003 — Citation mismatch

**Original Claim**: Line 200 has a bug.
**Why Rejected**: Line 200 is a comment, not code.

---

## Disputed Findings

### DP-001: R-002 — Severity uncertain

**Original Claim**: Unbounded growth is HIGH severity.
**Why Disputed**: Needs runtime profiling to confirm.

---

## Contradictions

### Contradiction 1: Severity of error check

| Aspect | Regular | Harsh |
|--------|---------|-------|
| Severity | HIGH | CRITICAL |

**Resolution**: HIGH is appropriate.

---

## Reviewer Accuracy Assessment

**Regular Reviewer**:
- Total Findings: 3
- Confirmed: 2
- **Accuracy**: 67%

**Harsh Reviewer**:
- Total Findings: 5
- Confirmed: 2
- **Accuracy**: 40%

---

## Notes for QA-Gate

**Recommendation**: CONDITIONAL

**Confidence**: HIGH
"#;

    #[test]
    fn test_compress_regular_golden_path() {
        let result = compress_review(SAMPLE_REGULAR, ReviewerType::Regular).unwrap();
        assert!(result.starts_with("@version 1\n"));
        assert!(result.contains("@review regular r1 | REQUEST_CHANGES | 3 findings"));
        assert!(result.contains("@counts C:0 H:1 M:1 L:1 N:0"));
        assert!(result.contains("@branch feature-x"));
        assert!(result.contains("@commit abc1234"));
        assert!(result.contains("@files src/tools.rs src/db.rs"));
        assert!(result.contains("@positive 3 items omitted"));
        assert!(result.contains("R-001|HIGH|src/tools.rs:42|"));
        assert!(result.contains("R-002|MED|src/tools.rs:108-115|"));
        assert!(result.contains("R-003|LOW|src/db.rs:55|"));
    }

    #[test]
    fn test_compress_grounded_golden_path() {
        let result = compress_review(SAMPLE_GROUNDED, ReviewerType::Grounded).unwrap();
        assert!(result.starts_with("@version 1\n"));
        assert!(result.contains("@grounded r1 | CONDITIONAL"));
        assert!(result.contains("@counts C:0 H:1 M:1 L:1 N:0"));
        assert!(result.contains("UF-001|CONFIRMED|HIGH|src/tools.rs:42|"));
        assert!(result.contains("UF-002|DISPUTED|MED|src/tools.rs:108|"));
        assert!(result.contains("UF-003|CONFIRMED|LOW|src/db.rs:55|"));
        assert!(result.contains("@rejected"));
        assert!(result.contains("@disputed"));
        assert!(result.contains("@contradictions 1"));
        assert!(result.contains("@accuracy"));
    }

    #[test]
    fn test_malformed_input() {
        let content = "# Some random document\n\nNo summary here.\n";
        let result = compress_review(content, ReviewerType::Regular);
        assert!(result.is_err());
    }

    #[test]
    fn test_zero_findings() {
        let content = r#"# Code Review

**Round**: 1

## Summary

**Total Findings**: 0
- CRITICAL: 0
- HIGH: 0
- MEDIUM: 0
- LOW: 0
- NITPICK: 0

**Recommendation**: APPROVE
"#;
        let result = compress_review(content, ReviewerType::Regular).unwrap();
        assert!(result.contains("@review regular r1 | APPROVE | 0 findings"));
        assert!(!result.contains("R-001"));
    }

    #[test]
    fn test_pipe_in_description() {
        let content = r#"# Code Review

**Round**: 1

## Summary

**Total Findings**: 1
- CRITICAL: 0
- HIGH: 1
- MEDIUM: 0
- LOW: 0
- NITPICK: 0

**Recommendation**: REQUEST_CHANGES

## Findings

### R-001: Pipe issue

**Severity**: HIGH
**Location**: `src/main.rs:10`

**Description**: The expression `a | b` should use `a || b` instead.

**Recommendation**: Replace `|` with `||`.

---
"#;
        let result = compress_review(content, ReviewerType::Regular).unwrap();
        assert!(result.contains("\\|"));
        // Verify the line count is correct (finding should be one line)
        let finding_line = result.lines().find(|l| l.starts_with("R-001")).unwrap();
        assert!(finding_line.contains("a \\| b"));
    }

    #[test]
    fn test_nested_code_fences() {
        let content = r#"# Code Review

**Round**: 1

## Summary

**Total Findings**: 1
- CRITICAL: 0
- HIGH: 0
- MEDIUM: 1
- LOW: 0
- NITPICK: 0

**Recommendation**: APPROVE

## Findings

### R-001: Code issue

**Severity**: MEDIUM
**Location**: `src/main.rs:5`

**Description**: Issue found.

**Evidence**:
```rust
fn main() {
    println!("```nested fence```");
}
```

**Recommendation**: Fix it.

---
"#;
        let result = compress_review(content, ReviewerType::Regular).unwrap();
        assert!(result.contains("R-001|MED|src/main.rs:5|"));
        // The code block content should not appear
        assert!(!result.contains("println!"));
    }

    #[test]
    fn test_missing_grounded_sections() {
        let content = r#"# Grounded Review

**Round**: 1

## Verification Summary

**Final Unified Findings**: 0
- CRITICAL: 0
- HIGH: 0
- MEDIUM: 0
- LOW: 0
- NITPICK: 0

## Notes for QA-Gate

**Recommendation**: APPROVED
"#;
        let result = compress_review(content, ReviewerType::Grounded).unwrap();
        assert!(result.contains("@warning missing sections:"));
        assert!(result.contains("Reviewer Accuracy Assessment"));
        assert!(result.contains("Input Reviews"));
    }

    #[test]
    fn test_long_description_truncation() {
        let long_desc = "A ".repeat(150); // 300 chars
        let content = format!(
            r#"# Code Review

**Round**: 1

## Summary

**Total Findings**: 1
- CRITICAL: 1
- HIGH: 0
- MEDIUM: 0
- LOW: 0
- NITPICK: 0

**Recommendation**: BLOCK

## Findings

### R-001: Long finding

**Severity**: CRITICAL
**Location**: `src/main.rs:1`

**Description**: {long_desc}

**Recommendation**: Fix it.

---
"#
        );
        let result = compress_review(&content, ReviewerType::Regular).unwrap();
        let finding_line = result.lines().find(|l| l.starts_with("R-001")).unwrap();
        // The description field should be truncated with ...
        assert!(finding_line.contains("..."));
        // Each field in the pipe-delimited line
        let fields: Vec<&str> = finding_line.split('|').collect();
        assert!(fields[3].len() <= 204); // 200 + "..."
    }

    #[test]
    fn test_multiline_description() {
        let content = r#"# Code Review

**Round**: 1

## Summary

**Total Findings**: 1
- CRITICAL: 0
- HIGH: 1
- MEDIUM: 0
- LOW: 0
- NITPICK: 0

**Recommendation**: REQUEST_CHANGES

## Findings

### R-001: Multi-line issue

**Severity**: HIGH
**Location**: `src/main.rs:10`

**Description**: First line of description.
Second line continues the thought.
Third line wraps up.

**Recommendation**: Do something about it.

---
"#;
        let result = compress_review(content, ReviewerType::Regular).unwrap();
        let finding_line = result.lines().find(|l| l.starts_with("R-001")).unwrap();
        // All lines should be collapsed to single space-separated
        assert!(finding_line.contains("First line of description. Second line continues the thought. Third line wraps up."));
    }

    #[test]
    fn test_is_enabled_default() {
        // By default (no env var), should be disabled
        // Note: this test may be affected by test environment
        std::env::remove_var("REVIEW_MCP_COMPACT");
        assert!(!is_enabled());
    }

    #[test]
    fn test_strip_fenced_code() {
        let input = "before\n```rust\ncode here\n```\nafter\n";
        let result = strip_fenced_code(input);
        assert!(result.contains("before"));
        assert!(result.contains("after"));
        assert!(!result.contains("code here"));
    }

    #[test]
    fn test_truncate_at_word_boundary() {
        assert_eq!(truncate_at_word_boundary("short", 200), "short");
        let long = "word ".repeat(50); // 250 chars
        let result = truncate_at_word_boundary(&long, 200);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 204);
    }

    #[test]
    fn test_escape_pipes() {
        assert_eq!(escape_pipes("a | b"), "a \\| b");
        assert_eq!(escape_pipes("no pipes"), "no pipes");
    }
}
