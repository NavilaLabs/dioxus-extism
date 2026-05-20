use std::{collections::HashMap, path::PathBuf};

use async_trait::async_trait;
use dioxus_extism_protocol::PluginId;

use crate::error::PersistenceError;
use crate::runtime::StatePersistenceProvider;

/// JSON-file-backed persistence for plugin global state.
///
/// Each plugin's state is stored as a separate `.json` file under `dir`.
/// Writes are atomic: data is written to a temp file first, then renamed over the
/// target so a crash mid-write never corrupts the existing file.
pub struct JsonFilePersistence {
    /// Directory where per-plugin state files are stored.
    pub dir: PathBuf,
}

impl JsonFilePersistence {
    /// Create a new persistence backend writing to `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path_for(&self, plugin_id: &PluginId) -> PathBuf {
        let filename = format!("{}.json", plugin_id.0.replace('/', "_"));
        self.dir.join(filename)
    }
}

#[async_trait]
impl StatePersistenceProvider for JsonFilePersistence {
    async fn save(
        &self,
        plugin_id: &PluginId,
        state: &HashMap<String, serde_json::Value>,
    ) -> Result<(), PersistenceError> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let target = self.path_for(plugin_id);
        // Write to a sibling temp file then rename — atomic on POSIX.
        let tmp = target.with_extension("tmp");
        let json = serde_json::to_string_pretty(state)?;
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, &target).await?;
        Ok(())
    }

    async fn load(
        &self,
        plugin_id: &PluginId,
    ) -> Result<Option<HashMap<String, serde_json::Value>>, PersistenceError> {
        let path = self.path_for(plugin_id);
        match tokio::fs::read_to_string(&path).await {
            Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(PersistenceError::Io(e)),
        }
    }
}
