use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct VivlingLiveStats {
    pub(crate) energy: u8,
    pub(crate) naps_total: u32,
    pub(crate) minutes_slept: u32,
    pub(crate) bites_eaten: u32,
    pub(crate) games_played: u32,
    pub(crate) last_state_change_epoch_ms: u64,
}

impl Default for VivlingLiveStats {
    fn default() -> Self {
        Self {
            energy: 80,
            naps_total: 0,
            minutes_slept: 0,
            bites_eaten: 0,
            games_played: 0,
            last_state_change_epoch_ms: now_epoch_ms(),
        }
    }
}

impl VivlingLiveStats {
    pub(crate) fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub(crate) fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        let data = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, data)?;
        fs::rename(&tmp, path)
    }

    pub(crate) fn clamp_energy(&mut self) {
        self.energy = self.energy.clamp(0, 100);
    }
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_corrupted_file_returns_default() {
        let dir = std::env::temp_dir().join("vl_stats_test_corrupt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("live_stats.json");
        fs::write(&path, "not json!!!").unwrap();
        let stats = VivlingLiveStats::load_from(&path);
        assert_eq!(stats.energy, 80);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("vl_stats_test_roundtrip");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("live_stats.json");
        let mut stats = VivlingLiveStats::default();
        stats.naps_total = 3;
        stats.energy = 42;
        stats.save_to(&path).unwrap();
        let loaded = VivlingLiveStats::load_from(&path);
        assert_eq!(loaded.naps_total, 3);
        assert_eq!(loaded.energy, 42);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn energy_clamp_bounds() {
        let mut stats = VivlingLiveStats::default();
        stats.energy = 200;
        stats.clamp_energy();
        assert_eq!(stats.energy, 100);
        stats.energy = 0;
        stats.clamp_energy();
        assert_eq!(stats.energy, 0);
    }
}
