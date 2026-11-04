#!/usr/bin/env bash
# Writes one version into every place that carries it (development.md, "Releases"):
#
#   - the Cargo workspace: `[workspace.package].version` in Cargo.toml, the
#     versions of the path dependencies between the crates, and Cargo.lock
#     (`cargo set-version --workspace`, from cargo-edit)
#   - the frontend: web/package.json and web/package-lock.json (`npm version`)
#
# crates/desktop/tauri.conf.json has no `version`: Tauri reads it from the
# crate, which inherits it from the workspace, so there is no third place.
# The script checks that this stays so.
#
# Usage: scripts/set-version.sh 1.2.3   (a leading `v` is accepted)
set -euo pipefail

version="${1:?usage: scripts/set-version.sh <version>}"
version="${version#v}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
	echo "not a semver version: $version" >&2
	exit 1
fi

if jq --exit-status 'has("version")' crates/desktop/tauri.conf.json > /dev/null; then
	echo "crates/desktop/tauri.conf.json must not set \"version\": it inherits the workspace version from Cargo.toml" >&2
	exit 1
fi

cargo set-version --workspace "$version"
(cd web && npm version "$version" --no-git-tag-version --allow-same-version > /dev/null)

# Every copy must agree before the version is committed.
cargo_version="$(cargo metadata --format-version 1 --no-deps | jq --raw-output '.packages[] | select(.name == "muxmultitool-server") | .version')"
npm_version="$(jq --raw-output .version web/package.json)"
lock_version="$(jq --raw-output .version web/package-lock.json)"
if [[ "$cargo_version" != "$version" || "$npm_version" != "$version" || "$lock_version" != "$version" ]]; then
	echo "version mismatch after writing $version: Cargo $cargo_version, package.json $npm_version, package-lock.json $lock_version" >&2
	exit 1
fi
echo "version $version written to Cargo.toml, Cargo.lock, web/package.json and web/package-lock.json"
