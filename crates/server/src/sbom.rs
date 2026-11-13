//! `GET /sbom.cdx.json`: the software bill of materials, a `CycloneDX` 1.6
//! JSON document that names everything the running instance is made of.
//!
//! Three lists go in it:
//! - the Rust crates compiled into this binary, recorded at build time by
//!   `build.rs` (the normal-dependency closure of this crate for the build
//!   target, without the workspace crates: they are the application) and
//!   embedded here. The procedural macros among them, and the crates only
//!   they use, run inside the compiler and are not in the binary: they get
//!   the `CycloneDX` scope `excluded`, every other component `required`;
//! - the npm packages bundled into the frontend, from `sbom.web.json` next
//!   to `index.html` in the frontend folder, which `vite build` writes. A
//!   frontend without the file (an old build, a test) lists no npm packages;
//! - the ffmpeg build the server uses, as the capability check reports it,
//!   with a license only when the build says it is GPL version 3 or later.
//!
//! The Settings page shows the document, runtime and build time apart, and
//! links to it for a download. The desktop app serves the same list: the Tauri crates around
//! the embedded server are not in it.

use std::path::Path;
use std::sync::{Arc, LazyLock};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use muxmultitool_core::ffmpeg::capabilities::Capabilities;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tracing::{debug, warn};

use crate::{AppState, VERSION};

/// What `build.rs` recorded: the crates, the build time and the serial number.
const BUILD_RECORD: &str = include_str!(concat!(env!("OUT_DIR"), "/sbom-crates.json"));

/// The file `vite build` writes next to `index.html` with the npm packages.
pub const WEB_FILE_NAME: &str = "sbom.web.json";

/// The route's path.
pub const PATH: &str = "/sbom.cdx.json";

/// The `CycloneDX` media type, with the spec version.
const MEDIA_TYPE: &str = "application/vnd.cyclonedx+json; version=1.6";

/// The crates compiled into this binary, as `build.rs` wrote them.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildRecord {
	serial_number: String,
	timestamp: String,
	crates: Vec<Crate>,
}

#[derive(Debug, Deserialize)]
struct Crate {
	name: String,
	version: String,
	/// The manifest's `license` field; `None` for a crate that only ships a
	/// `license-file`.
	license: Option<String>,
	repository: Option<String>,
	/// A procedural macro, or a crate only procedural macros use: it runs
	/// while compiling and is not in the binary.
	#[serde(default, rename = "buildOnly")]
	build_only: bool,
}

/// The npm packages bundled into the frontend, as `vite build` wrote them.
#[derive(Debug, Default, Deserialize)]
struct WebRecord {
	packages: Vec<NpmPackage>,
}

#[derive(Debug, Deserialize)]
struct NpmPackage {
	name: String,
	version: String,
	license: Option<String>,
}

static BUILD: LazyLock<BuildRecord> = LazyLock::new(|| match serde_json::from_str(BUILD_RECORD) {
	Ok(record) => record,
	Err(error) => {
		warn!(%error, "the embedded crate list is unreadable; the SBOM lists no crates");
		BuildRecord::default()
	}
});

/// The SBOM route, to merge into the app router.
pub(crate) fn routes() -> Router<Arc<AppState>> {
	Router::new().route(PATH, get(sbom))
}

async fn sbom(State(state): State<Arc<AppState>>) -> Response {
	let web = read_web_record(&state.frontend_dir).await;
	// `get` runs `ffmpeg -version` and answers from the cache when the version
	// matches: no full detection per request.
	let ffmpeg = match state.capabilities.get(&state.runner).await {
		Ok(found) => Some(found),
		Err(error) => {
			debug!(%error, "no ffmpeg component in the SBOM");
			None
		}
	};
	let document = document(&BUILD, &web, ffmpeg.as_deref());
	match serde_json::to_vec(&document) {
		Ok(body) => (
			StatusCode::OK,
			[(header::CONTENT_TYPE, HeaderValue::from_static(MEDIA_TYPE))],
			body,
		)
			.into_response(),
		Err(error) => {
			warn!(%error, "cannot serialize the SBOM");
			StatusCode::INTERNAL_SERVER_ERROR.into_response()
		}
	}
}

/// `sbom.web.json` from the frontend folder; empty when it is missing or
/// unreadable, which is not an error: the Rust side of the SBOM still holds.
async fn read_web_record(frontend_dir: &Path) -> WebRecord {
	let path = frontend_dir.join(WEB_FILE_NAME);
	match tokio::fs::read(&path).await {
		Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
			warn!(path = %path.display(), %error, "unreadable npm package list; the SBOM lists no npm packages");
			WebRecord::default()
		}),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => WebRecord::default(),
		Err(error) => {
			warn!(path = %path.display(), %error, "cannot read the npm package list");
			WebRecord::default()
		}
	}
}

/// The `CycloneDX` document.
fn document(build: &BuildRecord, web: &WebRecord, ffmpeg: Option<&Capabilities>) -> Value {
	let mut components: Vec<Value> = build.crates.iter().map(crate_component).collect();
	components.extend(web.packages.iter().map(npm_component));
	components.extend(ffmpeg.map(ffmpeg_component));
	let refs: Vec<Value> = components.iter().map(|c| c["bom-ref"].clone()).collect();

	json!({
		"bomFormat": "CycloneDX",
		"specVersion": "1.6",
		"serialNumber": build.serial_number,
		"version": 1,
		"metadata": {
			"timestamp": build.timestamp,
			"component": {
				"type": "application",
				"bom-ref": "muxmultitool",
				"name": "muxmultitool",
				"version": VERSION,
				"licenses": [license_entry(env!("CARGO_PKG_LICENSE"))],
				"externalReferences": [{ "type": "vcs", "url": env!("CARGO_PKG_REPOSITORY") }],
			},
		},
		"components": components,
		"dependencies": [{ "ref": "muxmultitool", "dependsOn": refs }],
	})
}

fn crate_component(krate: &Crate) -> Value {
	let purl = cargo_purl(&krate.name, &krate.version);
	let mut component = component(
		"library",
		&purl,
		&krate.name,
		&krate.version,
		krate.license.as_deref(),
	);
	component.insert("purl".into(), purl.into());
	if krate.build_only {
		component.insert("scope".into(), SCOPE_BUILD_ONLY.into());
	}
	if let Some(repository) = &krate.repository {
		component.insert(
			"externalReferences".into(),
			json!([{ "type": "vcs", "url": repository }]),
		);
	}
	Value::Object(component)
}

fn npm_component(package: &NpmPackage) -> Value {
	let purl = npm_purl(&package.name, &package.version);
	let mut component = component(
		"library",
		&purl,
		&package.name,
		&package.version,
		package.license.as_deref(),
	);
	component.insert("purl".into(), purl.into());
	Value::Object(component)
}

/// ffmpeg carries a license only when the build reports `--enable-gpl` and
/// `--enable-version3`; any other build (LGPL, GPL-2.0-or-later) is left
/// without one rather than guessed.
fn ffmpeg_component(found: &Capabilities) -> Value {
	let license = found.is_gpl3().then_some("GPL-3.0-or-later");
	let mut component = component("application", "ffmpeg", "ffmpeg", &found.version, license);
	component.insert(
		"externalReferences".into(),
		json!([{ "type": "website", "url": "https://ffmpeg.org" }]),
	);
	Value::Object(component)
}

/// The `CycloneDX` scope of a component that is in the running program.
const SCOPE_RUNTIME: &str = "required";

/// The `CycloneDX` scope of a component that is not: the spec's "not
/// reachable within a call graph at runtime".
const SCOPE_BUILD_ONLY: &str = "excluded";

/// The fields every component has, with the runtime scope. `license`
/// becomes a `licenses` entry when present.
fn component(
	kind: &str,
	bom_ref: &str,
	name: &str,
	version: &str,
	license: Option<&str>,
) -> Map<String, Value> {
	let mut component = Map::new();
	component.insert("type".into(), kind.into());
	component.insert("bom-ref".into(), bom_ref.into());
	component.insert("name".into(), name.into());
	component.insert("version".into(), version.into());
	component.insert("scope".into(), SCOPE_RUNTIME.into());
	if let Some(license) = license.map(str::trim).filter(|l| !l.is_empty()) {
		component.insert("licenses".into(), json!([license_entry(license)]));
	}
	component
}

/// A `CycloneDX` license choice: an SPDX `expression` when the text reads as
/// one, otherwise a named license (npm's `SEE LICENSE IN …`, free text).
/// Cargo's old `MIT/Apache-2.0` spelling means `OR`.
fn license_entry(text: &str) -> Value {
	let expression = text
		.split('/')
		.map(str::trim)
		.collect::<Vec<_>>()
		.join(" OR ");
	if is_spdx_expression(&expression) {
		json!({ "expression": expression })
	} else {
		json!({ "license": { "name": text } })
	}
}

/// Whether `text` has the shape of an SPDX expression: license ids (letters,
/// digits, `.`, `-`, `+`) joined by `AND`, `OR` and `WITH`, with parentheses.
/// It does not check the ids against the SPDX list.
fn is_spdx_expression(text: &str) -> bool {
	let mut expect_operand = true;
	let mut depth = 0i32;
	for token in text.split_whitespace() {
		let token = token.trim_start_matches(|c: char| {
			if c == '(' && expect_operand {
				depth += 1;
				true
			} else {
				false
			}
		});
		let token = token.trim_end_matches(|c: char| {
			if c == ')' {
				depth -= 1;
				true
			} else {
				false
			}
		});
		let is_operator = matches!(token, "AND" | "OR" | "WITH");
		let is_operand = !token.is_empty()
			&& !is_operator
			&& token
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
		if (expect_operand && !is_operand) || (!expect_operand && !is_operator) || depth < 0 {
			return false;
		}
		expect_operand = !expect_operand;
	}
	!expect_operand && depth == 0
}

/// Everything a purl component percent-encodes: all but the unreserved
/// characters (alphanumerics, `-`, `.`, `_`, `~`).
const PURL_ENCODED: &AsciiSet = &NON_ALPHANUMERIC
	.remove(b'-')
	.remove(b'.')
	.remove(b'_')
	.remove(b'~');

/// `pkg:cargo/<name>@<version>`.
fn cargo_purl(name: &str, version: &str) -> String {
	format!(
		"pkg:cargo/{}@{}",
		utf8_percent_encode(name, PURL_ENCODED),
		utf8_percent_encode(version, PURL_ENCODED)
	)
}

/// `pkg:npm/<name>@<version>`; a scoped name keeps its `/` between the
/// namespace and the name and encodes the `@` of the scope as `%40`.
fn npm_purl(name: &str, version: &str) -> String {
	let path: Vec<String> = name
		.split('/')
		.map(|segment| utf8_percent_encode(segment, PURL_ENCODED).to_string())
		.collect();
	format!(
		"pkg:npm/{}@{}",
		path.join("/"),
		utf8_percent_encode(version, PURL_ENCODED)
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn capabilities(configuration: &[&str]) -> Capabilities {
		Capabilities {
			version: "9.0.2".to_owned(),
			configuration: configuration.iter().map(|o| (*o).to_owned()).collect(),
			encoders: Vec::new(),
			decoders: Vec::new(),
			formats: Vec::new(),
			filters: Vec::new(),
			hwaccels: Vec::new(),
			bitstream_filters: Vec::new(),
		}
	}

	#[test]
	fn the_embedded_record_parses() {
		assert!(BUILD.serial_number.starts_with("urn:uuid:"));
		assert_ne!(BUILD.timestamp, "");
		assert!(BUILD.crates.iter().any(|c| c.name == "tokio"));
	}

	#[test]
	fn cargo_purls_encode_the_version() {
		assert_eq!(cargo_purl("tokio", "1.54.1"), "pkg:cargo/tokio@1.54.1");
		assert_eq!(
			cargo_purl("toml", "1.2.1+spec-1.1.0"),
			"pkg:cargo/toml@1.2.1%2Bspec-1.1.0"
		);
	}

	#[test]
	fn npm_purls_encode_the_scope() {
		assert_eq!(npm_purl("vue", "3.5.45"), "pkg:npm/vue@3.5.45");
		assert_eq!(
			npm_purl("@connectrpc/connect", "2.3.0"),
			"pkg:npm/%40connectrpc/connect@2.3.0"
		);
	}

	#[test]
	fn spdx_expressions_are_recognised() {
		for ok in [
			"MIT",
			"MIT OR Apache-2.0",
			"(MIT OR Apache-2.0) AND Unicode-3.0",
			"Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
			"GPL-3.0-or-later",
			"(Apache-2.0 AND BSD-3-Clause)",
			"LGPL-2.1+",
		] {
			assert!(is_spdx_expression(ok), "{ok}");
		}
		for bad in [
			"",
			"SEE LICENSE IN LICENSE.md",
			"MIT OR",
			"MIT Apache-2.0",
			"(MIT",
			"MIT)",
		] {
			assert!(!is_spdx_expression(bad), "{bad}");
		}
	}

	#[test]
	fn license_entries_are_expressions_or_names() {
		assert_eq!(
			license_entry("MIT OR Apache-2.0"),
			json!({ "expression": "MIT OR Apache-2.0" })
		);
		assert_eq!(
			license_entry("MIT/Apache-2.0"),
			json!({ "expression": "MIT OR Apache-2.0" })
		);
		assert_eq!(
			license_entry("Apache-2.0 / MIT"),
			json!({ "expression": "Apache-2.0 OR MIT" })
		);
		assert_eq!(
			license_entry("SEE LICENSE IN LICENSE.md"),
			json!({ "license": { "name": "SEE LICENSE IN LICENSE.md" } })
		);
	}

	#[test]
	fn ffmpeg_is_gpl3_only_with_both_options() {
		let gpl = ffmpeg_component(&capabilities(&["--enable-gpl", "--enable-version3"]));
		assert_eq!(gpl["type"], "application");
		assert_eq!(gpl["version"], "9.0.2");
		assert_eq!(gpl["licenses"][0]["expression"], "GPL-3.0-or-later");
		let gpl2 = ffmpeg_component(&capabilities(&["--enable-gpl"]));
		assert!(gpl2.get("licenses").is_none());
		let lgpl = ffmpeg_component(&capabilities(&[]));
		assert!(lgpl.get("licenses").is_none());
	}

	#[test]
	fn a_crate_without_license_field_has_no_licenses() {
		let component = crate_component(&Crate {
			name: "ring".to_owned(),
			version: "0.17.0".to_owned(),
			license: None,
			repository: Some("https://github.com/briansmith/ring".to_owned()),
			build_only: false,
		});
		assert!(component.get("licenses").is_none());
		assert_eq!(
			component["externalReferences"][0]["url"],
			"https://github.com/briansmith/ring"
		);
		assert_eq!(component["purl"], "pkg:cargo/ring@0.17.0");
		assert_eq!(component["scope"], "required");
	}

	#[test]
	fn a_build_only_crate_is_excluded() {
		let record: BuildRecord = serde_json::from_value(json!({
			"serialNumber": "urn:uuid:00000000-0000-4000-8000-000000000000",
			"timestamp": "2026-11-13T12:00:00Z",
			"crates": [
				{ "name": "tokio", "version": "1.54.1" },
				{ "name": "serde_derive", "version": "1.0.229", "buildOnly": true },
			],
		}))
		.unwrap();
		let components: Vec<Value> = record.crates.iter().map(crate_component).collect();
		assert_eq!(components[0]["scope"], "required");
		assert_eq!(components[1]["scope"], "excluded");
	}

	#[test]
	fn the_embedded_record_marks_proc_macros_build_only() {
		let by_name = |name: &str| BUILD.crates.iter().find(|c| c.name == name);
		assert!(by_name("tokio").is_some_and(|c| !c.build_only));
		assert!(by_name("serde_derive").is_some_and(|c| c.build_only));
	}

	#[test]
	fn the_document_has_the_cyclonedx_shape() {
		let build = BuildRecord {
			serial_number: "urn:uuid:00000000-0000-4000-8000-000000000000".to_owned(),
			timestamp: "2026-11-07T12:00:00Z".to_owned(),
			crates: vec![Crate {
				name: "tokio".to_owned(),
				version: "1.54.1".to_owned(),
				license: Some("MIT".to_owned()),
				repository: None,
				build_only: false,
			}],
		};
		let web = WebRecord {
			packages: vec![NpmPackage {
				name: "@vue/shared".to_owned(),
				version: "3.5.45".to_owned(),
				license: Some("MIT".to_owned()),
			}],
		};
		let document = document(&build, &web, None);
		assert_eq!(document["bomFormat"], "CycloneDX");
		assert_eq!(document["specVersion"], "1.6");
		assert_eq!(document["version"], 1);
		assert_eq!(document["metadata"]["timestamp"], "2026-11-07T12:00:00Z");
		assert_eq!(document["metadata"]["component"]["name"], "muxmultitool");
		assert_eq!(document["metadata"]["component"]["version"], VERSION);
		let components = document["components"].as_array().unwrap();
		assert_eq!(components.len(), 2);
		assert_eq!(components[0]["purl"], "pkg:cargo/tokio@1.54.1");
		assert_eq!(components[1]["purl"], "pkg:npm/%40vue/shared@3.5.45");
		assert!(components.iter().all(|c| c["scope"] == "required"));
		assert_eq!(
			document["dependencies"][0]["dependsOn"],
			json!(["pkg:cargo/tokio@1.54.1", "pkg:npm/%40vue/shared@3.5.45"])
		);
	}
}
