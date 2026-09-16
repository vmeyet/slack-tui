use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn for_workspace(key: &str) -> Self {
        let root = std::env::var_os("SLACK_CLI_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::cache_dir().map(|d| d.join("slack-cli")))
            .unwrap_or_else(|| PathBuf::from(".slack-cli-cache"));
        Self::new(root.join(key))
    }

    pub fn load<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let raw = std::fs::read(self.dir.join(format!("{name}.json"))).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{name}.json"));
        std::fs::write(&path, serde_json::to_vec(value)?).with_context(|| format!("writing {}", path.display()))
    }

    pub fn clear(&self) -> Result<()> {
        match std::fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("t"));
        assert_eq!(cache.load::<Vec<u32>>("x"), None);
        cache.save("x", &vec![1, 2]).unwrap();
        assert_eq!(cache.load::<Vec<u32>>("x"), Some(vec![1, 2]));
        cache.clear().unwrap();
        cache.clear().unwrap();
        assert_eq!(cache.load::<Vec<u32>>("x"), None);
    }

    #[test]
    fn corrupt_file_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        std::fs::write(dir.path().join("x.json"), b"{nope").unwrap();
        assert_eq!(cache.load::<Vec<u32>>("x"), None);
    }
}
