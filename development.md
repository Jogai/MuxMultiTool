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
### Running the server

`cargo run -p muxmultitool-server` starts the axum server on `127.0.0.1:8080`. It mounts the ConnectRPC services under `/rpc`, serves the built frontend (`web/dist`, from `npm run build`) at `/` and logs to stderr (`RUST_LOG` filters, default `info`). Every setting is a flag with an environment variable behind it, so a compose file needs no command line; `--help` lists them:

| Setting | Flag | Variable | Default |
|---|---|---|---|
| Listen address | `--listen` | `MMT_LISTEN` | `127.0.0.1` (use `0.0.0.0` in a container) |
| Port | `--port` | `MMT_PORT` | `8080` |
| Media folders | `--media-root`, repeatable | `MMT_MEDIA_ROOTS`, comma-separated | none |
| ffmpeg, ffprobe | `--ffmpeg`, `--ffprobe` | `MMT_FFMPEG`, `MMT_FFPROBE` | found on `PATH` |
| Frontend folder | `--frontend-dir` | `MMT_FRONTEND_DIR` | `web/dist` |

Ctrl-C or `SIGTERM` (what `docker stop` sends) stops it cleanly. The health method is a plain JSON call, which also serves as the container `HEALTHCHECK`:

```sh
curl -X POST -H 'content-type: application/json' -d '{}' \
  http://127.0.0.1:8080/rpc/muxmultitool.system.v1.SystemService/GetHealth
# {"status":"HEALTH_STATUS_OK"}
```

The frontend sends JSON in development (readable in devtools) and binary protobuf in release builds; the server answers in whichever encoding a request uses.

### Editor setup and git hooks

`.editorconfig` sets tabs as the baseline (YAML and `.proto` use two spaces). `.vscode/` makes the oxc extension the formatter for `.ts` and `.vue`, turns off Vue (Official)'s formatter and sets tab indentation; accept the recommended extensions when VS Code asks.

Pre-commit hooks run the fast format and lint checks through [lefthook](https://lefthook.dev) (`lefthook.yml`). Install them once per clone:

```sh
cd web && npm ci && npx lefthook install
```

The hooks also need `typos` (`cargo install typos-cli --locked`), `committed` (`cargo install committed --locked`) and, for Rust files, the toolchain from `rust-toolchain.toml`. Run `typos` by hand to check spelling across the repo.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat(core): add the job queue`), because the release version is worked out from them. The rules are in `committed.toml`; the commit-msg hook and CI check them.

### CI

GitHub Actions runs the checks above on every pull request and on `master` (`.github/workflows/ci.yml`). The `changes` job looks at the changed paths and skips what they don't touch:

| Changed | Runs |
|---|---|
| `crates/core/**`, `crates/proto/**`, `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `.github/**` | Everything |
| `crates/server/**`, `crates/desktop/**`, `Dockerfile`, `tauri.conf.json`, the root `*.toml` configs | Backend checks |
| `web/**` | Frontend checks |
| `crates/proto/**` | RPC contract checks (lint, format, breaking changes) |
| anything else (readme, docs) | Nothing |

The jobs: `codegen` runs `buf generate` once and hands the result to the others; `backend-lint`, `backend-test` (nextest with coverage, needs ffmpeg) and `backend-musl` (`cargo zigbuild` of the server for `x86_64-unknown-linux-musl`, so a C dependency can't slip in); `frontend` runs the `npm run ci` steps one by one, then the unit tests; `contract` runs the buf checks; `commits` runs `committed` over the new commits. `CI` is the one status check to require in branch protection: it is green when every job passed or was skipped. Cargo builds are cached with `Swatinem/rust-cache`, npm with `actions/setup-node`.

A daily workflow (`.github/workflows/security.yml`) runs `cargo deny check advisories` and `npm audit`.

Without a GitHub remote, check the workflows with [actionlint](https://github.com/rhysd/actionlint) and run the steps' commands locally.

### Frontend

The Vue app lives in `web/`. From there: `npm ci`, `npm run dev`, and `npm run ci` for the format, lint, type-check and build steps.

## License

MuxMultiTool is licensed under the [GNU General Public License v3.0 or later](https://www.gnu.org/licenses/gpl-3.0.html).

SPDX-License-Identifier: GPL-3.0-or-later
