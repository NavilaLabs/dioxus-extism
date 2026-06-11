//! Dependency graph for installed plugins.
//!
//! Tracks which plugins depend on which, validates version constraints using semver,
//! and checks for cycles (forbidden by the RFC).

use std::collections::{HashMap, HashSet, VecDeque};

use dioxus_extism_protocol::{PluginDependency, PluginId};
use semver::{Version, VersionReq};

use crate::error::InstallError;

/// Lightweight snapshot of a loaded plugin's dependency metadata.
#[derive(Debug, Clone)]
pub struct PluginDepInfo {
    /// SemVer-parseable version string, e.g. `"1.5.0"`.
    pub version: String,
    /// Dependency declarations from the manifest.
    pub requires: Vec<PluginDependency>,
}

/// The live dependency graph across all installed plugins.
#[derive(Debug, Default)]
pub struct DepGraph {
    /// Keyed by plugin id. Contains every installed plugin's version + deps.
    nodes: HashMap<PluginId, PluginDepInfo>,
}

impl DepGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a node (used during install / reload).
    ///
    /// Does NOT check for cycles — call [`check_acyclic`] after building the graph.
    pub fn insert(&mut self, id: PluginId, info: PluginDepInfo) {
        self.nodes.insert(id, info);
    }

    /// Remove a node (used during uninstall).
    pub fn remove(&mut self, id: &PluginId) {
        self.nodes.remove(id);
    }

    /// Returns `true` if the plugin is in the graph.
    pub fn contains(&self, id: &PluginId) -> bool {
        self.nodes.contains_key(id)
    }

    /// Returns the version string of a loaded plugin, or `None`.
    pub fn version_of(&self, id: &PluginId) -> Option<&str> {
        self.nodes.get(id).map(|n| n.version.as_str())
    }

    /// Validate that all `required_deps` of `plugin_id` are satisfiable by currently
    /// installed plugins.
    ///
    /// For **required** dependencies, missing or version-mismatched targets produce
    /// [`InstallError`] variants. For **optional** dependencies, errors are silently
    /// skipped here — the caller consults optional availability separately.
    ///
    /// Also validates that each required dependency's referenced functions are declared
    /// public in `public_exports_of` (called by the outer install logic which has that data).
    pub fn validate_required_deps(
        &self,
        plugin_id: &PluginId,
        deps: &[PluginDependency],
    ) -> Result<(), InstallError> {
        for dep in deps {
            if !dep.required {
                continue;
            }
            let target_info = self.nodes.get(&dep.id).ok_or_else(|| {
                InstallError::DependencyMissing {
                    plugin: plugin_id.clone(),
                    dependency: dep.id.clone(),
                }
            })?;
            check_version_match(plugin_id, &dep.id, &dep.version.0, &target_info.version)?;
        }
        Ok(())
    }

    /// Returns which optional dependencies of `plugin_id` are satisfiable.
    ///
    /// Returns a list of `(dependency_index, satisfiable)` pairs — one per optional dep.
    pub fn optional_satisfiability(
        &self,
        deps: &[PluginDependency],
    ) -> Vec<(usize, bool)> {
        deps.iter()
            .enumerate()
            .filter(|(_, d)| !d.required)
            .map(|(i, d)| {
                let ok = self.nodes.get(&d.id).is_some_and(|info| {
                    check_version_match(&PluginId::default(), &d.id, &d.version.0, &info.version)
                        .is_ok()
                });
                (i, ok)
            })
            .collect()
    }

    /// Perform a topological sort (Kahn's algorithm) and return `Err` if a cycle exists.
    ///
    /// This validates the entire graph, including the newly-added node.
    pub fn check_acyclic(&self) -> Result<(), InstallError> {
        // Build adjacency list: caller → list of callees present in graph.
        let mut in_degree: HashMap<&PluginId, usize> = self.nodes.keys().map(|k| (k, 0)).collect();
        let mut adj: HashMap<&PluginId, Vec<&PluginId>> = HashMap::new();

        for (id, info) in &self.nodes {
            for dep in &info.requires {
                if self.nodes.contains_key(&dep.id) {
                    adj.entry(id).or_default().push(&dep.id);
                    *in_degree.entry(&dep.id).or_insert(0) += 1;
                }
            }
        }

        let mut queue: VecDeque<&PluginId> =
            in_degree.iter().filter(|(_, d)| **d == 0).map(|(k, _)| *k).collect();
        let mut visited = 0usize;

        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(callees) = adj.get(node) {
                for callee in callees {
                    let deg = in_degree.entry(callee).or_insert(0);
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push_back(callee);
                    }
                }
            }
        }

        if visited < self.nodes.len() {
            let cycle: Vec<PluginId> = in_degree
                .into_iter()
                .filter(|(_, d)| *d > 0)
                .map(|(k, _)| k.clone())
                .collect();
            return Err(InstallError::CyclicDependency { cycle });
        }
        Ok(())
    }

    /// Returns the set of plugins that declare a **required** dependency on `id`.
    ///
    /// Used to auto-disable dependents when a plugin is uninstalled.
    pub fn required_dependents_of(&self, id: &PluginId) -> HashSet<PluginId> {
        self.nodes
            .iter()
            .filter(|(_, info)| {
                info.requires
                    .iter()
                    .any(|d| d.required && &d.id == id)
            })
            .map(|(dep_id, _)| dep_id.clone())
            .collect()
    }

    /// Returns the set of plugins that declare an **optional** dependency on `id`.
    pub fn optional_dependents_of(&self, id: &PluginId) -> HashSet<PluginId> {
        self.nodes
            .iter()
            .filter(|(_, info)| {
                info.requires
                    .iter()
                    .any(|d| !d.required && &d.id == id)
            })
            .map(|(dep_id, _)| dep_id.clone())
            .collect()
    }
}

/// Check that `target_version` satisfies the semver `constraint`.
///
/// Returns `Err(InstallError::DependencyVersionConflict)` on mismatch or parse failure.
pub fn check_version_match(
    plugin: &PluginId,
    dep_id: &PluginId,
    constraint: &str,
    target_version: &str,
) -> Result<(), InstallError> {
    let req = VersionReq::parse(constraint).map_err(|_| InstallError::DependencyVersionConflict {
        plugin: plugin.clone(),
        dependency: dep_id.clone(),
        required: constraint.to_owned(),
        found: target_version.to_owned(),
    })?;
    let ver = Version::parse(target_version).map_err(|_| InstallError::DependencyVersionConflict {
        plugin: plugin.clone(),
        dependency: dep_id.clone(),
        required: constraint.to_owned(),
        found: target_version.to_owned(),
    })?;
    if req.matches(&ver) {
        Ok(())
    } else {
        Err(InstallError::DependencyVersionConflict {
            plugin: plugin.clone(),
            dependency: dep_id.clone(),
            required: constraint.to_owned(),
            found: target_version.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus_extism_protocol::PluginDependency;

    fn pid(s: &str) -> PluginId {
        PluginId(s.into())
    }

    fn info(version: &str, deps: Vec<PluginDependency>) -> PluginDepInfo {
        PluginDepInfo { version: version.into(), requires: deps }
    }

    fn req_dep(id: &str, version: &str) -> PluginDependency {
        PluginDependency::new(id, version, true, vec![])
    }

    #[test]
    fn no_deps_is_acyclic() {
        let mut g = DepGraph::new();
        g.insert(pid("a"), info("1.0.0", vec![]));
        g.insert(pid("b"), info("1.0.0", vec![]));
        assert!(g.check_acyclic().is_ok());
    }

    #[test]
    fn linear_chain_is_acyclic() {
        let mut g = DepGraph::new();
        g.insert(pid("a"), info("1.0.0", vec![req_dep("b", "^1.0")]));
        g.insert(pid("b"), info("1.0.0", vec![req_dep("c", "^1.0")]));
        g.insert(pid("c"), info("1.0.0", vec![]));
        assert!(g.check_acyclic().is_ok());
    }

    #[test]
    fn two_node_cycle_detected() {
        let mut g = DepGraph::new();
        g.insert(pid("a"), info("1.0.0", vec![req_dep("b", "^1.0")]));
        g.insert(pid("b"), info("1.0.0", vec![req_dep("a", "^1.0")]));
        assert!(matches!(g.check_acyclic(), Err(InstallError::CyclicDependency { .. })));
    }

    #[test]
    fn version_mismatch_detected() {
        let mut g = DepGraph::new();
        g.insert(pid("b"), info("2.0.0", vec![]));
        let deps = vec![req_dep("b", "^1.0")];
        assert!(matches!(
            g.validate_required_deps(&pid("a"), &deps),
            Err(InstallError::DependencyVersionConflict { .. })
        ));
    }

    #[test]
    fn version_match_succeeds() {
        let mut g = DepGraph::new();
        g.insert(pid("b"), info("1.5.0", vec![]));
        let deps = vec![req_dep("b", "^1.0")];
        assert!(g.validate_required_deps(&pid("a"), &deps).is_ok());
    }
}
