#!/bin/sh
# Checks the contract for breaking changes against a base branch (default: master).
# Until the base branch has a proto module there is nothing to compare with, so the check is skipped.
set -eu
cd "$(dirname "$0")"
base="${1:-master}"
if ! git cat-file -e "$base:crates/proto/buf.yaml" 2>/dev/null; then
	echo "buf breaking: $base has no proto module yet, skipping"
	exit 0
fi
exec buf breaking --against "../../.git#branch=$base,subdir=crates/proto"
