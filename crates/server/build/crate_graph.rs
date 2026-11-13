//! The walk over `cargo metadata` behind the crate list of the SBOM.
//!
//! Compiled twice: into `build.rs` (`#[path]`), which records the list, and
//! into the library's unit tests only, so the walk is tested on a small
//! graph without a build. It depends on `std` and `serde_json` alone, which
//! both the build script and the library have.
//!
//! The list is the normal-dependency closure of the server for the build
//! target, without the workspace crates. A crate in it is *build time* when
//! it does not end up in the binary: a procedural macro, which runs inside
//! the compiler, or a crate reachable only through procedural macros (`syn`,
//! `quote`, …). Every other crate is *runtime*.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde_json::{Map, Value};

/// The packages of `cargo metadata`, by id.
pub fn packages(metadata: &Value) -> Result<HashMap<&str, &Value>, String> {
	metadata["packages"]
		.as_array()
		.ok_or("cargo metadata has no packages")?
		.iter()
		.map(|package| {
			Ok((
				package["id"].as_str().ok_or("a package has no id")?,
				package,
			))
		})
		.collect()
}

/// Whether the package is one of the workspace's own crates.
pub fn is_workspace_member(metadata: &Value, package: &Value) -> bool {
	metadata["workspace_members"]
		.as_array()
		.is_some_and(|members| members.iter().any(|member| member == &package["id"]))
}

/// Whether the package is a procedural macro: one of its targets has the
/// `proc-macro` kind.
fn is_proc_macro(package: &Value) -> bool {
	package["targets"]
		.as_array()
		.into_iter()
		.flatten()
		.any(|target| {
			target["kind"]
				.as_array()
				.is_some_and(|kinds| kinds.iter().any(|kind| kind == "proc-macro"))
		})
}

/// The id of the package whose manifest is `manifest`.
pub fn package_id<'a>(metadata: &'a Value, manifest: &Path) -> Result<&'a str, String> {
	packages(metadata)?
		.into_values()
		.find(|package| {
			package["manifest_path"]
				.as_str()
				.is_some_and(|path| Path::new(path) == manifest)
		})
		.and_then(|package| package["id"].as_str())
		.ok_or_else(|| format!("{} is not in cargo metadata", manifest.display()))
}

/// The normal-dependency closure of `root`, sorted by name and version,
/// without the workspace crates. Each entry has the name, version, license
/// and repository, and `"buildOnly": true` when the crate is build time.
pub fn dependency_crates(metadata: &Value, root: &str) -> Result<Vec<Value>, String> {
	let packages = packages(metadata)?;
	let nodes: HashMap<&str, &Value> = metadata["resolve"]["nodes"]
		.as_array()
		.ok_or("cargo metadata has no resolve graph")?
		.iter()
		.filter_map(|node| Some((node["id"].as_str()?, node)))
		.collect();

	let everything = closure(&nodes, root, &|_| true)?;
	// The same walk without entering a procedural macro: what is left out is
	// reachable only through one.
	let runtime = closure(&nodes, root, &|id| {
		!packages
			.get(id)
			.is_some_and(|package| is_proc_macro(package))
	})?;

	let mut crates: Vec<Value> = everything
		.into_iter()
		.filter_map(|id| Some((id, *packages.get(id)?)))
		.filter(|(_, package)| !is_workspace_member(metadata, package))
		.map(|(id, package)| {
			let mut entry = Map::new();
			entry.insert("name".into(), package["name"].clone());
			entry.insert("version".into(), package["version"].clone());
			if let Some(license) = package["license"].as_str() {
				entry.insert("license".into(), license.into());
			}
			if let Some(repository) = package["repository"].as_str() {
				entry.insert("repository".into(), repository.into());
			}
			if !runtime.contains(id) {
				entry.insert("buildOnly".into(), true.into());
			}
			Value::Object(entry)
		})
		.collect();
	crates.sort_by(|a, b| {
		(a["name"].as_str(), a["version"].as_str())
			.cmp(&(b["name"].as_str(), b["version"].as_str()))
	});
	Ok(crates)
}

/// The ids reachable from `root` along normal dependencies, `root`
/// included, entering only the packages `enter` accepts.
fn closure<'a>(
	nodes: &HashMap<&'a str, &'a Value>,
	root: &'a str,
	enter: &dyn Fn(&str) -> bool,
) -> Result<BTreeSet<&'a str>, String> {
	let mut seen = BTreeSet::new();
	let mut pending = vec![root];
	while let Some(id) = pending.pop() {
		if !seen.insert(id) {
			continue;
		}
		let node = nodes
			.get(id)
			.ok_or_else(|| format!("{id} is not in the resolve graph"))?;
		for dep in node["deps"].as_array().into_iter().flatten() {
			// `kind` is null for a normal dependency, "dev" or "build" otherwise.
			// `--filter-platform` already dropped the entries for other targets.
			let normal = dep["dep_kinds"]
				.as_array()
				.is_some_and(|kinds| kinds.iter().any(|kind| kind["kind"].is_null()));
			if let (true, Some(pkg)) = (normal, dep["pkg"].as_str())
				&& enter(pkg)
			{
				pending.push(pkg);
			}
		}
	}
	Ok(seen)
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	/// A package with one target of `kind` ("lib" or "proc-macro").
	fn package(id: &str, kind: &str) -> Value {
		json!({
			"id": id,
			"name": id,
			"version": "1.0.0",
			"license": "MIT",
			"manifest_path": format!("/src/{id}/Cargo.toml"),
			"targets": [{ "kind": [kind] }],
		})
	}

	/// A resolve node; a dependency's kind is `None` for a normal one.
	fn node(id: &str, deps: &[(&str, Option<&str>)]) -> Value {
		let deps: Vec<Value> = deps
			.iter()
			.map(|(pkg, kind)| json!({ "pkg": pkg, "dep_kinds": [{ "kind": kind }] }))
			.collect();
		json!({ "id": id, "deps": deps })
	}

	/// The server depends on tokio (with its macros), serde (with its derive),
	/// `shared` (also used by `serde_derive`), the workspace crate `core`, a
	/// build-dependency and a dev-dependency.
	fn metadata() -> Value {
		json!({
			"workspace_members": ["server", "core"],
			"packages": [
				package("server", "lib"),
				package("core", "lib"),
				package("tokio", "lib"),
				package("tokio-macros", "proc-macro"),
				package("serde", "lib"),
				package("serde_derive", "proc-macro"),
				package("syn", "lib"),
				package("proc-macro2", "lib"),
				package("shared", "lib"),
				package("cc", "lib"),
				package("insta", "lib"),
			],
			"resolve": { "nodes": [
				node("server", &[
					("core", None),
					("tokio", None),
					("serde", None),
					("shared", None),
					("cc", Some("build")),
					("insta", Some("dev")),
				]),
				node("core", &[("tokio", None)]),
				node("tokio", &[("tokio-macros", None)]),
				node("tokio-macros", &[("syn", None), ("proc-macro2", None)]),
				node("serde", &[("serde_derive", None)]),
				node("serde_derive", &[("syn", None), ("shared", None)]),
				node("syn", &[("proc-macro2", None)]),
				node("proc-macro2", &[]),
				node("shared", &[]),
				node("cc", &[]),
				node("insta", &[]),
			]},
		})
	}

	fn listed(crates: &[Value]) -> Vec<(String, bool)> {
		crates
			.iter()
			.map(|krate| {
				(
					krate["name"].as_str().unwrap().to_owned(),
					krate["buildOnly"] == true,
				)
			})
			.collect()
	}

	#[test]
	fn proc_macros_and_what_only_they_use_are_build_time() {
		let crates = dependency_crates(&metadata(), "server").unwrap();
		assert_eq!(
			listed(&crates),
			[
				("proc-macro2".to_owned(), true),
				("serde".to_owned(), false),
				("serde_derive".to_owned(), true),
				// Used by a proc-macro and by the server: it is in the binary
				("shared".to_owned(), false),
				("syn".to_owned(), true),
				("tokio".to_owned(), false),
				("tokio-macros".to_owned(), true),
			]
		);
	}

	#[test]
	fn a_runtime_crate_has_no_build_only_field() {
		let crates = dependency_crates(&metadata(), "server").unwrap();
		let tokio = crates.iter().find(|c| c["name"] == "tokio").unwrap();
		assert_eq!(
			tokio,
			&json!({ "name": "tokio", "version": "1.0.0", "license": "MIT" })
		);
	}

	#[test]
	fn the_root_is_found_by_its_manifest() {
		let metadata = metadata();
		assert_eq!(
			package_id(&metadata, Path::new("/src/server/Cargo.toml")).unwrap(),
			"server"
		);
		assert!(package_id(&metadata, Path::new("/elsewhere/Cargo.toml")).is_err());
	}

	#[test]
	fn a_dependency_missing_from_the_graph_is_an_error() {
		let mut metadata = metadata();
		metadata["resolve"]["nodes"]
			.as_array_mut()
			.unwrap()
			.retain(|node| node["id"] != "syn");
		let error = dependency_crates(&metadata, "server").unwrap_err();
		assert_eq!(error, "syn is not in the resolve graph");
	}
}
