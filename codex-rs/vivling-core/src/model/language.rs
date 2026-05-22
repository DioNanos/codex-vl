//! Memory V2 Step 5.A — pure language primitives for the Axis G
//! "inheritance from user context" subsystem.
//!
//! All behaviour here is intentionally cheap and dependency-free: stop-word
//! counts + accented-character heuristics over a small rolling sample
//! window. The detector is conservative on purpose — short or ambiguous
//! samples must return `None` instead of producing a wrong tag, otherwise
//! the dispatcher would happily steer a Vivling into the wrong language
//! for the rest of a session.

use chrono::DateTime;
use chrono::Utc;

use super::types::VivlingLanguageMode;
use super::types::VivlingLanguageState;

/// Maximum rolling user samples kept in `recent_samples`. Sized to match
/// the design (§8.2 P2.10 q.10) — twenty messages is enough to catch a
/// dominant language without paying for unbounded growth.
pub const MAX_RECENT_SAMPLES: usize = 20;

/// Minimum number of stop-word hits required before a single short
/// sample is allowed to produce a verdict. Below this we fall through to
/// the accented-character signal and ultimately to `None`.
const MIN_STOPWORD_HITS: usize = 2;

/// Length threshold (in chars) above which a dominant stop-word language
/// is accepted with a single hit. Short messages stay conservative.
const LONG_TEXT_DOMINANCE_THRESHOLD: usize = 40;

/// Supported language tags. Kept as a static slice so callers can iterate
/// in deterministic order and we never accidentally invent a code.
pub const SUPPORTED_LANGS: &[&str] = &["it", "en", "es", "fr", "de"];

fn stopwords(lang: &str) -> &'static [&'static str] {
    // Lists are deliberately curated so the same word never appears in
    // two supported languages — otherwise short bilingual samples would
    // produce ties. English is the most likely confounder; pick stop
    // words that do not also appear in the Italian / Spanish / French /
    // German sets above.
    match lang {
        "it" => &[
            "il", "la", "le", "lo", "gli", "di", "che", "non", "una", "uno", "per", "con", "del",
            "della", "delle", "dei", "degli", "sono", "questo", "questa", "ho", "ha", "hai",
            "anche", "ma", "come", "stai", "sto", "ora", "qui", "fare", "ciao", "oggi", "domani",
            "ieri", "molto", "tutto", "bene",
        ],
        "en" => &[
            "the", "and", "of", "to", "is", "that", "this", "was", "for", "with", "are", "you",
            "but", "not", "have", "has", "had", "can", "will", "would", "should", "just", "only",
            "very", "what", "when", "where", "why", "how", "they", "them", "their",
        ],
        "es" => &[
            "el", "los", "las", "de", "que", "y", "en", "por", "para", "con", "una", "este",
            "esta", "pero", "hola", "gracias", "como", "estas", "esta", "soy", "estoy", "tengo",
            "tiene", "muy", "muchas",
        ],
        "fr" => &[
            "les", "et", "est", "un", "une", "pour", "dans", "avec", "sur", "pas", "mais", "vous",
            "nous", "je", "tu", "suis", "sommes", "ce", "ces", "cette", "merci", "bonjour",
            "salut", "oui", "non",
        ],
        "de" => &[
            "der", "die", "das", "und", "ist", "nicht", "ein", "eine", "den", "dem", "mit", "auf",
            "von", "zu", "aber", "ich", "du", "wir", "sie", "war", "sind", "haben", "kann", "wird",
            "soll", "auch",
        ],
        _ => &[],
    }
}

/// Distinctive accented / non-ASCII characters per language. Hits here
/// are a strong signal because no other supported language uses the
/// same set.
fn distinctive_chars(lang: &str) -> &'static [char] {
    match lang {
        "it" => &['à', 'è', 'é', 'ì', 'ò', 'ù'],
        "es" => &['ñ', '¿', '¡'],
        "fr" => &['ç', 'œ', 'â', 'ê', 'î', 'ô', 'û', 'ë', 'ï', 'ü'],
        "de" => &['ß', 'ä', 'ö', 'ü'],
        // English has no distinctive non-ASCII; the empty slice avoids
        // accidental "accent-vs-stopword" tie-breaks polluting en.
        _ => &[],
    }
}

/// Conservative single-text detector. Returns `None` when no language
/// clears the minimum-hit bar or two languages tie.
pub fn detect_language_code(text: &str) -> Option<&'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let len = lower.chars().count();

    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|tok| !tok.is_empty())
        .collect();

    let mut scores: Vec<(&'static str, usize)> = Vec::with_capacity(SUPPORTED_LANGS.len());
    for &lang in SUPPORTED_LANGS {
        let stop_hits = tokens
            .iter()
            .filter(|tok| stopwords(lang).contains(tok))
            .count();
        let accent_hits = lower
            .chars()
            .filter(|c| distinctive_chars(lang).contains(c))
            .count();
        // Accented characters count double because they almost never
        // appear by accident in another supported language.
        let weight = stop_hits + accent_hits.saturating_mul(2);
        if weight > 0 {
            scores.push((lang, weight));
        }
    }

    if scores.is_empty() {
        return None;
    }
    scores.sort_by(|a, b| b.1.cmp(&a.1));

    let (top_lang, top_score) = scores[0];
    // Tie at the top is ambiguous: stay silent.
    if scores.len() > 1 && scores[1].1 == top_score {
        return None;
    }

    // Below the stop-word floor we only trust the verdict when either
    // the text is long enough for a single-hit dominance to be safe,
    // or the signal came from a distinctive accented character (which
    // never appears in another supported language's distinctive set).
    if top_score >= MIN_STOPWORD_HITS {
        return Some(top_lang);
    }
    if len >= LONG_TEXT_DOMINANCE_THRESHOLD && top_score >= 1 {
        return Some(top_lang);
    }
    // Distinctive accents alone, on a short text, are enough only if
    // they are unique to one supported language (the tie check above
    // already filtered ambiguous cases).
    let accent_hits_for_top = lower
        .chars()
        .filter(|c| distinctive_chars(top_lang).contains(c))
        .count();
    if accent_hits_for_top >= 1 {
        return Some(top_lang);
    }

    None
}

/// Normalise a `LANG`-style locale string into one of [`SUPPORTED_LANGS`].
/// Returns `None` for unknown/unsupported codes so callers always fall
/// back to the design-mandated final default (`"en"`).
pub fn normalize_lang_env(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    // Strip the optional `.UTF-8` / `@modifier` suffixes, then take the
    // primary code before the country segment.
    let stripped = lower
        .split('.')
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("");
    let primary = stripped.split(['_', '-']).next()?;
    if SUPPORTED_LANGS.contains(&primary) {
        Some(primary.to_string())
    } else {
        None
    }
}

impl VivlingLanguageState {
    /// Append `text` to the rolling sample window. Whitespace-only
    /// strings are dropped so they never consume one of the bounded
    /// 20 slots.
    pub fn record_sample(&mut self, now: DateTime<Utc>, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.recent_samples.push((now, text.to_string()));
        if self.recent_samples.len() > MAX_RECENT_SAMPLES {
            let excess = self.recent_samples.len() - MAX_RECENT_SAMPLES;
            self.recent_samples.drain(0..excess);
        }
    }

    /// Recompute `detected_language` from the current rolling window.
    /// Honours the mode contract:
    /// - `Strict`: first non-empty detection sticks; later samples never
    ///   overwrite it.
    /// - `MirrorUser` / `DominantOnly`: majority of the per-sample
    ///   detections; ties or empty windows leave the previous value.
    pub fn refresh_detected_language(&mut self) {
        if matches!(self.language_mode, VivlingLanguageMode::Strict)
            && !self.detected_language.is_empty()
        {
            return;
        }

        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for (_, sample) in &self.recent_samples {
            if let Some(code) = detect_language_code(sample) {
                if let Some(entry) = counts.iter_mut().find(|(lang, _)| *lang == code) {
                    entry.1 += 1;
                } else {
                    counts.push((code, 1));
                }
            }
        }
        if counts.is_empty() {
            return;
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        if counts.len() > 1 && counts[0].1 == counts[1].1 {
            // Tie: do not flap, leave the previous verdict in place.
            return;
        }
        self.detected_language = counts[0].0.to_string();
    }

    /// Localized one-shot hint pointing the user at the dedicated chat
    /// panel (Ctrl+J). The text follows the Vivling's effective language
    /// (override → detected → system `LANG` → English fallback) so the
    /// suggestion matches the same locale used for greetings and brain
    /// replies. Supported locales: `it`, `en`, `es`, `fr`, `pt`, `de`.
    /// Unknown locales fall back to English.
    pub fn chat_panel_hint(lang: &str) -> &'static str {
        match lang {
            "it" => "Suggerimento: premi Ctrl+J per aprire la chat dedicata del Vivling \
                     (storia preservata, scroll line-based, niente clutter del thread principale).",
            "es" => "Sugerencia: pulsa Ctrl+J para abrir el chat dedicado del Vivling \
                     (historial preservado, scroll por línea, sin saturar el hilo principal).",
            "fr" => "Astuce : appuie sur Ctrl+J pour ouvrir le chat dédié du Vivling \
                     (historique préservé, défilement ligne par ligne, sans encombrer le thread principal).",
            "pt" => "Dica: pressione Ctrl+J para abrir o chat dedicado do Vivling \
                     (histórico preservado, rolagem por linha, sem poluir o thread principal).",
            "de" => "Tipp: Drücke Strg+J, um das dedizierte Vivling-Chatfenster zu öffnen \
                     (Verlauf bleibt erhalten, zeilenweises Scrollen, ohne den Hauptthread zu überladen).",
            _ => "Tip: press Ctrl+J to open the dedicated Vivling chat panel \
                  (history preserved, line-based scroll, no clutter in the main thread).",
        }
    }

    /// Resolve the language the Vivling should speak right now. Order
    /// matches design §8.2: explicit `language_override` wins, then the
    /// detected window verdict, then the optional system `LANG` after
    /// normalisation, finally `"en"` as a hard fallback.
    pub fn effective_language(&self, system_lang: Option<&str>) -> String {
        if let Some(ovr) = self
            .language_override
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return ovr.to_string();
        }
        if !self.detected_language.trim().is_empty() {
            return self.detected_language.trim().to_string();
        }
        if let Some(env_lang) = system_lang.and_then(normalize_lang_env) {
            return env_lang;
        }
        "en".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn detect_simple_italian() {
        assert_eq!(detect_language_code("ciao come stai oggi?"), Some("it"));
    }

    #[test]
    fn detect_simple_english() {
        assert_eq!(detect_language_code("hello how are you today?"), Some("en"));
    }

    #[test]
    fn detect_simple_spanish() {
        assert_eq!(
            detect_language_code("hola como estas el dia de hoy y la noche"),
            Some("es")
        );
    }

    #[test]
    fn detect_mixed_italian_with_tech_english_keeps_dominant_italian() {
        // Realistic DAG-style mix: italiano discorsivo + tech terms inglesi.
        // La maggioranza degli stopword italiani vince.
        let text =
            "ho fatto il refactor che non era pronto, controllo il loop runtime e poi il merge";
        assert_eq!(detect_language_code(text), Some("it"));
    }

    #[test]
    fn detect_unknown_short_text_returns_none() {
        assert_eq!(detect_language_code("ok"), None);
        assert_eq!(detect_language_code("xyz123"), None);
    }

    #[test]
    fn detect_empty_or_whitespace_returns_none() {
        assert_eq!(detect_language_code(""), None);
        assert_eq!(detect_language_code("   \n  "), None);
    }

    #[test]
    fn detect_distinctive_accent_short_text() {
        // Single italian distinctive char on a short sample: accepted.
        assert_eq!(detect_language_code("perché"), Some("it"));
        // Single german umlaut: accepted.
        assert_eq!(detect_language_code("schön"), Some("de"));
    }

    #[test]
    fn record_sample_is_bounded_to_max_recent_samples() {
        let mut state = VivlingLanguageState::default();
        let now = Utc::now();
        for i in 0..(MAX_RECENT_SAMPLES + 5) {
            state.record_sample(now, &format!("sample {i}"));
        }
        assert_eq!(state.recent_samples.len(), MAX_RECENT_SAMPLES);
        // FIFO: oldest must have been evicted.
        assert!(state.recent_samples[0].1.contains("sample 5"));
        assert!(
            state.recent_samples[MAX_RECENT_SAMPLES - 1]
                .1
                .contains(&format!("sample {}", MAX_RECENT_SAMPLES + 4))
        );
    }

    #[test]
    fn record_sample_drops_whitespace_only_input() {
        let mut state = VivlingLanguageState::default();
        let now = Utc::now();
        state.record_sample(now, "   \t\n  ");
        state.record_sample(now, "");
        assert!(state.recent_samples.is_empty());
    }

    #[test]
    fn effective_language_override_wins() {
        let mut state = VivlingLanguageState::default();
        state.language_override = Some("es".to_string());
        state.detected_language = "it".to_string();
        assert_eq!(state.effective_language(Some("en_US.UTF-8")), "es");
    }

    #[test]
    fn effective_language_detected_wins_over_lang() {
        let mut state = VivlingLanguageState::default();
        state.detected_language = "it".to_string();
        assert_eq!(state.effective_language(Some("en_US.UTF-8")), "it");
    }

    #[test]
    fn effective_language_normalises_lang_env() {
        let state = VivlingLanguageState::default();
        assert_eq!(state.effective_language(Some("it_IT.UTF-8")), "it");
        assert_eq!(state.effective_language(Some("fr_CA")), "fr");
        assert_eq!(state.effective_language(Some("en_GB@euro")), "en");
    }

    #[test]
    fn effective_language_falls_back_to_en() {
        let state = VivlingLanguageState::default();
        assert_eq!(state.effective_language(None), "en");
        assert_eq!(state.effective_language(Some("zz_ZZ")), "en");
        assert_eq!(state.effective_language(Some("")), "en");
    }

    #[test]
    fn refresh_detected_language_majority() {
        let mut state = VivlingLanguageState::default();
        let now = Utc::now();
        state.record_sample(now, "ciao come stai oggi?");
        state.record_sample(now, "ho fatto il refactor che non era pronto");
        state.record_sample(now, "hello how are you today?");
        state.refresh_detected_language();
        assert_eq!(state.detected_language, "it");
    }

    #[test]
    fn refresh_detected_language_preserves_previous_on_tie() {
        let mut state = VivlingLanguageState::default();
        state.detected_language = "it".to_string();
        let now = Utc::now();
        state.record_sample(now, "ciao come stai oggi?");
        state.record_sample(now, "hello how are you today?");
        state.refresh_detected_language();
        // Tie 1-1: previous "it" must stick instead of flapping.
        assert_eq!(state.detected_language, "it");
    }

    #[test]
    fn strict_mode_freezes_first_detected_language() {
        let mut state = VivlingLanguageState::default();
        state.language_mode = VivlingLanguageMode::Strict;
        let now = Utc::now();

        state.record_sample(now, "ciao come stai oggi?");
        state.refresh_detected_language();
        assert_eq!(state.detected_language, "it");

        // Massive amount of english afterwards: Strict must NOT switch.
        for _ in 0..10 {
            state.record_sample(now, "hello how are you today, the world is good");
        }
        state.refresh_detected_language();
        assert_eq!(
            state.detected_language, "it",
            "Strict mode must freeze the first detection"
        );
    }
}
