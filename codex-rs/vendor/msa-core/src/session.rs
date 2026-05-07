//! Server-side cursor for `msa_search_iterative` (Memory Interleave, paper §3.5).
//!
//! A session ties a sequence of `msa_search_iterative` calls together so
//! the AI client can multi-hop without re-sending the doc_ids it has
//! already consumed. Each call:
//!
//! 1. Reuses (or creates) a `MsaSession` keyed by `session_id`.
//! 2. Excludes `seen_doc_ids` from retrieval.
//! 3. Adds the new hits to `seen_doc_ids` so the next round dedups them.
//! 4. Reports `exhausted = true` when the underlying retrieval returns 0
//!    new chunks, signaling the AI client to stop iterating.
//!
//! Sessions live in memory with a TTL (default 10 minutes idle). GC runs
//! lazily on `acquire`/`store` so there is no background task. Lost session
//! state on server restart is acceptable: the AI client can simply restart
//! the loop with the original query.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MsaSession {
    pub id: String,
    pub collection: String,
    /// The original (un-augmented) user query. Held for diagnostics; v0.3
    /// does not yet rewrite the underlying query, only excludes seen docs.
    pub query: String,
    pub seen_doc_ids: HashSet<String>,
    pub round: u32,
    pub last_touched: Instant,
}

impl MsaSession {
    pub fn new(id: String, collection: String, query: String) -> Self {
        Self {
            id,
            collection,
            query,
            seen_doc_ids: HashSet::new(),
            round: 0,
            last_touched: Instant::now(),
        }
    }
}

pub struct SessionRegistry {
    inner: Mutex<HashMap<String, MsaSession>>,
    ttl: Duration,
}

impl SessionRegistry {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Default 10-minute idle TTL.
    pub fn with_default_ttl() -> Self {
        Self::new(Duration::from_secs(10 * 60))
    }

    /// Take the session out for mutation. The caller is expected to call
    /// [`Self::store`] when done. This avoids holding the mutex across
    /// the search call.
    pub fn take(&self, session_id: &str) -> Option<MsaSession> {
        let mut guard = self.inner.lock().expect("session registry poisoned");
        self.gc(&mut guard);
        guard.remove(session_id)
    }

    pub fn store(&self, mut session: MsaSession) {
        session.last_touched = Instant::now();
        let mut guard = self.inner.lock().expect("session registry poisoned");
        self.gc(&mut guard);
        guard.insert(session.id.clone(), session);
    }

    pub fn drop_session(&self, session_id: &str) -> bool {
        let mut guard = self.inner.lock().expect("session registry poisoned");
        guard.remove(session_id).is_some()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("session registry poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn gc(&self, guard: &mut HashMap<String, MsaSession>) {
        let now = Instant::now();
        let ttl = self.ttl;
        guard.retain(|_, s| now.duration_since(s.last_touched) < ttl);
    }
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::with_default_ttl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let reg = SessionRegistry::with_default_ttl();
        let s = MsaSession::new("sid".into(), "col".into(), "q".into());
        reg.store(s);
        let mut taken = reg.take("sid").expect("session");
        taken.seen_doc_ids.insert("d1".into());
        taken.round += 1;
        reg.store(taken);
        let again = reg.take("sid").expect("session");
        assert_eq!(again.round, 1);
        assert!(again.seen_doc_ids.contains("d1"));
    }

    #[test]
    fn ttl_evicts_idle_sessions() {
        let reg = SessionRegistry::new(Duration::from_millis(50));
        reg.store(MsaSession::new("sid".into(), "col".into(), "q".into()));
        std::thread::sleep(Duration::from_millis(80));
        // gc runs on next access
        assert!(reg.take("sid").is_none(), "session should be evicted");
    }

    #[test]
    fn drop_session_removes_immediately() {
        let reg = SessionRegistry::with_default_ttl();
        reg.store(MsaSession::new("sid".into(), "col".into(), "q".into()));
        assert!(reg.drop_session("sid"));
        assert!(reg.take("sid").is_none());
    }
}
