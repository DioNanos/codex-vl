/// State schema version. Bumped to 10 in Memory V2 Step 12.B.A; the
/// only observable difference vs version 9 is the presence of new
/// `#[serde(default)]` fields on `VivlingState`:
///   - `crt_brain_mode: VivlingExpressionMode` (tri-state Default/On/Off)
///   - `daily_llm_*` counters + `daily_llm_day_key`
///   - `last_llm_dispatch_at`
/// V9 (and transitively V8) JSON loads unchanged into a V10 binary
/// because every new field has an explicit default. The save path
/// (Step 1.C/2.B) records a one-shot `.v9.bak` snapshot the first time
/// a V9 file is rewritten as V10, mirroring the V8→V9 pre-migration
/// backup contract.
pub const VERSION: u32 = 10;
pub const MAX_LEVEL: u64 = 99;
pub const JUVENILE_LEVEL: u64 = 30;
pub const ADULT_LEVEL: u64 = 60;
pub const SPAWN_SLOT_LEVEL_STEP: u64 = 30;
pub const JUVENILE_ACTIVE_DAYS: u64 = 30;
pub const ADULT_ACTIVE_DAYS: u64 = 90;
pub const WORK_XP_PER_LEVEL: u64 = 60;
pub const DAILY_WORK_XP_CAP: u64 = 60;
pub const MAX_WORK_MEMORY_ENTRIES: usize = 64;
pub const MAX_DISTILLED_MEMORY_ENTRIES: usize = 24;
pub const MAX_MENTAL_PATH_ENTRIES: usize = 256;
pub const DISTILL_TRIGGER_CAPSULES: u64 = 8;

/// Capsule kinds that are operational bookkeeping, not knowledge. They live
/// in the rolling working memory (and feed `loop_profile` signals) but are
/// excluded from BOTH the long-term MSA archive (ingest gate, live audit
/// 2026-06-07 F1) and the distillation pipeline (F3: they produced garbage
/// topics like "wait"/"verify" with compounding observation counters).
pub const BOOKKEEPING_KINDS: &[&str] = &[
    "live_context",
    "loop_runtime",
    "loop_config",
    "loop_profile",
    "loop_blocked_busy",
    "loop_blocked_review",
    "loop_blocked_side",
];
pub const MAX_DIRECT_REPLY_LEN: usize = 120;
pub const MAX_CARD_REPLY_LEN: usize = 280;
