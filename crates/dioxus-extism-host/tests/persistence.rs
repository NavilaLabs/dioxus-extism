use dioxus_extism_host::{JsonFilePersistence, StatePersistenceProvider};
use dioxus_extism_protocol::PluginId;
use std::collections::HashMap;

#[tokio::test]
async fn save_atomic_write_leaves_no_temp_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let persistence = JsonFilePersistence::new(dir.path());
    let plugin_id = PluginId("test/plugin".into());

    let mut state: HashMap<String, serde_json::Value> = HashMap::new();
    state.insert("key".into(), serde_json::json!("value"));

    persistence.save(&plugin_id, &state).await.expect("save must succeed");

    let target = dir.path().join("test_plugin.json");
    assert!(target.exists(), "target file must exist after save");

    let temp_files: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(temp_files.is_empty(), "temp file remains after save: {temp_files:?}");
}

#[tokio::test]
async fn load_missing_file_returns_ok_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    let persistence = JsonFilePersistence::new(dir.path().join("nonexistent_subdir"));
    let result = persistence.load(&PluginId("any/plugin".into())).await;
    assert!(matches!(result, Ok(None)), "expected Ok(None), got {result:?}");
}
