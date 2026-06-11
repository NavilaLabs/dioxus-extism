//! Plugin bundle support: `bundle.toml` parsing, atomic install, and uninstall.
//!
//! A bundle is a directory containing a `bundle.toml` manifest plus one subdirectory per
//! plugin. Bundles install atomically — if any plugin fails, all already-installed siblings
//! are rolled back. Trust-group synthesis grants mutual `CallPlugin` capabilities for all
//! public functions between bundle siblings.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use dioxus_extism_protocol::PluginId;

use crate::error::PluginRuntimeError;

// ── Bundle manifest types ─────────────────────────────────────────────────────

/// Declares how trust flows among bundle siblings.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BundleTrustGroup {
    /// When `true`, each plugin in the bundle is granted implicit `CallPlugin` capabilities
    /// for every public function of every sibling.
    #[serde(default)]
    pub mutual_call_plugin: bool,
}

/// One plugin entry inside a bundle manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlePluginEntry {
    /// Plugin id (must match the `id` in the plugin's own manifest).
    pub id: PluginId,
    /// Path relative to the bundle root directory containing the plugin's manifest and `.wasm`.
    pub path: String,
}

/// Top-level `bundle.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    pub bundle: BundleInner,
}

/// Inner `[bundle]` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleInner {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub plugins: Vec<BundlePluginEntry>,
    #[serde(default)]
    pub trust_group: BundleTrustGroup,
}

/// Identifier for a bundle install. Currently the `bundle.id` string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BundleId(pub String);

// ── Bundle source ─────────────────────────────────────────────────────────────

/// Where to load a bundle from.
pub enum BundleSource {
    /// Local directory containing `bundle.toml` and plugin subdirectories.
    Directory(std::path::PathBuf),
}

impl BundleSource {
    /// Parse the `bundle.toml` from this source.
    pub fn load_manifest(&self) -> Result<BundleManifest, PluginRuntimeError> {
        match self {
            Self::Directory(dir) => {
                let toml_path = dir.join("bundle.toml");
                let contents =
                    std::fs::read_to_string(&toml_path).map_err(PluginRuntimeError::Io)?;
                toml::from_str::<BundleManifest>(&contents)
                    .map_err(|e| PluginRuntimeError::Pool(format!("bundle.toml parse error: {e}")))
            }
        }
    }

    /// Return the filesystem path for a plugin entry within this bundle.
    pub fn plugin_path(&self, entry: &BundlePluginEntry) -> std::path::PathBuf {
        match self {
            Self::Directory(dir) => dir.join(&entry.path),
        }
    }
}

// ── Trust-group synthesis ─────────────────────────────────────────────────────

/// Synthesise implicit `requires_plugins` entries for bundle siblings.
///
/// When `trust_group.mutual_call_plugin = true`, each plugin is granted access to every
/// public function declared by each sibling. This is done by injecting synthetic
/// `PluginDependency` entries before install — the dep-graph validation then treats
/// these as regular required dependencies.
///
/// `public_fns_by_id` maps sibling plugin id → list of public function names
/// (as read from each sibling's manifest).
pub fn synthesise_trust_group(
    plugin_ids: &[PluginId],
    public_fns_by_id: &HashMap<PluginId, Vec<String>>,
) -> HashMap<PluginId, Vec<dioxus_extism_protocol::PluginDependency>> {
    let mut result: HashMap<PluginId, Vec<dioxus_extism_protocol::PluginDependency>> =
        HashMap::new();

    for id in plugin_ids {
        let mut deps = Vec::new();
        for sibling_id in plugin_ids {
            if sibling_id == id {
                continue;
            }
            let functions = public_fns_by_id
                .get(sibling_id)
                .cloned()
                .unwrap_or_default();
            if functions.is_empty() {
                continue;
            }
            deps.push(dioxus_extism_protocol::PluginDependency::new(
                sibling_id.0.clone(),
                "*",  // any version — bundle siblings install together
                true, // required (both installed as a unit)
                functions,
            ));
        }
        if !deps.is_empty() {
            result.insert(id.clone(), deps);
        }
    }

    result
}

// ── find_wasm ─────────────────────────────────────────────────────────────────

/// Find the single `.wasm` file inside a plugin directory.
///
/// Returns `Err` if there are zero or more than one `.wasm` files.
pub fn find_wasm_in_dir(dir: &Path) -> Result<std::path::PathBuf, PluginRuntimeError> {
    let entries = std::fs::read_dir(dir).map_err(PluginRuntimeError::Io)?;
    let wasms: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("wasm"))
        .collect();
    match wasms.len() {
        0 => Err(PluginRuntimeError::Pool(format!(
            "no .wasm file found in {}",
            dir.display()
        ))),
        1 => Ok(wasms.into_iter().next().expect("len == 1")),
        n => Err(PluginRuntimeError::Pool(format!(
            "{n} .wasm files found in {}; expected exactly one",
            dir.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus_extism_protocol::PluginId;

    #[test]
    fn synthesise_trust_group_empty_fns_skipped() {
        let ids = vec![PluginId("a".into()), PluginId("b".into())];
        let public = HashMap::new(); // no public functions
        let result = synthesise_trust_group(&ids, &public);
        assert!(result.is_empty(), "no deps when no public fns");
    }

    #[test]
    fn synthesise_trust_group_creates_mutual_deps() {
        let ids = vec![PluginId("a".into()), PluginId("b".into())];
        let mut public = HashMap::new();
        public.insert(PluginId("a".into()), vec!["fn_a".into()]);
        public.insert(PluginId("b".into()), vec!["fn_b".into()]);
        let result = synthesise_trust_group(&ids, &public);

        let a_deps = result.get(&PluginId("a".into())).expect("a has deps");
        assert_eq!(a_deps.len(), 1);
        assert_eq!(a_deps[0].id.0, "b");
        assert_eq!(a_deps[0].functions, vec!["fn_b".to_string()]);

        let b_deps = result.get(&PluginId("b".into())).expect("b has deps");
        assert_eq!(b_deps.len(), 1);
        assert_eq!(b_deps[0].id.0, "a");
        assert_eq!(b_deps[0].functions, vec!["fn_a".to_string()]);
    }
}
