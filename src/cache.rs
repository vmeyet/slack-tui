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
        Self::new(root().join(key))
    }

    /// What every workspace shares, next to their directories.
    pub fn shared() -> Self {
        Self::new(root())
    }

    pub async fn load<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let raw = tokio::fs::read(self.dir.join(format!("{name}.json"))).await.ok()?;
        serde_json::from_slice(&raw).ok()
    }

    pub async fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let path = self.dir.join(format!("{name}.json"));
        tokio::fs::write(&path, serde_json::to_vec(value)?).await.with_context(|| format!("writing {}", path.display()))
    }

    pub async fn clear(&self) -> Result<()> {
        match tokio::fs::remove_dir_all(&self.dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn root() -> PathBuf {
    std::env::var_os("SLACK_CLI_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::cache_dir().map(|d| d.join("slack-cli")))
        .unwrap_or_else(|| PathBuf::from(".slack-cli-cache"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[tokio::test]
    async fn round_trip_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().join("t"));
        assert_eq!(cache.load::<Vec<u32>>("x").await, None);
        cache.save("x", &vec![1, 2]).await.unwrap();
        assert_eq!(cache.load::<Vec<u32>>("x").await, Some(vec![1, 2]));
        cache.clear().await.unwrap();
        cache.clear().await.unwrap();
        assert_eq!(cache.load::<Vec<u32>>("x").await, None);
    }

    #[tokio::test]
    async fn corrupt_file_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        std::fs::write(dir.path().join("x.json"), b"{nope").unwrap();
        assert_eq!(cache.load::<Vec<u32>>("x").await, None);
    }
}
