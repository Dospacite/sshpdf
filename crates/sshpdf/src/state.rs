//! Remembering the last page of each file, in a small state file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Files remembered at most.
const MAX_FILES: usize = 500;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Absolute path → (last page, 0-based; order of last use).
    files: BTreeMap<String, Entry>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
struct Entry {
    page: usize,
    used: u64,
}

pub fn default_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("sshpdf").join("state.toml"))
}

fn key(file: &Path) -> String {
    std::fs::canonicalize(file)
        .unwrap_or_else(|_| file.to_path_buf())
        .display()
        .to_string()
}

impl State {
    pub fn load(path: &Path) -> State {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn last_page(&self, file: &Path) -> Option<usize> {
        self.files.get(&key(file)).map(|e| e.page)
    }

    pub fn set_last_page(&mut self, file: &Path, page: usize) {
        let used = self.files.values().map(|e| e.used).max().unwrap_or(0) + 1;
        self.files.insert(key(file), Entry { page, used });
        if self.files.len() > MAX_FILES {
            let mut by_use: Vec<(u64, String)> = self.files.iter().map(|(k, e)| (e.used, k.clone())).collect();
            by_use.sort();
            for (_, k) in by_use.into_iter().take(self.files.len() - MAX_FILES) {
                self.files.remove(&k);
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        // Write then rename, so a crash never leaves half a file.
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("sshpdf-state-test-{}", std::process::id()));
        let path = dir.join("state.toml");
        let mut s = State::default();
        s.set_last_page(Path::new("/x/a.pdf"), 4);
        s.set_last_page(Path::new("/x/b.pdf"), 7);
        s.save(&path).unwrap();
        let t = State::load(&path);
        assert_eq!(t.last_page(Path::new("/x/a.pdf")), Some(4));
        assert_eq!(t.last_page(Path::new("/x/b.pdf")), Some(7));
        assert_eq!(t.last_page(Path::new("/x/c.pdf")), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keeps_the_most_recent_files() {
        let mut s = State::default();
        for i in 0..MAX_FILES + 10 {
            s.set_last_page(Path::new(&format!("/f/{i}.pdf")), i);
        }
        assert_eq!(s.files.len(), MAX_FILES);
        assert!(s.last_page(Path::new("/f/0.pdf")).is_none());
        assert!(s.last_page(Path::new(&format!("/f/{}.pdf", MAX_FILES + 9))).is_some());
    }
}
