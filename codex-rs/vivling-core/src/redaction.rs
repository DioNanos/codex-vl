//! Secret redaction for Vivling memory and prompts.
//!
//! Replaces high-value credential patterns with categorical markers BEFORE
//! anything is indexed into MSA, distilled into summaries, or sent to a
//! remote LLM. Policy: prefer false positives (over-redaction) over leaks.
//! See design doc §11.4.
//!
//! The high-entropy fallback is intentionally conservative: hex-only strings
//! (typical of SHA-256 / blake3 commit hashes) have ~4 bits/char and must
//! not trigger, while base64-ish 40+ char strings cross the 4.5 bits/char
//! threshold and do trigger.

use once_cell::sync::Lazy;
use regex::Regex;

/// Single redaction rule: a compiled regex paired with the marker that
/// replaces every match.
struct Rule {
    regex: Regex,
    marker: &'static str,
}

impl Rule {
    fn new(pattern: &str, marker: &'static str) -> Self {
        Self {
            regex: Regex::new(pattern).expect("redaction regex must compile"),
            marker,
        }
    }
}

static RULES: Lazy<Vec<Rule>> = Lazy::new(|| {
    vec![
        // PEM private keys (multiline). Match first so the embedded base64 body
        // is not partially consumed by other rules.
        Rule::new(
            r"(?s)-----BEGIN (?:RSA |EC |DSA )?PRIVATE KEY-----.*?-----END (?:RSA |EC |DSA )?PRIVATE KEY-----",
            "[REDACTED:PEM_PRIVATE_KEY]",
        ),
        // Anthropic
        Rule::new(r"sk-ant-[a-zA-Z0-9_-]{40,}", "[REDACTED:ANTHROPIC_KEY]"),
        // OpenAI (project keys + classic)
        Rule::new(r"sk-(?:proj-)?[a-zA-Z0-9_-]{40,}", "[REDACTED:OPENAI_KEY]"),
        // AWS access key id
        Rule::new(r"(?:AKIA|ASIA)[A-Z0-9]{16}", "[REDACTED:AWS_KEY]"),
        // GitHub PAT classic / fine-grained
        Rule::new(r"github_pat_[a-zA-Z0-9_]{82,}", "[REDACTED:GITHUB_PAT]"),
        Rule::new(
            r"\b(?:ghp|gho|ghu|ghs|ghr)_[a-zA-Z0-9]{36,}\b",
            "[REDACTED:GITHUB_PAT]",
        ),
        // Google API key
        Rule::new(r"\bAIza[a-zA-Z0-9_-]{35}\b", "[REDACTED:GOOGLE_KEY]"),
        // Slack
        Rule::new(
            r"\bxox[abprs]-[a-zA-Z0-9-]{10,48}",
            "[REDACTED:SLACK_TOKEN]",
        ),
        // Stripe
        Rule::new(r"\bsk_live_[a-zA-Z0-9]{24,}", "[REDACTED:STRIPE_KEY]"),
        // npm fine-grained
        Rule::new(r"\bnpm_[a-zA-Z0-9]{36}\b", "[REDACTED:NPM_TOKEN]"),
        // npm legacy authToken in .npmrc-style lines
        Rule::new(r"_authToken=[^\s]+", "_authToken=[REDACTED:NPM_TOKEN]"),
        // JWT-ish three-segment dot-separated tokens
        Rule::new(
            r"\beyJ[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\.[a-zA-Z0-9_-]{10,}\b",
            "[REDACTED:JWT]",
        ),
        // Z.AI / GLM
        Rule::new(r"\b(?:zai_|glm_)[a-zA-Z0-9]{32,}\b", "[REDACTED:ZAI_KEY]"),
        // Telegram bot token (numeric id : token)
        Rule::new(
            r"\b[0-9]{8,10}:[A-Za-z0-9_-]{35,}\b",
            "[REDACTED:TELEGRAM_BOT]",
        ),
        // Forgejo / Gitea tokens (named)
        Rule::new(
            r"\b(?:forge|gitea)_[a-zA-Z0-9_-]{40,}\b",
            "[REDACTED:FORGE_TOKEN]",
        ),
        // Apple APNs .p8 auth keys (filename pattern; the key body itself is
        // a PEM block already caught above)
        Rule::new(r"\bAuthKey_[A-Z0-9]{10}\.p8\b", "[REDACTED:APPLE_P8]"),
        // Generic Bearer / Authorization headers
        Rule::new(
            r"(?i)(?:Bearer|Authorization:\s*Bearer)\s+[a-zA-Z0-9_.\-]{20,}",
            "[REDACTED:BEARER]",
        ),
        // Email: redact local-part, keep domain for audit/grouping
        Rule::new(
            r"\b([a-zA-Z0-9._%+-]+)@([a-zA-Z0-9.-]+\.[a-zA-Z]{2,})\b",
            "[REDACTED:EMAIL]@$2",
        ),
        // Filesystem paths that strongly suggest secrets at rest
        Rule::new(
            r"/(?:secrets|credentials\.json)\b|\.env(?:\.[a-z]+)?\b|\.key\b|\.pem\b",
            "[REDACTED:SECRET_PATH]",
        ),
    ]
});

/// Redact secrets in `input` and return a new owned `String`.
///
/// Two-pass strategy: first the explicit category rules (which often have
/// strong syntactic anchors), then a conservative high-entropy sweep over
/// any remaining long opaque tokens.
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for rule in RULES.iter() {
        out = rule.regex.replace_all(&out, rule.marker).to_string();
    }
    high_entropy_sweep(&out)
}

/// Replace any whitespace-bounded token of length >= 40 whose Shannon
/// entropy exceeds 4.5 bits/char with `[REDACTED:HIGH_ENTROPY]`.
///
/// Hex-only tokens (`[a-f0-9]+`) are exempted because SHA-256 / blake3
/// commit hashes are common in legitimate context and their alphabet
/// caps entropy at log2(16) = 4 bits/char.
fn high_entropy_sweep(input: &str) -> String {
    static TOKEN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"[A-Za-z0-9+/=_-]{40,}").expect("token regex"));
    TOKEN
        .replace_all(input, |caps: &regex::Captures<'_>| {
            let token = &caps[0];
            if is_hex_only(token) {
                return token.to_string();
            }
            if shannon_entropy_bits_per_char(token) >= 4.5 {
                return "[REDACTED:HIGH_ENTROPY]".to_string();
            }
            token.to_string()
        })
        .to_string()
}

fn is_hex_only(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

fn shannon_entropy_bits_per_char(s: &str) -> f64 {
    let len = s.chars().count();
    if len == 0 {
        return 0.0;
    }
    let mut counts = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    let len_f = len as f64;
    counts
        .values()
        .map(|&c| {
            let p = c as f64 / len_f;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_key() {
        let out =
            redact_secrets("Use sk-ant-api03-abc123def456ghi789jkl012mno345pqr678stu901 here");
        assert!(out.contains("[REDACTED:ANTHROPIC_KEY]"));
        assert!(!out.contains("sk-ant-"));
    }

    #[test]
    fn redacts_openai_project_key() {
        let out = redact_secrets(
            "export OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz0123456789ABCD",
        );
        assert!(out.contains("[REDACTED:OPENAI_KEY]"));
    }

    #[test]
    fn redacts_aws_keys() {
        let akia = redact_secrets("token AKIAIOSFODNN7EXAMPLE rest");
        assert!(akia.contains("[REDACTED:AWS_KEY]"));
        let asia = redact_secrets("token ASIAXXXXXXXXXXXXXXXX rest");
        assert!(asia.contains("[REDACTED:AWS_KEY]"));
    }

    #[test]
    fn redacts_github_pat_classic_and_finegrained() {
        let classic = redact_secrets("ghp_1234567890abcdefghijklmnopqrstuvwxyzAB");
        assert!(classic.contains("[REDACTED:GITHUB_PAT]"));
        let fine = redact_secrets(
            "github_pat_11AABBCCDD0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        assert!(fine.contains("[REDACTED:GITHUB_PAT]"));
    }

    #[test]
    fn redacts_google_api_key() {
        // Google API keys are exactly 39 chars: literal "AIza" + 35 alphanum/_-.
        let out = redact_secrets("key=AIzaSyAabcdefghijklmnopqrstuvwxyz012345");
        assert!(out.contains("[REDACTED:GOOGLE_KEY]"), "got: {out}");
    }

    #[test]
    fn redacts_pem_private_key_multiline() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...body...\n-----END RSA PRIVATE KEY-----";
        let out = redact_secrets(input);
        assert!(out.contains("[REDACTED:PEM_PRIVATE_KEY]"));
        assert!(!out.contains("MIIEowIBAAKCAQEA"));
    }

    #[test]
    fn redacts_telegram_bot_token() {
        let out = redact_secrets("TELEGRAM=123456789:ABCdefGHIjklMNOpqrSTUvwxYZ0123456789AB");
        assert!(out.contains("[REDACTED:TELEGRAM_BOT]"));
    }

    #[test]
    fn redacts_jwt_like_token() {
        let out = redact_secrets(
            "Authorization: Bearer eyJabc123def456.eyJpc3MiOiJtZSI.signature_part_here_longer",
        );
        // Either JWT or Bearer rule fires; both must redact.
        assert!(
            out.contains("[REDACTED:JWT]") || out.contains("[REDACTED:BEARER]"),
            "expected redacted token, got: {out}"
        );
        assert!(!out.contains("eyJabc123def456"));
    }

    #[test]
    fn email_local_part_redacted_domain_preserved() {
        let out = redact_secrets("contact davide@mmmbuto.com for details");
        assert_eq!(out, "contact [REDACTED:EMAIL]@mmmbuto.com for details");
    }

    #[test]
    fn high_entropy_fallback_does_not_redact_hex_hash() {
        let commit = "commit a2d1928d17d69fafe38ec727fa61927a25b12009 by user";
        assert_eq!(redact_secrets(commit), commit);
    }

    #[test]
    fn high_entropy_fallback_catches_unknown_long_secrets() {
        // A long, mixed-case, non-hex token that no specific rule matches.
        let opaque = "X9k4qH7nM3pZbV2sLwGcRfYuJiOaT5dEbN1xCv8oQwErTyUiOpAsDfGhJkL";
        let out = redact_secrets(&format!("token={opaque} end"));
        assert!(out.contains("[REDACTED:HIGH_ENTROPY]"));
    }

    #[test]
    fn secret_path_redacted() {
        let out = redact_secrets("source /home/user/.env.production");
        assert!(out.contains("[REDACTED:SECRET_PATH]"));
    }

    #[test]
    fn apple_p8_filename_redacted() {
        let out = redact_secrets("/path/to/AuthKey_ABCDE12345.p8");
        assert!(out.contains("[REDACTED:APPLE_P8]"));
    }

    #[test]
    fn redacts_forgejo_named_token() {
        let out =
            redact_secrets("export FORGE_TOKEN=forge_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(out.contains("[REDACTED:FORGE_TOKEN]"));
    }
}
