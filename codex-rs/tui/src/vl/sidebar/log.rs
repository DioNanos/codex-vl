use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VivlingLogKind {
    Chat,
    Assist,
    Life,
}

#[derive(Debug, Clone)]
pub(crate) struct VivlingLogEntry {
    pub(crate) kind: VivlingLogKind,
    pub(crate) text: String,
    pub(crate) ts: Instant,
    pub(crate) vivling_id: Option<String>,
}

pub(crate) struct VivlingChatLog {
    entries: Vec<VivlingLogEntry>,
    cap: usize,
    unread: usize,
}

impl VivlingChatLog {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            cap: 500,
            unread: 0,
        }
    }

    pub(crate) fn push(&mut self, entry: VivlingLogEntry) {
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push(entry);
        self.unread = self.unread.saturating_add(1);
    }

    pub(crate) fn iter_recent(&self, n: usize) -> impl Iterator<Item = &VivlingLogEntry> {
        let start = self.entries.len().saturating_sub(n);
        self.entries[start..].iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn unread_count(&self) -> usize {
        self.unread
    }

    pub(crate) fn mark_read(&mut self) {
        self.unread = 0;
    }

    pub(crate) fn last_entry(&self) -> Option<&VivlingLogEntry> {
        self.entries.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(text: &str) -> VivlingLogEntry {
        VivlingLogEntry {
            kind: VivlingLogKind::Chat,
            text: text.to_string(),
            ts: Instant::now(),
            vivling_id: None,
        }
    }

    #[test]
    fn ring_buffer_overflow_caps_at_500() {
        let mut log = VivlingChatLog::new();
        for i in 0..600 {
            log.push(make_entry(&format!("msg-{i}")));
        }
        assert_eq!(log.len(), 500);
        let all: Vec<_> = log.iter_recent(500).collect();
        assert!(all[0].text.contains("msg-100"));
        assert!(all.last().unwrap().text.contains("msg-599"));
    }

    #[test]
    fn unread_increases_after_push_and_resets_after_mark_read() {
        let mut log = VivlingChatLog::new();
        assert_eq!(log.unread_count(), 0);
        log.push(make_entry("a"));
        log.push(make_entry("b"));
        assert_eq!(log.unread_count(), 2);
        log.mark_read();
        assert_eq!(log.unread_count(), 0);
    }

    #[test]
    fn iter_recent_returns_last_n() {
        let mut log = VivlingChatLog::new();
        for i in 0..10 {
            log.push(make_entry(&format!("{i}")));
        }
        let recent: Vec<&str> = log.iter_recent(3).map(|e| e.text.as_str()).collect();
        assert_eq!(recent, vec!["7", "8", "9"]);
    }

    #[test]
    fn last_entry_returns_most_recent() {
        let mut log = VivlingChatLog::new();
        assert!(log.last_entry().is_none());
        log.push(make_entry("first"));
        log.push(make_entry("second"));
        assert_eq!(log.last_entry().unwrap().text, "second");
    }
}
