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

To fix formatting instead of checking it, drop `--check`. Lints are defined once in the root `Cargo.toml` and every crate inherits them; project rules such as the disallowed `Command::new` (use `core::ffmpeg::Runner`) are in `clippy.toml`. Indentation is tabs everywhere the format allows it (see `.editorconfig`, `rustfmt.toml` and `taplo.toml`).

## License

MuxMultiTool is licensed under the [GNU General Public License v3.0 or later](https://www.gnu.org/licenses/gpl-3.0.html).

SPDX-License-Identifier: GPL-3.0-or-later

## Frontend

The Vue app lives in `web/`. From there: `npm ci`, `npm run dev`, and `npm run ci` for the format, lint, type-check and build steps.
