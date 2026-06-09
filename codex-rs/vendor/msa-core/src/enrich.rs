//! Document text enrichment: compose an indexable `Document.text` from a
//! short summary plus a bounded, sanitized detail section.
//!
//! Domain-neutral by contract: this module knows nothing about who produces
//! the summary/detail (no capsule kinds, no caller state, no policy). The
//! caller decides *when* to enrich (gating) and *what* the detail source is;
//! this module owns *how* the text is composed, sanitized and bounded, so
//! every embedder (MCP server, vendored copies, future CLIs) shares one
//! deterministic, dependency-free implementation.
//!
//! Sanitization is a char-level deterministic scanner (no regex dependency):
//! secret-shaped tokens are replaced with `[redacted]`, oversized code fences
//! are truncated, whitespace is collapsed. Sanitize FIRST, cap AFTER — the
//! cap must never be what saves a secret from redaction.

/// Version of the enriched `Document.text` layout. Stored by callers in
/// document metadata so mixed collections remain interpretable.
pub const INDEX_TEXT_VERSION: u64 = 2;

/// Conventional metadata key names (callers are free to add their own).
pub const METADATA_KEY_INDEX_TEXT_VERSION: &str = "index_text_version";
pub const METADATA_KEY_RICH: &str = "rich";
pub const METADATA_KEY_DETAIL_CHARS: &str = "detail_chars";
pub const METADATA_KEY_SOURCE: &str = "source";

/// Maximum lines a fenced code block may keep before truncation.
const MAX_FENCE_LINES: usize = 12;
/// Replacement marker for redacted secrets. Deliberately contains no
/// alphanumeric run long enough to be mistaken for a secret itself.
const REDACTED: &str = "[redacted]";
const FENCE_TRUNCATED: &str = "[code truncated]";

/// Result of [`compose_rich_text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedText {
    /// The text to index as `Document.text`.
    pub text: String,
    /// Characters of detail actually included (after bounding).
    pub detail_chars: usize,
    /// True if the detail was longer than the applied cap.
    pub truncated: bool,
}

/// Compose `Document.text` from a summary and an (already sanitized) detail,
/// bounding the detail to `max_detail_chars` Unicode scalar values.
///
/// Empty/whitespace detail degrades to the plain summary (layout v1): callers
/// keep today's exact behavior when there is nothing to enrich with.
pub fn compose_rich_text(summary: &str, detail: &str, max_detail_chars: usize) -> ComposedText {
    let detail = detail.trim();
    if detail.is_empty() {
        return ComposedText {
            text: summary.to_string(),
            detail_chars: 0,
            truncated: false,
        };
    }

    let mut bounded = String::new();
    let mut n = 0usize;
    let mut truncated = false;
    for ch in detail.chars() {
        if n >= max_detail_chars {
            truncated = true;
            break;
        }
        bounded.push(ch);
        n += 1;
    }

    ComposedText {
        text: format!("summary: {summary}\ndetail: {bounded}"),
        detail_chars: n,
        truncated,
    }
}

/// Heuristic: is this detail text too low-signal to be worth indexing?
///
/// Generic by design (length, alphanumeric density, repetition) — whether to
/// *consult* this heuristic at all is the caller's policy.
pub fn is_low_signal_detail(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() < 80 {
        return true;
    }
    let total = trimmed.chars().count();
    let alnum = trimmed.chars().filter(|c| c.is_alphanumeric()).count();
    if (alnum as f32) / (total as f32) < 0.4 {
        return true;
    }
    // Repetition: a single distinct non-empty line repeated throughout.
    let lines: Vec<&str> = trimmed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() >= 3 {
        let first = lines[0];
        if lines.iter().all(|l| *l == first) {
            return true;
        }
    }
    false
}

/// Sanitize a raw detail source for indexing: collapse whitespace, truncate
/// oversized code fences, redact secret-shaped content.
pub fn sanitize_detail(raw: &str) -> String {
    let mut out_lines: Vec<String> = Vec::new();
    let mut fence_open = false;
    let mut fence_kept = 0usize;
    let mut fence_truncating = false;
    let mut in_pem_block = false;
    let mut blank_run = 0usize;

    for line in raw.lines() {
        let line = collapse_spaces(line);
        let trimmed = line.trim();

        // PEM / private key blocks: drop wholesale.
        if !in_pem_block && trimmed.starts_with("-----BEGIN") && trimmed.contains("KEY") {
            in_pem_block = true;
            out_lines.push(format!("{REDACTED} key block"));
            continue;
        }
        if in_pem_block {
            if trimmed.starts_with("-----END") {
                in_pem_block = false;
            }
            continue;
        }

        // Code fences: keep at most MAX_FENCE_LINES per block.
        if trimmed.starts_with("```") {
            if fence_open {
                fence_open = false;
                fence_truncating = false;
            } else {
                fence_open = true;
                fence_kept = 0;
            }
            out_lines.push(redact_line(&line));
            continue;
        }
        if fence_open {
            fence_kept += 1;
            if fence_kept > MAX_FENCE_LINES {
                if !fence_truncating {
                    fence_truncating = true;
                    out_lines.push(FENCE_TRUNCATED.to_string());
                }
                continue;
            }
        }

        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
            out_lines.push(String::new());
            continue;
        }
        blank_run = 0;
        out_lines.push(redact_line(&line));
    }

    out_lines.join("\n").trim().to_string()
}

fn collapse_spaces(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_ws = false;
    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            in_ws = false;
            out.push(ch);
        }
    }
    out
}

/// Keys whose assigned values are always redacted (`key=value`, `key: value`).
const SENSITIVE_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "access_key",
    "access-key",
    "private_key",
    "private-key",
    "secret",
    "token",
    "password",
    "passwd",
    "auth",
    "authorization",
];

/// Prefixes that mark a token as a credential regardless of context.
const SECRET_PREFIXES: &[&str] = &[
    "sk-", "ghp_", "gho_", "ghu_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "xoxa-", "xoxs-",
];

fn redact_line(line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut redact_next = false; // set by a preceding `Bearer` / sensitive key
    for tok in line.split(' ') {
        if redact_next {
            // A bare `=` / `:` between key and value keeps the redaction armed
            // (`password = hunter2` tokenizes as three tokens).
            if tok == "=" || tok == ":" {
                out.push(tok.to_string());
                continue;
            }
            if looks_credential_value(tok) {
                out.push(REDACTED.to_string());
                redact_next = false;
                continue;
            }
            redact_next = false;
        }

        let lower = tok.to_lowercase();
        let bare = lower.trim_matches(|c: char| !c.is_alphanumeric());

        // `Bearer <token>` (any case) — redact the following token.
        if bare == "bearer" {
            out.push(tok.to_string());
            redact_next = true;
            continue;
        }

        // Bare sensitive key (`password`, `token`, …) — arm redaction for the
        // value that follows a standalone separator.
        if SENSITIVE_KEYS.contains(&bare) {
            out.push(tok.to_string());
            redact_next = true;
            continue;
        }

        // `key=value` / `key:value` with a sensitive key — keep key, drop value.
        if let Some(redacted) = redact_assignment(tok, &lower) {
            out.push(redacted);
            // `key:` / `key=` with the value as the NEXT token.
            if tok.ends_with('=') || tok.ends_with(':') {
                redact_next = true;
            }
            continue;
        }

        if is_secret_token(tok, &lower) {
            out.push(REDACTED.to_string());
            continue;
        }

        out.push(tok.to_string());
    }
    out.join(" ")
}

fn redact_assignment(tok: &str, lower: &str) -> Option<String> {
    for sep in ['=', ':'] {
        if let Some(pos) = tok.find(sep) {
            let key = &lower[..pos];
            let key = key.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if SENSITIVE_KEYS.iter().any(|k| key.ends_with(k)) {
                let value = &tok[pos + 1..];
                if value.is_empty() {
                    return Some(tok.to_string()); // value in next token
                }
                return Some(format!("{}{sep}{REDACTED}", &tok[..pos]));
            }
        }
    }
    None
}

fn looks_credential_value(tok: &str) -> bool {
    let core: String = tok
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '/' | '='))
        .collect();
    core.chars().count() >= 8
}

fn is_secret_token(tok: &str, lower: &str) -> bool {
    let bare = tok.trim_matches(|c: char| {
        !c.is_alphanumeric() && !matches!(c, '-' | '_' | '.' | '+' | '/' | '=')
    });
    let bare_lower = lower.trim_matches(|c: char| {
        !c.is_alphanumeric() && !matches!(c, '-' | '_' | '.' | '+' | '/' | '=')
    });

    if SECRET_PREFIXES.iter().any(|p| bare_lower.starts_with(p))
        && bare.chars().count() >= 12
    {
        return true;
    }

    // AWS access key id: AKIA + 16 uppercase alphanumerics.
    if bare.len() == 20 && bare.starts_with("AKIA")
        && bare[4..].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return true;
    }

    // JWT-shaped: eyJ…<dot>…<dot>… (three base64url segments).
    if bare.starts_with("eyJ") && bare.matches('.').count() >= 2 && bare.len() >= 24 {
        return true;
    }

    // Long hex (>= 40) — digests are fine to drop from detail text too.
    let hex_len = bare.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    if hex_len >= 40 && hex_len == bare.chars().count() {
        return true;
    }

    // Long base64-ish blob (>= 32 chars of base64 alphabet, mixed case+digit).
    let b64_alphabet = bare
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '-' | '_'));
    if b64_alphabet && bare.chars().count() >= 32 {
        let has_upper = bare.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = bare.chars().any(|c| c.is_ascii_lowercase());
        let has_digit = bare.chars().any(|c| c.is_ascii_digit());
        if (has_upper as u8 + has_lower as u8 + has_digit as u8) >= 2 {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_plain_summary_when_detail_empty() {
        let c = compose_rich_text("breve riassunto", "   ", 800);
        assert_eq!(c.text, "breve riassunto");
        assert_eq!(c.detail_chars, 0);
        assert!(!c.truncated);
    }

    #[test]
    fn compose_bounds_detail_and_reports_truncation() {
        let detail = "x".repeat(1000);
        let c = compose_rich_text("s", &detail, 800);
        assert!(c.text.starts_with("summary: s\ndetail: "));
        assert_eq!(c.detail_chars, 800);
        assert!(c.truncated);
    }

    #[test]
    fn sanitize_redacts_bearer_and_assignments() {
        let raw = "chiamata con Authorization: Bearer abc123def456ghi789 e poi\n\
                   api_key=AbCdEf123456 token: SuperSecretValue99 password = hunter2hunter2";
        let s = sanitize_detail(raw);
        assert!(!s.contains("abc123def456ghi789"), "{s}");
        assert!(!s.contains("AbCdEf123456"), "{s}");
        assert!(!s.contains("SuperSecretValue99"), "{s}");
        assert!(!s.contains("hunter2hunter2"), "{s}");
        assert!(s.contains("[redacted]"));
    }

    #[test]
    fn sanitize_redacts_known_credential_prefixes() {
        for tok in [
            "sk-proj-abcdefghijklmnop123456",
            "ghp_abcdefghijklmnopqrstuvwxyz1234",
            "github_pat_11ABCDEF0abcdefghijklmn",
            "xoxb-1234567890-abcdefghijklm",
        ] {
            let s = sanitize_detail(&format!("uso il token {tok} per la call"));
            assert!(!s.contains(tok), "non redatto: {tok} -> {s}");
        }
    }

    #[test]
    fn sanitize_redacts_aws_jwt_hex_base64() {
        let raw = "key AKIAIOSFODNN7EXAMPLE jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9P \
                   sha 0123456789abcdef0123456789abcdef01234567 blob QWJjRGVmR2hpSmtsTW5vUHFyU3R1dndYeXowMTIzNDU2Nzg5";
        let s = sanitize_detail(raw);
        assert!(!s.contains("AKIAIOSFODNN7EXAMPLE"), "{s}");
        assert!(!s.contains("eyJhbGciOiJIUzI1NiJ9"), "{s}");
        assert!(!s.contains("0123456789abcdef0123456789abcdef01234567"), "{s}");
        assert!(!s.contains("QWJjRGVmR2hpSmtsTW5vUHFyU3R1dndYeXowMTIzNDU2Nzg5"), "{s}");
    }

    #[test]
    fn sanitize_drops_pem_blocks() {
        let raw = "prima\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----\ndopo";
        let s = sanitize_detail(raw);
        assert!(!s.contains("MIIEowIBAAKCAQEA"), "{s}");
        assert!(s.contains("prima") && s.contains("dopo"));
    }

    #[test]
    fn sanitize_caps_code_fences_marker_not_secretlike() {
        let body: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let raw = format!("testo\n```\n{body}```\ncoda");
        let s = sanitize_detail(&raw);
        assert!(s.contains("[code truncated]"));
        assert!(!s.contains("line 20"), "{s}");
        assert!(s.contains("line 5"));
        // The marker itself must survive a second sanitize pass untouched.
        assert_eq!(sanitize_detail("[code truncated]"), "[code truncated]");
    }

    #[test]
    fn sanitize_preserves_ordinary_technical_text() {
        let raw = "fix in retry_backoff.rs: la funzione reconnect_with_jitter ora rispetta max_attempts=5 \
                   e logga su tracing::warn; vedi commit nel branch develop";
        let s = sanitize_detail(raw);
        assert!(s.contains("retry_backoff.rs"));
        assert!(s.contains("reconnect_with_jitter"));
        assert!(s.contains("max_attempts=5"));
    }

    #[test]
    fn low_signal_detail_gate() {
        assert!(is_low_signal_detail("breve"));
        assert!(is_low_signal_detail("ok ok ok\nok ok ok\nok ok ok")); // corto
        assert!(is_low_signal_detail(&"=*=-".repeat(50))); // densità alfanumerica bassa
        let line = "riga identica ripetuta che non aggiunge informazione utile";
        assert!(is_low_signal_detail(&format!("{line}\n{line}\n{line}")));
        assert!(!is_low_signal_detail(
            "Analisi del bug nel modulo di retry: la backoff window cresceva senza limite \
             perche' il moltiplicatore non veniva azzerato dopo un successo."
        ));
    }

    #[test]
    fn rich_text_round_trip_index_search_secret_not_retrievable() {
        use crate::config::MsaConfig;
        use crate::index::CollectionRegistry;
        use crate::schema::{ChunkConfig, Document};
        use std::collections::HashSet;

        let dir = tempfile::tempdir().expect("tempdir");
        let config = MsaConfig {
            storage: crate::config::StorageConfig {
                storage_dir: dir.path().to_path_buf(),
            },
            chunking: ChunkConfig {
                chunk_size: 16,
                overlap: 0,
            },
            ..Default::default()
        };
        let registry = CollectionRegistry::new();
        let idx = registry
            .open_or_create("enrich-rt", &config.storage.storage_dir, &config.chunking)
            .expect("collection");

        let raw = "il fix vive in reconnect_with_jitter e il token usato era \
                   Bearer abcdef1234567890SECRET ma non deve essere indicizzato";
        let composed = compose_rich_text("fix websocket reconnect", &sanitize_detail(raw), 800);
        idx.index_document(
            &Document {
                id: "d1".into(),
                text: composed.text,
                metadata: Default::default(),
                created_at: chrono::Utc::now(),
            },
            None,
        )
        .expect("index");

        // Positivo: il contenuto del detail e' retrievabile.
        let hits = idx
            .search_excluding("reconnect_with_jitter", 3, None, &HashSet::new())
            .expect("search");
        assert!(hits.iter().any(|h| h.doc_id == "d1"));

        // Negativo: il segreto non e' MAI retrievabile.
        let hits = idx
            .search_excluding("abcdef1234567890SECRET", 3, None, &HashSet::new())
            .expect("search");
        assert!(hits.is_empty(), "il segreto e' retrievabile: {hits:?}");
    }
}
