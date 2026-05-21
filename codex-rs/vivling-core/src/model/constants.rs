/// State schema version. Bumped to 9 in Memory V2 Step 2.A; the only
/// observable difference vs version 8 is the presence of new
/// `#[serde(default)]` fields on `VivlingState` (and the matching pure
/// data types in `codex_vivling_core::model`). V8 JSON loads unchanged
/// into a V9 binary because every new field has an explicit default.
pub const VERSION: u32 = 9;
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
pub const MAX_DIRECT_REPLY_LEN: usize = 120;
pub const MAX_CARD_REPLY_LEN: usize = 280;
