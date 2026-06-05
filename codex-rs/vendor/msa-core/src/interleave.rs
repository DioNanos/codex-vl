//! A5 — Memory Interleave bounded: stateless round-trip state + per-round knobs.
//!
//! This is the **foundation** layer (A5.1). It defines the data types that
//! cross the `msa_interleave_round` tool boundary and the validation/clamping
//! of the per-round knobs. The round logic (route → dedup → fetch bounded →
//! inject → stats/budget/stop-hints) lands in A5.2; the MCP tool wiring and
//! tier defaults in A5.3.
//!
//! **Why stateless** (design D1, Codex reversing M3): the cursor in
//! [`crate::session`] lives in process RAM and dies on MCP subprocess
//! reconnect/retry/crash, which would replay Memory Interleave from round 1.
//! Instead the state is small and the client already holds it, so the client
//! round-trips it in every request. Reconnect/crash become irrelevant — there
//! is no server-side session to lose, hence no file-lock/GC/race to manage.
//!
//! **Privacy** (design §7 risk 6): only IDs and counters ever live in the
//! state. Raw injected text never does — it would bloat the payload and echo
//! user content back through the client on every round.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{MsaError, Result};
use crate::index::MsaIndex;
use crate::schema::{ChunkHit, Document, SearchFilter};
use crate::validate::{clamp_top_k, validate_doc_id, validate_query};

/// Wire schema version of the interleave state. Bumped only on a breaking
/// change to the on-wire state shape; the client echoes it back and the server
/// rejects versions it does not understand (design §7 risk 7). Kept distinct
/// from any future on-disk session schema version.
pub const INTERLEAVE_STATE_VERSION: u32 = 1;

/// Hard cap on the number of distinct ids carried in the round-trip state.
/// With realistic tier defaults (`top_k <= 16`, `max_rounds <= 5`) the state
/// holds a few dozen ids; this bound (design §3, §7 risk 5) only fires on
/// pathological/forged input and yields a clean `invalid_params`, never a
/// crash or an unbounded echoed payload.
pub const MAX_STATE_DOC_IDS: usize = 100_000;

// ---- Server hard ceilings for the per-round knobs --------------------------
//
// Tier *defaults* (RPi3 floor vs top) are selected by the caller from config
// (A5.3). These ceilings are the absolute maximum the server will honor
// regardless of tier, so a client can never request unbounded work. They are
// generous enough that any realistic top-tier request passes unclamped.

/// Max rounds the server will ever allow in one interleave loop.
pub const MAX_ROUNDS_CEILING: u32 = 32;
/// Max characters injected from a single document in one round.
pub const MAX_CHARS_PER_DOC_CEILING: usize = 64 * 1024;
/// Max total characters injected across the whole loop (char budget proxy,
/// design D8: characters are a declared proxy for tokens, not token-accurate).
pub const MAX_TOTAL_CHARS_CEILING: usize = 1024 * 1024;
/// Max documents fetched+injected per round (also bounded by `top_k`).
pub const MAX_FETCH_TOP_N_CEILING: usize = 64;
/// Default low-gain stop threshold (design §3 example): `marginal_gain_chars`
/// below this for two consecutive rounds raises `low_gain_2_rounds`.
pub const DEFAULT_LOW_GAIN_THRESHOLD: f32 = 0.05;

// ---- Reference tier defaults for the per-round knobs -----------------------
//
// These are the *reference* (mid-tier) defaults a client gets when it omits a
// knob, matching the design §3 request example. They are the tier knob
// (design §6): a deployment lowers them in config for a Raspberry Pi 3 floor,
// or raises them for a top-tier box. The server still clamps every resolved
// value into the hard ceilings above, so config can never exceed the ceiling.

/// Default candidate pool size per round.
pub const DEFAULT_TOP_K: usize = 16;
/// Default number of fresh docs fetched+injected per round.
pub const DEFAULT_FETCH_TOP_N: usize = 4;
/// Default per-document injection cap, in characters.
pub const DEFAULT_MAX_CHARS_PER_DOC: usize = 2000;
/// Default total injection budget across the loop, in characters.
pub const DEFAULT_MAX_TOTAL_CHARS: usize = 16_000;
/// Default hard round ceiling for one loop.
pub const DEFAULT_MAX_ROUNDS: u32 = 5;

/// Dedup strategy for a round. A5 ships only per-id dedup; the enum exists so
/// future semantic modes (fingerprint/minhash, design §5) are additive and the
/// wire format never has to break (D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DedupMode {
    /// Exclude documents whose `doc_id` is already in `seen_doc_ids`.
    #[default]
    Id,
}

impl DedupMode {
    /// Wire tokens a client may pass, surfaced verbatim in error messages.
    pub const SUPPORTED: &'static [&'static str] = &["id"];

    /// Parse a client-supplied mode string. Unknown modes return a clean
    /// `InvalidInput` listing what is supported (design §3, §6), never a silent
    /// fallback to a different mode.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "id" => Ok(DedupMode::Id),
            other => Err(MsaError::InvalidInput(format!(
                "unsupported dedup_mode {other:?}; supported: {:?}",
                Self::SUPPORTED
            ))),
        }
    }
}

/// Identity of one unit of evidence the client has already been shown.
///
/// A unit is a document, optionally narrowed to a chunk. `chunk_idx = None`
/// means the whole document was injected, or the unit was compacted to
/// document granularity. Ordered (`doc_id`, then `chunk_idx` with `None`
/// before `Some`) so the serialized state is deterministic and tests stable.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EvidenceUnitId {
    pub doc_id: String,
    /// `None` = whole document (or compacted). Kept explicit in the wire form
    /// (serialized as `null`) so the saturation set is unambiguous.
    pub chunk_idx: Option<usize>,
}

impl EvidenceUnitId {
    pub fn doc(doc_id: impl Into<String>) -> Self {
        Self {
            doc_id: doc_id.into(),
            chunk_idx: None,
        }
    }

    pub fn chunk(doc_id: impl Into<String>, chunk_idx: usize) -> Self {
        Self {
            doc_id: doc_id.into(),
            chunk_idx: Some(chunk_idx),
        }
    }
}

/// Stateless round-trip state for Memory Interleave (design §3).
///
/// The client receives this in each response and sends the previous one back
/// in the next request; the server holds no session. Three control concepts
/// (D3) live here: dedup (`seen_doc_ids`), saturation (`injected_units` — what
/// was *read*, not merely searched), and char budget (`used_chars_total`),
/// plus the consecutive-round counters that feed the soft stop-hints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InterleaveStateV1 {
    /// Wire schema version. Must equal [`INTERLEAVE_STATE_VERSION`].
    pub schema_version: u32,
    /// Number of completed rounds reflected in this state (0 = none yet).
    pub round: u32,
    /// Doc ids already **injected (read)** in prior rounds, for per-id dedup
    /// (D3/D7). A budget-skipped or fetch-failed candidate is NOT recorded here,
    /// so it stays re-surfaceable next round.
    pub seen_doc_ids: Vec<String>,
    /// Evidence units already injected (read), for saturation tracking.
    pub injected_units: Vec<EvidenceUnitId>,
    /// Running total of injected characters across all rounds (char budget).
    pub used_chars_total: usize,
    /// Consecutive rounds that injected nothing new (feeds `exhausted_2_rounds`).
    pub empty_rounds: u8,
    /// Consecutive rounds below the low-gain threshold (feeds `low_gain_2_rounds`).
    pub low_gain_rounds: u8,
}

impl InterleaveStateV1 {
    /// The starting state for round 1 (equivalent to the client sending `null`).
    pub fn fresh() -> Self {
        Self {
            schema_version: INTERLEAVE_STATE_VERSION,
            round: 0,
            seen_doc_ids: Vec::new(),
            injected_units: Vec::new(),
            used_chars_total: 0,
            empty_rounds: 0,
            low_gain_rounds: 0,
        }
    }

    /// Resolve the incoming `state` field: `None` (client sent `null`) becomes a
    /// fresh round-1 state; otherwise the provided state is validated. This is
    /// the single entry point the round handler (A5.2) uses to obtain a trusted
    /// state from an untrusted request.
    pub fn resolve(state: Option<InterleaveStateV1>) -> Result<InterleaveStateV1> {
        match state {
            None => Ok(InterleaveStateV1::fresh()),
            Some(s) => {
                s.validate()?;
                Ok(s)
            }
        }
    }

    /// Validate a client-supplied state. The state crosses an untrusted boundary
    /// (the client could forge it), so we reject an unknown schema version,
    /// oversized id collections, and malformed doc ids — each as a clean
    /// `InvalidInput`, never an internal error.
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != INTERLEAVE_STATE_VERSION {
            return Err(MsaError::InvalidInput(format!(
                "unsupported interleave state schema_version {} (expected {INTERLEAVE_STATE_VERSION})",
                self.schema_version
            )));
        }
        // Length checks first: cheap, and they bound the per-id loop below.
        if self.seen_doc_ids.len() > MAX_STATE_DOC_IDS {
            return Err(MsaError::InvalidInput(format!(
                "interleave state too large: {} seen_doc_ids (max {MAX_STATE_DOC_IDS})",
                self.seen_doc_ids.len()
            )));
        }
        if self.injected_units.len() > MAX_STATE_DOC_IDS {
            return Err(MsaError::InvalidInput(format!(
                "interleave state too large: {} injected_units (max {MAX_STATE_DOC_IDS})",
                self.injected_units.len()
            )));
        }
        for id in &self.seen_doc_ids {
            validate_doc_id(id)?;
        }
        for u in &self.injected_units {
            validate_doc_id(&u.doc_id)?;
        }
        Ok(())
    }

    /// Sort and de-duplicate the id collections so the serialized state is
    /// byte-stable regardless of insertion order (design §3: deterministic
    /// output, stable tests). The round handler (A5.2) must call this before
    /// emitting the state in a response.
    pub fn canonicalize(&mut self) {
        self.seen_doc_ids.sort_unstable();
        self.seen_doc_ids.dedup();
        self.injected_units.sort_unstable();
        self.injected_units.dedup();
    }

    /// Materialize `seen_doc_ids` as a set for O(1) dedup lookups during a round.
    pub fn seen_doc_id_set(&self) -> HashSet<String> {
        self.seen_doc_ids.iter().cloned().collect()
    }

    /// Materialize `injected_units` as a set for O(1) saturation lookups.
    pub fn injected_unit_set(&self) -> HashSet<EvidenceUnitId> {
        self.injected_units.iter().cloned().collect()
    }
}

impl Default for InterleaveStateV1 {
    fn default() -> Self {
        Self::fresh()
    }
}

/// Validated, clamped per-round knobs for `msa_interleave_round`.
///
/// Construct via [`InterleaveParams::clamped`], which clamps each knob into its
/// server ceiling and enforces the `fetch_top_n <= top_k` invariant. Default
/// *selection* (including tier defaults, design §6) is the caller's job (A5.3);
/// this type only enforces the absolute server bounds. `dense_alpha` is
/// intentionally absent: it is validated by the search layer
/// (`search_hybrid` rejects out-of-range / missing-scorer with a clean error),
/// so duplicating that here would risk drift.
#[derive(Debug, Clone)]
pub struct InterleaveParams {
    pub top_k: usize,
    pub fetch_top_n: usize,
    pub max_chars_per_doc: usize,
    pub max_total_chars: usize,
    pub max_rounds: u32,
    pub low_gain_threshold: f32,
    pub dedup_mode: DedupMode,
}

impl InterleaveParams {
    /// Clamp concrete knob values into the server's hard ranges and enforce
    /// cross-field invariants. Defaults (incl. tier defaults) must already be
    /// applied by the caller; here we only enforce ceilings, floors, and
    /// `fetch_top_n <= top_k`. Clamping (not rejecting) keeps retrieval working
    /// when a client passes something absurd, mirroring [`clamp_top_k`].
    #[allow(clippy::too_many_arguments)]
    pub fn clamped(
        top_k: usize,
        fetch_top_n: usize,
        max_chars_per_doc: usize,
        max_total_chars: usize,
        max_rounds: u32,
        low_gain_threshold: f32,
        dedup_mode: DedupMode,
    ) -> Self {
        let top_k = clamp_top_k(top_k);
        let fetch_top_n = fetch_top_n.clamp(1, MAX_FETCH_TOP_N_CEILING).min(top_k);
        let max_chars_per_doc = max_chars_per_doc.clamp(1, MAX_CHARS_PER_DOC_CEILING);
        let max_total_chars = max_total_chars.clamp(1, MAX_TOTAL_CHARS_CEILING);
        let max_rounds = max_rounds.clamp(1, MAX_ROUNDS_CEILING);
        Self {
            top_k,
            fetch_top_n,
            max_chars_per_doc,
            max_total_chars,
            max_rounds,
            low_gain_threshold: clamp_low_gain_threshold(low_gain_threshold),
            dedup_mode,
        }
    }
}

/// Clamp the low-gain threshold into `[0.0, 1.0]`. `marginal_gain_chars` is a
/// ratio in `[0, 1]`, so the threshold lives in the same range. A non-finite
/// value (NaN/inf, e.g. from a malformed client) falls back to the default
/// rather than poisoning the comparison.
fn clamp_low_gain_threshold(t: f32) -> f32 {
    if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        DEFAULT_LOW_GAIN_THRESHOLD
    }
}

// ===========================================================================
// A5.2 — round logic and response payload
// ===========================================================================

/// One document injected into the round's evidence, with its original text
/// bounded to the per-doc character cap. Serialize-only: this is a response
/// payload, never deserialized from a client.
#[derive(Debug, Clone, Serialize)]
pub struct InjectedDoc {
    pub doc_id: String,
    /// Original document text, bounded to `max_chars_per_doc` (and to the
    /// remaining total budget). The bounded *original* text is the heart of
    /// MSA (paper Tab.4: injection −37% if absent), not a re-generated summary.
    pub text: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    /// Characters actually injected (after bounding).
    pub chars: usize,
    /// True if the original text was longer than the applied cap.
    pub truncated: bool,
    /// `chunk_idx` of the top-ranked hit that surfaced this document, for the
    /// client to correlate evidence with the routing result.
    pub source_hit_chunk_idx: Option<usize>,
}

/// Per-round telemetry (design §3). All transparency, no enforcement: the
/// client reads these to decide the semantic stop.
#[derive(Debug, Clone, Serialize)]
pub struct RoundStats {
    /// Candidate chunk hits examined this round (`<= top_k`).
    pub candidates_considered: usize,
    /// Candidate chunks whose document was already injected in a prior round.
    pub dedup_skipped_count: usize,
    /// Fresh documents that could not be injected because the char budget was
    /// exhausted (within the `fetch_top_n` picks for this round).
    pub budget_skipped_count: usize,
    /// Characters injected this round.
    pub new_chars_injected: usize,
    /// `new_chars_injected / max(1, used_chars_total)`. **Monotonic-decreasing
    /// by construction** (round 1 is ~1.0, then it falls as the budget fills);
    /// a low value is the loop filling up, NOT a malfunction — do not read it as
    /// "the loop is broken".
    pub marginal_gain_chars: f32,
    /// `dedup_skipped_count / max(1, candidates_considered)`. A **saturation
    /// signal, not a quality metric**: a high value means the candidate pool has
    /// collapsed onto already-read docs (M3) and is *informative*, not bad — the
    /// client uses it to avoid concluding prematurely, never as a score to
    /// minimize.
    pub dedup_ratio: f32,
}

/// Character-budget accounting for the round (design §3). `chars` are a
/// declared proxy for tokens (D8), not token-accurate.
#[derive(Debug, Clone, Serialize)]
pub struct RoundBudget {
    pub max_total_chars: usize,
    pub used_chars_total: usize,
    pub remaining_chars: usize,
    pub max_chars_per_doc: usize,
    /// Number of injected docs whose text was truncated to fit a cap.
    pub truncated_docs: usize,
}

/// Stop signals (design §4). Hard stops are server-enforced; the rest are hints
/// the client weighs against its own "enough to answer" judgment.
#[derive(Debug, Clone, Serialize)]
pub struct StopHints {
    /// `round >= max_rounds` OR `used_chars_total >= max_total_chars`.
    pub hard_stop: bool,
    /// No new document was injectable this round (after search+dedup+budget).
    pub exhausted: bool,
    /// Two consecutive empty rounds (a single empty round may be BM25 noise, D6).
    pub exhausted_2_rounds: bool,
    /// Two consecutive rounds below `low_gain_threshold`.
    pub low_gain_2_rounds: bool,
    /// Human-readable reason set only when `hard_stop` is true; else `None`.
    pub reason: Option<String>,
}

/// A non-fatal per-document fetch failure (design D9). The round proceeds; a
/// document that vanished between routing and fetch (reindex/delete race,
/// §7 risk 1/3) is reported here rather than aborting the whole round.
#[derive(Debug, Clone, Serialize)]
pub struct FetchError {
    pub doc_id: String,
    pub reason: String,
}

/// Full response of one `msa_interleave_round` call (design §3). Serialize-only.
#[derive(Debug, Clone, Serialize)]
pub struct RoundResponse {
    pub round: u32,
    pub hits: Vec<ChunkHit>,
    pub injected_docs: Vec<InjectedDoc>,
    /// The updated, canonicalized state the client must echo back next round.
    pub state: InterleaveStateV1,
    pub stats: RoundStats,
    pub budget: RoundBudget,
    pub stop_hints: StopHints,
    pub fetch_errors: Vec<FetchError>,
}

/// Bound `text` to at most `max_chars` Unicode scalar values, single-pass so a
/// large document is not fully scanned. Returns `(bounded_text, truncated,
/// char_count)`.
fn bound_text(text: &str, max_chars: usize) -> (String, bool, usize) {
    let mut out = String::new();
    let mut n = 0usize;
    for ch in text.chars() {
        if n >= max_chars {
            return (out, true, n);
        }
        out.push(ch);
        n += 1;
    }
    (out, false, n)
}

/// Outcome of the fetch+inject phase of a round. Split out (with the fetcher as
/// a closure) so the non-fatal fetch-failure handling (D9) is testable without
/// a live index — `MsaIndex` is not a trait, so a failing fetch cannot be
/// mocked at the index boundary otherwise.
struct InjectResult {
    injected_docs: Vec<InjectedDoc>,
    fetch_errors: Vec<FetchError>,
    used_chars_total: usize,
    new_chars: usize,
    budget_skipped_count: usize,
    truncated_docs: usize,
    newly_injected_ids: Vec<String>,
}

/// Fetch and inject the fresh documents (up to `fetch_top_n`) within the char
/// budget, via the `fetch` closure. A fetch failure is recorded in
/// `fetch_errors` and skipped (D9): the round proceeds with the docs that did
/// load, and the failed doc is *not* marked injected, so a transient failure
/// gets another chance next round. `used_start` is the running char total
/// carried from prior rounds.
fn inject_fresh<F>(
    fresh_order: &[String],
    source_chunk: &HashMap<String, usize>,
    fetch_top_n: usize,
    max_chars_per_doc: usize,
    max_total_chars: usize,
    used_start: usize,
    mut fetch: F,
) -> InjectResult
where
    F: FnMut(&str) -> Result<Document>,
{
    let mut used = used_start;
    let mut new_chars = 0usize;
    let mut budget_skipped_count = 0usize;
    let mut truncated_docs = 0usize;
    let mut injected_docs: Vec<InjectedDoc> = Vec::new();
    let mut fetch_errors: Vec<FetchError> = Vec::new();
    let mut newly_injected_ids: Vec<String> = Vec::new();

    for doc_id in fresh_order.iter().take(fetch_top_n) {
        if used >= max_total_chars {
            // Wanted this fresh doc but the total budget is spent.
            budget_skipped_count += 1;
            continue;
        }
        match fetch(doc_id) {
            Ok(doc) => {
                let remaining = max_total_chars - used;
                let cap = max_chars_per_doc.min(remaining);
                let (text, truncated, chars) = bound_text(&doc.text, cap);
                if truncated {
                    truncated_docs += 1;
                }
                used += chars;
                new_chars += chars;
                injected_docs.push(InjectedDoc {
                    doc_id: doc_id.clone(),
                    text,
                    metadata: doc.metadata,
                    created_at: doc.created_at,
                    chars,
                    truncated,
                    source_hit_chunk_idx: source_chunk.get(doc_id).copied(),
                });
                newly_injected_ids.push(doc_id.clone());
            }
            Err(e) => fetch_errors.push(FetchError {
                doc_id: doc_id.clone(),
                reason: e.to_string(),
            }),
        }
    }

    InjectResult {
        injected_docs,
        fetch_errors,
        used_chars_total: used,
        new_chars,
        budget_skipped_count,
        truncated_docs,
        newly_injected_ids,
    }
}

/// Build the no-work response for a round refused by the hard-stop preflight:
/// the incoming state is already at `max_rounds` or `max_total_chars`, so the
/// server does NO route/fetch/inject (design §4: the hard stop is
/// server-enforced, not merely advisory). The state is echoed back
/// canonicalized and otherwise unchanged — no round actually ran, so the
/// round counter and the soft-hint counters are not advanced.
fn pre_stopped_response(state: &InterleaveStateV1, params: &InterleaveParams) -> RoundResponse {
    let hard_round = state.round >= params.max_rounds;
    let hard_budget = state.used_chars_total >= params.max_total_chars;
    let reason = match (hard_round, hard_budget) {
        (true, true) => Some("max_rounds and max_total_chars already reached".to_string()),
        (true, false) => Some("max_rounds already reached".to_string()),
        (false, true) => Some("max_total_chars already reached".to_string()),
        // Unreachable: only called when at least one hard limit holds.
        (false, false) => None,
    };
    let mut echoed = state.clone();
    echoed.schema_version = INTERLEAVE_STATE_VERSION;
    echoed.canonicalize();
    RoundResponse {
        round: state.round,
        hits: Vec::new(),
        injected_docs: Vec::new(),
        state: echoed,
        stats: RoundStats {
            candidates_considered: 0,
            dedup_skipped_count: 0,
            budget_skipped_count: 0,
            new_chars_injected: 0,
            marginal_gain_chars: 0.0,
            dedup_ratio: 0.0,
        },
        budget: RoundBudget {
            max_total_chars: params.max_total_chars,
            used_chars_total: state.used_chars_total,
            remaining_chars: params.max_total_chars.saturating_sub(state.used_chars_total),
            max_chars_per_doc: params.max_chars_per_doc,
            truncated_docs: 0,
        },
        stop_hints: StopHints {
            hard_stop: true,
            exhausted: true,
            exhausted_2_rounds: state.empty_rounds >= 2,
            low_gain_2_rounds: state.low_gain_rounds >= 2,
            reason,
        },
        fetch_errors: Vec::new(),
    }
}

/// Execute one Memory Interleave round (design §1, §3, §4).
///
/// Contract: `state` must already be the resolved/validated state
/// ([`InterleaveStateV1::resolve`]) and `params` already clamped
/// ([`InterleaveParams::clamped`]); this function enforces neither, so the
/// caller (the MCP tool in A5.3) owns input hygiene.
///
/// Pipeline: **route** (BM25 / hybrid over `top_k`, no retrieval-time exclude
/// so dedup is *visible*) → **dedup** per-id against documents injected in
/// prior rounds → **fetch** the top fresh docs (≤ `fetch_top_n`) → **inject**
/// their original text bounded by the per-doc and total char budgets →
/// telemetry + updated state. A document already injected in a prior round is
/// skipped (counted in `dedup_skipped_count`); the running query is the
/// *client's* reformulation each round, so dedup measures genuine cross-round
/// overlap, not the same query re-run.
///
/// Fatal only if routing fails (index open / query parse / bad `dense_alpha`);
/// a per-document fetch failure is collected in `fetch_errors` and the round
/// continues (D9).
pub fn run_round(
    index: &MsaIndex,
    query: &str,
    state: &InterleaveStateV1,
    params: &InterleaveParams,
    filter: Option<&SearchFilter>,
    dense_alpha: f32,
    query_emb: Option<&[f32]>,
) -> Result<RoundResponse> {
    validate_query(query)?;

    // Hard-stop preflight (design §4: the hard stop is server-ENFORCED, not just
    // advisory). If the incoming state is already at a hard limit, do no work —
    // a client that ignores a prior `hard_stop` and calls again must not be able
    // to drive more retrieval/injection past the budget.
    if state.round >= params.max_rounds || state.used_chars_total >= params.max_total_chars {
        return Ok(pre_stopped_response(state, params));
    }

    // Route. No retrieval-time exclude: we dedup afterward so dedup_ratio is an
    // honest, observable signal (M3). search_hybrid validates dense_alpha and
    // the embedding requirement, returning a clean error on misuse.
    let hits =
        index.search_hybrid(query, params.top_k, filter, &HashSet::new(), dense_alpha, query_emb)?;
    let candidates_considered = hits.len();

    let prior_seen = state.seen_doc_id_set();

    // Per-id dedup + fresh-doc ordering in one pass over ranked hits. The first
    // hit for a doc is its best (hits are ranked), so it owns the source chunk.
    let mut dedup_skipped_count = 0usize;
    let mut fresh_order: Vec<String> = Vec::new();
    let mut fresh_set: HashSet<String> = HashSet::new();
    let mut source_chunk: HashMap<String, usize> = HashMap::new();
    for hit in &hits {
        if prior_seen.contains(&hit.doc_id) {
            dedup_skipped_count += 1;
            continue;
        }
        if fresh_set.insert(hit.doc_id.clone()) {
            fresh_order.push(hit.doc_id.clone());
            source_chunk.insert(hit.doc_id.clone(), hit.chunk_idx);
        }
    }

    // Fetch + inject fresh docs (up to fetch_top_n) within the char budget. The
    // real index is the fetcher; a per-doc failure is non-fatal (D9).
    let InjectResult {
        injected_docs,
        fetch_errors,
        used_chars_total: used,
        new_chars,
        budget_skipped_count,
        truncated_docs,
        newly_injected_ids,
    } = inject_fresh(
        &fresh_order,
        &source_chunk,
        params.fetch_top_n,
        params.max_chars_per_doc,
        params.max_total_chars,
        state.used_chars_total,
        |id| index.fetch_doc(id),
    );

    // Build the next state. A doc enters `seen_doc_ids` iff it was injected
    // (read) this round; budget-skipped / fetch-failed docs stay re-surfaceable
    // (the budget is monotonic so they simply won't re-inject; a transient
    // fetch failure gets another chance). `injected_units` mirrors the reads at
    // doc granularity — kept distinct from dedup so the two can diverge later
    // (e.g. chunk-level saturation, design §5) without a wire break.
    // Exhausted = no new *evidence* entered the context this round. Keyed on
    // new_chars (not injected_docs.is_empty()) so a fetched-but-empty document
    // (chars == 0) is still marked seen yet correctly signals exhaustion.
    let exhausted = new_chars == 0;
    let marginal_gain_chars = new_chars as f32 / (used.max(1)) as f32;
    let dedup_ratio = dedup_skipped_count as f32 / (candidates_considered.max(1)) as f32;

    let mut next = state.clone();
    next.schema_version = INTERLEAVE_STATE_VERSION;
    next.round = state.round.saturating_add(1);
    next.used_chars_total = used;
    for id in &newly_injected_ids {
        next.seen_doc_ids.push(id.clone());
        next.injected_units.push(EvidenceUnitId::doc(id.clone()));
    }
    next.empty_rounds = if exhausted {
        state.empty_rounds.saturating_add(1)
    } else {
        0
    };
    next.low_gain_rounds = if marginal_gain_chars < params.low_gain_threshold {
        state.low_gain_rounds.saturating_add(1)
    } else {
        0
    };
    next.canonicalize();

    // Stop model (design §4).
    let hard_round = next.round >= params.max_rounds;
    let hard_budget = used >= params.max_total_chars;
    let hard_stop = hard_round || hard_budget;
    let reason = match (hard_round, hard_budget) {
        (true, true) => Some("max_rounds and max_total_chars reached".to_string()),
        (true, false) => Some("max_rounds reached".to_string()),
        (false, true) => Some("max_total_chars reached".to_string()),
        (false, false) => None,
    };
    let stop_hints = StopHints {
        hard_stop,
        exhausted,
        exhausted_2_rounds: next.empty_rounds >= 2,
        low_gain_2_rounds: next.low_gain_rounds >= 2,
        reason,
    };

    let budget = RoundBudget {
        max_total_chars: params.max_total_chars,
        used_chars_total: used,
        remaining_chars: params.max_total_chars.saturating_sub(used),
        max_chars_per_doc: params.max_chars_per_doc,
        truncated_docs,
    };
    let stats = RoundStats {
        candidates_considered,
        dedup_skipped_count,
        budget_skipped_count,
        new_chars_injected: new_chars,
        marginal_gain_chars,
        dedup_ratio,
    };

    Ok(RoundResponse {
        round: next.round,
        hits,
        injected_docs,
        state: next,
        stats,
        budget,
        stop_hints,
        fetch_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ChunkConfig, Document};
    use chrono::Utc;

    #[test]
    fn dedup_mode_parse() {
        assert_eq!(DedupMode::parse("id").unwrap(), DedupMode::Id);
        assert!(DedupMode::parse("semantic").is_err());
        assert!(DedupMode::parse("").is_err());
        // Default is the only A5 mode.
        assert_eq!(DedupMode::default(), DedupMode::Id);
    }

    #[test]
    fn dedup_mode_serde_token() {
        // Wire token must be the lowercase "id", matching SUPPORTED.
        assert_eq!(serde_json::to_string(&DedupMode::Id).unwrap(), "\"id\"");
        let back: DedupMode = serde_json::from_str("\"id\"").unwrap();
        assert_eq!(back, DedupMode::Id);
    }

    #[test]
    fn fresh_state_is_round_one() {
        let s = InterleaveStateV1::fresh();
        assert_eq!(s.schema_version, INTERLEAVE_STATE_VERSION);
        assert_eq!(s.round, 0);
        assert!(s.seen_doc_ids.is_empty());
        assert!(s.injected_units.is_empty());
        assert_eq!(s.used_chars_total, 0);
        assert_eq!(InterleaveStateV1::default(), s);
    }

    #[test]
    fn state_serde_round_trip() {
        let s = InterleaveStateV1 {
            schema_version: 1,
            round: 2,
            seen_doc_ids: vec!["docA".into(), "docB".into()],
            injected_units: vec![EvidenceUnitId::doc("docA"), EvidenceUnitId::chunk("docB", 3)],
            used_chars_total: 1800,
            empty_rounds: 1,
            low_gain_rounds: 0,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: InterleaveStateV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn evidence_unit_chunk_idx_serializes_as_null() {
        // The wire form keeps chunk_idx explicit (design §3 example shows null).
        let u = EvidenceUnitId::doc("docA");
        assert_eq!(
            serde_json::to_value(&u).unwrap(),
            serde_json::json!({"doc_id": "docA", "chunk_idx": null})
        );
    }

    #[test]
    fn canonicalize_is_order_independent() {
        // Two states with the same ids in different insertion order must
        // serialize identically after canonicalize().
        let mut a = InterleaveStateV1 {
            seen_doc_ids: vec!["docC".into(), "docA".into(), "docB".into(), "docA".into()],
            injected_units: vec![
                EvidenceUnitId::chunk("docB", 5),
                EvidenceUnitId::doc("docA"),
                EvidenceUnitId::chunk("docB", 1),
            ],
            ..InterleaveStateV1::fresh()
        };
        let mut b = InterleaveStateV1 {
            seen_doc_ids: vec!["docA".into(), "docB".into(), "docC".into()],
            injected_units: vec![
                EvidenceUnitId::doc("docA"),
                EvidenceUnitId::chunk("docB", 1),
                EvidenceUnitId::chunk("docB", 5),
            ],
            ..InterleaveStateV1::fresh()
        };
        a.canonicalize();
        b.canonicalize();
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        // Dedup actually removed the duplicate "docA".
        assert_eq!(a.seen_doc_ids, vec!["docA", "docB", "docC"]);
    }

    #[test]
    fn evidence_unit_ordering_none_before_some() {
        let whole = EvidenceUnitId::doc("docA");
        let chunked = EvidenceUnitId::chunk("docA", 0);
        assert!(whole < chunked, "None chunk_idx must sort before Some");
    }

    #[test]
    fn resolve_none_is_fresh() {
        assert_eq!(
            InterleaveStateV1::resolve(None).unwrap(),
            InterleaveStateV1::fresh()
        );
    }

    #[test]
    fn resolve_rejects_bad_schema_version() {
        let s = InterleaveStateV1 {
            schema_version: 999,
            ..InterleaveStateV1::fresh()
        };
        assert!(matches!(
            InterleaveStateV1::resolve(Some(s)),
            Err(MsaError::InvalidInput(_))
        ));
    }

    #[test]
    fn validate_rejects_malformed_doc_id() {
        let s = InterleaveStateV1 {
            seen_doc_ids: vec!["ok".into(), "".into()],
            ..InterleaveStateV1::fresh()
        };
        assert!(s.validate().is_err());

        let s2 = InterleaveStateV1 {
            injected_units: vec![EvidenceUnitId::doc("bad\0id")],
            ..InterleaveStateV1::fresh()
        };
        assert!(s2.validate().is_err());
    }

    #[test]
    fn validate_rejects_oversized_state() {
        let s = InterleaveStateV1 {
            // One past the cap; the length check fires before the per-id loop.
            seen_doc_ids: vec!["d".to_string(); MAX_STATE_DOC_IDS + 1],
            ..InterleaveStateV1::fresh()
        };
        assert!(matches!(s.validate(), Err(MsaError::InvalidInput(_))));
    }

    #[test]
    fn validate_accepts_well_formed_state() {
        let s = InterleaveStateV1 {
            round: 3,
            seen_doc_ids: vec!["doc-1".into(), "vivling::abc".into()],
            injected_units: vec![EvidenceUnitId::chunk("doc-1", 2)],
            used_chars_total: 4096,
            empty_rounds: 0,
            low_gain_rounds: 1,
            schema_version: 1,
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn working_views_materialize_sets() {
        let s = InterleaveStateV1 {
            seen_doc_ids: vec!["a".into(), "b".into(), "a".into()],
            injected_units: vec![EvidenceUnitId::doc("a"), EvidenceUnitId::doc("a")],
            ..InterleaveStateV1::fresh()
        };
        // Sets collapse duplicates regardless of canonicalization.
        assert_eq!(s.seen_doc_id_set().len(), 2);
        assert_eq!(s.injected_unit_set().len(), 1);
        assert!(s.seen_doc_id_set().contains("a"));
    }

    #[test]
    fn params_clamp_into_server_ranges() {
        let p = InterleaveParams::clamped(
            0,           // top_k -> 1
            999,         // fetch_top_n -> min(ceiling, top_k) = 1
            usize::MAX,  // max_chars_per_doc -> ceiling
            usize::MAX,  // max_total_chars -> ceiling
            u32::MAX,    // max_rounds -> ceiling
            2.0,         // low_gain -> 1.0
            DedupMode::Id,
        );
        assert_eq!(p.top_k, 1);
        assert_eq!(p.fetch_top_n, 1); // bounded by clamped top_k
        assert_eq!(p.max_chars_per_doc, MAX_CHARS_PER_DOC_CEILING);
        assert_eq!(p.max_total_chars, MAX_TOTAL_CHARS_CEILING);
        assert_eq!(p.max_rounds, MAX_ROUNDS_CEILING);
        assert_eq!(p.low_gain_threshold, 1.0);
        assert_eq!(p.dedup_mode, DedupMode::Id);
    }

    #[test]
    fn params_fetch_top_n_bounded_by_top_k() {
        let p = InterleaveParams::clamped(8, 20, 2000, 16000, 5, 0.05, DedupMode::Id);
        assert_eq!(p.top_k, 8);
        assert_eq!(p.fetch_top_n, 8, "fetch_top_n must not exceed top_k");
    }

    #[test]
    fn params_low_gain_threshold_edge_cases() {
        assert_eq!(clamp_low_gain_threshold(-1.0), 0.0);
        assert_eq!(clamp_low_gain_threshold(0.5), 0.5);
        assert_eq!(clamp_low_gain_threshold(2.0), 1.0);
        assert_eq!(clamp_low_gain_threshold(f32::NAN), DEFAULT_LOW_GAIN_THRESHOLD);
        assert_eq!(
            clamp_low_gain_threshold(f32::INFINITY),
            DEFAULT_LOW_GAIN_THRESHOLD
        );
    }

    #[test]
    fn params_floors_are_at_least_one() {
        let p = InterleaveParams::clamped(0, 0, 0, 0, 0, 0.0, DedupMode::Id);
        assert_eq!(p.top_k, 1);
        assert_eq!(p.fetch_top_n, 1);
        assert_eq!(p.max_chars_per_doc, 1);
        assert_eq!(p.max_total_chars, 1);
        assert_eq!(p.max_rounds, 1);
    }

    // ---- A5.2 round logic --------------------------------------------------

    #[test]
    fn bound_text_caps_and_flags_truncation() {
        assert_eq!(bound_text("hello", 10), ("hello".into(), false, 5));
        assert_eq!(bound_text("hello", 5), ("hello".into(), false, 5));
        assert_eq!(bound_text("hello", 3), ("hel".into(), true, 3));
        assert_eq!(bound_text("", 4), (String::new(), false, 0));
        assert_eq!(bound_text("anything", 0), (String::new(), true, 0));
        // Counts Unicode scalar values, not bytes.
        assert_eq!(bound_text("àèìò", 2), ("àè".into(), true, 2));
    }

    /// Build a temp-backed index with the given `(id, text)` docs. Each short
    /// doc is one chunk under the default 64-word chunk size.
    fn idx_with(docs: &[(&str, &str)]) -> (tempfile::TempDir, MsaIndex) {
        let dir = tempfile::tempdir().unwrap();
        let idx =
            MsaIndex::open("test".into(), dir.path().to_path_buf(), ChunkConfig::default()).unwrap();
        for (id, text) in docs {
            idx.index_document(
                &Document {
                    id: (*id).into(),
                    text: (*text).into(),
                    metadata: Default::default(),
                    created_at: Utc::now(),
                },
                None,
            )
            .unwrap();
        }
        (dir, idx)
    }

    /// Default BM25-only params with explicit caps for deterministic tests.
    fn params(top_k: usize, fetch_top_n: usize, per_doc: usize, total: usize, rounds: u32) -> InterleaveParams {
        InterleaveParams::clamped(top_k, fetch_top_n, per_doc, total, rounds, 0.05, DedupMode::Id)
    }

    #[test]
    fn round1_injects_fresh_docs_and_updates_state() {
        let (_d, idx) = idx_with(&[
            ("a", "interleave memory alpha"),
            ("b", "interleave memory bravo"),
            ("c", "interleave memory charlie"),
        ]);
        let state = InterleaveStateV1::resolve(None).unwrap();
        let p = params(8, 2, 2000, 16000, 5);
        let r = run_round(&idx, "interleave", &state, &p, None, 1.0, None).unwrap();

        assert_eq!(r.round, 1);
        assert_eq!(r.injected_docs.len(), 2, "fetch_top_n=2");
        assert_eq!(r.state.seen_doc_ids.len(), 2);
        assert_eq!(r.state.injected_units.len(), 2);
        assert!(r.stats.candidates_considered >= 2);
        assert_eq!(r.stats.dedup_skipped_count, 0, "nothing seen yet");
        assert_eq!(r.stats.dedup_ratio, 0.0);
        assert!(!r.stop_hints.exhausted);
        assert_eq!(r.state.used_chars_total, r.budget.used_chars_total);
        assert!(r.stats.new_chars_injected > 0);
        // Round 1 marginal gain is ~1.0 (all injected chars are new).
        assert!((r.stats.marginal_gain_chars - 1.0).abs() < 1e-6);
    }

    #[test]
    fn round2_dedups_documents_injected_in_round1() {
        let (_d, idx) = idx_with(&[
            ("a", "interleave memory alpha"),
            ("b", "interleave memory bravo"),
            ("c", "interleave memory charlie"),
        ]);
        let p = params(8, 2, 2000, 16000, 5);
        let r1 = run_round(&idx, "interleave", &InterleaveStateV1::fresh(), &p, None, 1.0, None)
            .unwrap();
        // Feed r1.state back — same query, so the round-1 docs must dedup out.
        let r2 = run_round(&idx, "interleave", &r1.state, &p, None, 1.0, None).unwrap();

        assert_eq!(r2.round, 2);
        // The two docs from round 1 reappear as candidates but are dedup-skipped.
        assert!(r2.stats.dedup_skipped_count >= 2);
        for inj in &r2.injected_docs {
            assert!(
                !r1.state.seen_doc_ids.contains(&inj.doc_id),
                "round 2 must not re-inject a round-1 doc"
            );
        }
        // Cumulative seen set strictly grows (no doc lost, none duplicated).
        assert!(r2.state.seen_doc_ids.len() > r1.state.seen_doc_ids.len());
        assert!(r2.stats.dedup_ratio > 0.0);
    }

    #[test]
    fn exhausted_when_query_has_no_match() {
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16000, 5);
        let r = run_round(&idx, "nonexistentterm", &InterleaveStateV1::fresh(), &p, None, 1.0, None)
            .unwrap();
        assert_eq!(r.stats.candidates_considered, 0);
        assert!(r.injected_docs.is_empty());
        assert!(r.stop_hints.exhausted);
        assert_eq!(r.state.empty_rounds, 1);
        assert!(!r.stop_hints.exhausted_2_rounds, "single empty round is a hint, not a 2-round stop");
    }

    #[test]
    fn two_empty_rounds_raise_exhausted_2_rounds() {
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16000, 5);
        let r1 = run_round(&idx, "nope", &InterleaveStateV1::fresh(), &p, None, 1.0, None).unwrap();
        let r2 = run_round(&idx, "nope", &r1.state, &p, None, 1.0, None).unwrap();
        assert_eq!(r2.state.empty_rounds, 2);
        assert!(r2.stop_hints.exhausted_2_rounds);
    }

    #[test]
    fn per_doc_budget_truncates_and_reports() {
        let (_d, idx) = idx_with(&[("a", "interleave aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]);
        // Cap each doc to 5 chars; the stored original is much longer.
        let p = params(8, 2, 5, 16000, 5);
        let r = run_round(&idx, "interleave", &InterleaveStateV1::fresh(), &p, None, 1.0, None)
            .unwrap();
        assert_eq!(r.injected_docs.len(), 1);
        let inj = &r.injected_docs[0];
        assert_eq!(inj.chars, 5);
        assert!(inj.truncated);
        assert_eq!(r.budget.truncated_docs, 1);
        assert_eq!(r.budget.used_chars_total, 5);
        assert_eq!(r.budget.remaining_chars, 16000 - 5);
    }

    #[test]
    fn total_budget_skips_further_fresh_docs() {
        let (_d, idx) = idx_with(&[
            ("a", "interleave alpha alpha alpha"),
            ("b", "interleave bravo bravo bravo"),
        ]);
        // Total budget of 6 chars: the first injected doc fills it, the second
        // fresh doc (within fetch_top_n=2) is budget-skipped.
        let p = params(8, 2, 1000, 6, 5);
        let r = run_round(&idx, "interleave", &InterleaveStateV1::fresh(), &p, None, 1.0, None)
            .unwrap();
        assert_eq!(r.injected_docs.len(), 1, "only the first fresh doc fits");
        assert_eq!(r.stats.budget_skipped_count, 1);
        assert!(r.budget.used_chars_total <= 6);
        assert!(r.stop_hints.hard_stop, "used >= max_total_chars is a hard stop");
        assert_eq!(r.stop_hints.reason.as_deref(), Some("max_total_chars reached"));
    }

    #[test]
    fn hard_stop_on_max_rounds() {
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16000, 1); // max_rounds = 1
        let r = run_round(&idx, "interleave", &InterleaveStateV1::fresh(), &p, None, 1.0, None)
            .unwrap();
        assert_eq!(r.round, 1);
        assert!(r.stop_hints.hard_stop);
        assert_eq!(r.stop_hints.reason.as_deref(), Some("max_rounds reached"));
    }

    #[test]
    fn round_is_stateless_wrt_process() {
        // Statelessness: a hand-built state (as if from a crashed/reconnected
        // process) drives dedup identically — there is no server session.
        let (_d, idx) = idx_with(&[
            ("a", "interleave memory alpha"),
            ("b", "interleave memory bravo"),
        ]);
        let prior = InterleaveStateV1 {
            schema_version: 1,
            round: 7,
            seen_doc_ids: vec!["a".into()],
            injected_units: vec![EvidenceUnitId::doc("a")],
            used_chars_total: 100,
            empty_rounds: 0,
            low_gain_rounds: 0,
        };
        let p = params(8, 5, 2000, 16000, 32);
        let r = run_round(&idx, "interleave", &prior, &p, None, 1.0, None).unwrap();
        assert_eq!(r.round, 8, "round counter carried from client state");
        // "a" was already seen → only "b" is fresh and injected.
        assert_eq!(r.injected_docs.len(), 1);
        assert_eq!(r.injected_docs[0].doc_id, "b");
        assert!(r.stats.dedup_skipped_count >= 1);
        assert!(r.state.used_chars_total > 100, "budget carried and grew");
    }

    #[test]
    fn empty_query_is_invalid_input() {
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16000, 5);
        assert!(matches!(
            run_round(&idx, "   ", &InterleaveStateV1::fresh(), &p, None, 1.0, None),
            Err(MsaError::InvalidInput(_))
        ));
    }

    #[test]
    fn dense_alpha_below_one_without_embedding_errors() {
        // Parity with msa_search: dense_alpha < 1.0 with no embedding is
        // rejected by the search layer (clean error, not a silent BM25 fallback).
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16000, 5);
        assert!(run_round(&idx, "interleave", &InterleaveStateV1::fresh(), &p, None, 0.5, None).is_err());
    }

    #[test]
    fn run_round_reports_no_fetch_errors_on_healthy_index() {
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16000, 5);
        let r = run_round(&idx, "interleave", &InterleaveStateV1::fresh(), &p, None, 1.0, None)
            .unwrap();
        assert!(r.fetch_errors.is_empty());
    }

    /// A fetcher that fails for a chosen doc_id and otherwise returns a stub
    /// document. Lets us exercise the D9 non-fatal path without a corrupt index.
    fn stub_fetch(fail_id: &'static str) -> impl FnMut(&str) -> Result<Document> {
        move |id: &str| {
            if id == fail_id {
                Err(MsaError::UnknownDocument {
                    collection: "c".into(),
                    doc_id: id.into(),
                })
            } else {
                Ok(Document {
                    id: id.to_string(),
                    text: "interleave evidence text".into(),
                    metadata: Default::default(),
                    created_at: Utc::now(),
                })
            }
        }
    }

    #[test]
    fn inject_fresh_skips_failed_fetch_non_fatal() {
        let fresh = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let src = HashMap::new();
        let res = inject_fresh(&fresh, &src, 5, 1000, 16_000, 0, stub_fetch("b"));
        // "a" and "c" injected; "b" recorded as a non-fatal fetch error.
        assert_eq!(res.injected_docs.len(), 2);
        assert_eq!(res.newly_injected_ids, vec!["a", "c"]);
        assert_eq!(res.fetch_errors.len(), 1);
        assert_eq!(res.fetch_errors[0].doc_id, "b");
        assert!(res.new_chars > 0, "the round still produced evidence");
    }

    #[test]
    fn preflight_refuses_when_round_already_at_max() {
        // A client that ignores a prior hard_stop and calls again with a state
        // already at max_rounds must get NO further retrieval/injection.
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 16_000, 3); // max_rounds = 3
        let state = InterleaveStateV1 {
            round: 3, // already at the hard limit
            ..InterleaveStateV1::fresh()
        };
        let r = run_round(&idx, "interleave", &state, &p, None, 1.0, None).unwrap();
        assert_eq!(r.round, 3, "no new round runs");
        assert_eq!(
            r.stats.candidates_considered, 0,
            "search must not run (would be >=1 otherwise)"
        );
        assert!(r.hits.is_empty());
        assert!(r.injected_docs.is_empty());
        assert!(r.stop_hints.hard_stop);
        assert_eq!(r.stop_hints.reason.as_deref(), Some("max_rounds already reached"));
    }

    #[test]
    fn preflight_refuses_when_budget_already_spent() {
        let (_d, idx) = idx_with(&[("a", "interleave memory alpha")]);
        let p = params(8, 2, 2000, 100, 5); // max_total_chars = 100
        let state = InterleaveStateV1 {
            used_chars_total: 100, // budget already spent
            ..InterleaveStateV1::fresh()
        };
        let r = run_round(&idx, "interleave", &state, &p, None, 1.0, None).unwrap();
        assert_eq!(r.stats.candidates_considered, 0, "search must not run");
        assert!(r.injected_docs.is_empty());
        assert!(r.stop_hints.hard_stop);
        assert_eq!(
            r.stop_hints.reason.as_deref(),
            Some("max_total_chars already reached")
        );
        // The unchanged budget is reported honestly.
        assert_eq!(r.budget.used_chars_total, 100);
        assert_eq!(r.budget.remaining_chars, 0);
    }

    #[test]
    fn inject_fresh_all_failed_yields_empty_injection() {
        // Every fetch fails: no evidence, all recorded — run_round turns this
        // into exhausted=true / empty_rounds++ without aborting the round.
        let fresh = vec!["x".to_string(), "y".to_string()];
        let src = HashMap::new();
        let mut fail_all = |id: &str| -> Result<Document> {
            Err(MsaError::UnknownDocument {
                collection: "c".into(),
                doc_id: id.into(),
            })
        };
        let res = inject_fresh(&fresh, &src, 5, 1000, 16_000, 0, &mut fail_all);
        assert!(res.injected_docs.is_empty());
        assert_eq!(res.fetch_errors.len(), 2);
        assert_eq!(res.new_chars, 0);
        assert!(res.newly_injected_ids.is_empty());
    }
}
