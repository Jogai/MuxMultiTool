# MuxMultiTool development

How to build, run and check MuxMultiTool, and how its services work. For what MuxMultiTool is, see the [readme](readme.md).

## Development

### Backend checks

The Rust workspace lives in `crates/`. `rust-toolchain.toml` pins the Rust version, so `rustup` installs the right toolchain, components and targets on the first `cargo` call. Run these before pushing; CI runs the same commands.

| Check | Command |
|---|---|
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` |
| Docs | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |
| TOML | `taplo fmt --check` (`cargo install taplo-cli`, or `npx @taplo/cli fmt --check`) |
| Dependencies | `cargo deny check` (`cargo install cargo-deny`): RustSec advisories, licenses, bans and sources, configured in `deny.toml` |
| Unused dependencies | `cargo shear` (`cargo install cargo-shear`) |
| Feature combinations | `cargo hack check --each-feature --workspace --locked` (`cargo install cargo-hack`) |

To fix formatting instead of checking it, drop `--check`. Lints are defined once in the root `Cargo.toml` and every crate inherits them; project rules such as the disallowed `Command::new` (use `core::ffmpeg::Runner`) are in `clippy.toml`. Indentation is tabs everywhere the format allows it (see `.editorconfig`, `rustfmt.toml` and `taplo.toml`).

### Dependency rules

`deny.toml` holds the policy for every crate in the tree:

- **Licenses** must be compatible with GPL-3.0-or-later. The allow-list is the permissive licenses, MPL-2.0 and the GPL/LGPL family; anything else fails the check and needs a per-crate exception with a reason.
- **Bans:** crates that link a C library (`openssl-sys`, `native-tls`, `libz-sys`, `libsqlite3-sys`, ...) are banned because they break the static musl container build. Use rustls and pure-Rust codecs instead. Duplicate versions of a crate and wildcard version requirements are errors; a temporary duplicate after an update goes in `skip` with a reason.
- **Sources:** crates.io only; git dependencies must be listed in `allow-git`.

Release binaries are built with cargo-auditable (`cargo install cargo-auditable`), which embeds the dependency list in the binary so image scanners can see the Rust crates:

```sh
cargo auditable build --release -p muxmultitool-server
```

### RPC contract and generated code

All frontend-backend calls are ConnectRPC. The contract lives in `crates/proto/proto/` and `buf` generates the Rust and TypeScript code from it. Generated code is not committed (`crates/proto/src/generated/` and `web/src/generated/` are git-ignored), so generate it before building, linting or testing either side, and again after every change to a `.proto` file.

One-time setup (`buf` must be on the PATH, see https://buf.build/docs/installation):

```sh
cargo install --locked protoc-gen-buffa protoc-gen-buffa-packaging connectrpc-codegen
(cd web && npm ci)   # installs protoc-gen-es
```

Regenerate:

```sh
cd crates/proto && buf generate
```

Contract checks (CI runs the same):

| Check | Command |
|---|---|
| Lint | `buf lint` in `crates/proto` |
| Format | `buf format -d --exit-code` in `crates/proto` |
| Breaking changes | `crates/proto/breaking.sh [base-branch]`, which runs `buf breaking` against `master` (skipped until `master` has the proto module) |

Methods that have no side effects must declare `option idempotency_level = NO_SIDE_EFFECTS;` so Connect may send them as `GET`. The `SystemService` in `crates/proto/proto/muxmultitool/system/v1/system.proto` is the example to copy.
### Editor setup and git hooks

`.editorconfig` sets tabs as the baseline (YAML and `.proto` use two spaces). `.vscode/` makes the oxc extension the formatter for `.ts` and `.vue`, turns off Vue (Official)'s formatter and sets tab indentation; accept the recommended extensions when VS Code asks.

Pre-commit hooks run the fast format and lint checks through [lefthook](https://lefthook.dev) (`lefthook.yml`). Install them once per clone:

```sh
cd web && npm ci && npx lefthook install
```

The hooks also need `typos` (`cargo install typos-cli --locked`) and, for Rust files, the toolchain from `rust-toolchain.toml`. Run `typos` by hand to check spelling across the repo.

## License

MuxMultiTool is licensed under the [GNU General Public License v3.0 or later](https://www.gnu.org/licenses/gpl-3.0.html).

SPDX-License-Identifier: GPL-3.0-or-later

## Frontend

The Vue app lives in `web/`. From there: `npm ci`, `npm run dev`, and `npm run ci` for the format, lint, type-check and build steps.
