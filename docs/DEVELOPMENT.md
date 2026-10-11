# Development

## Prerequisites

- Rust through rustup. `rust-toolchain.toml` selects 1.93.1 with rustfmt and Clippy.
- Python 3 for generating original UI assets, notices and release packages.
- Git and the native compiler/linker for your platform.
- macOS: Xcode Command Line Tools (`xcode-select --install`).
- Windows: Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK. Use the MSVC Rust target.
- Ubuntu/Debian: `sudo apt-get install build-essential pkg-config libwayland-dev libxkbcommon-dev`. Running the GUI also needs a desktop session, OpenGL and the X11/Wayland runtime libraries.

## Run and build

```sh
cargo run --locked --features desktop --bin opencad-ui
cargo run --locked --features desktop --bin opencad-ui -- examples/courtyard.dxf
cargo build --locked --release --features desktop --bins
```

The default binary is the CLI: `cargo run --locked -- --help`. To work offline,
fetch dependencies once with `cargo fetch --locked`, then add `--offline` to Cargo
commands. `Cargo.lock` and the toolchain file define the reproducible dependency
inputs; platform binaries are not promised to be byte-identical across machines.

## Structure

| Path | Responsibility |
| --- | --- |
| `src/` geometry, DXF and editor modules | Original CAD model, validated exchange and editing operations |
| `src/desktop/` | egui canvas, commands, palettes and application state |
| `src/bin/` | Desktop entry point and macOS native menu/quit integration |
| `tests/` | Geometry, exchange, CLI and actual-widget behavior checks |
| `tests/fixtures/` | Synthetic plain-text DXF interoperability cases |
| `assets/ui/catalog.json` | Reviewed functional UI metadata |
| `assets/icons/` | Original generated SVG/PNG artwork |
| `scripts/` | Asset generation and native release packaging |

## Validation

Run the narrowest relevant test while working, then the full suite before shipping:

```sh
cargo test --locked --features desktop --test desktop_ui reference_start_page_preserves_dirty_drawing_and_reopens_recent_files -- --exact
cargo fmt --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features
```

Desktop tests exercise real widgets through accessibility actions and keyboard
input without opening a window. They check connected actions, focus, dirty-file
prompts, geometry and saved DXF results. Native OS chrome, file dialogs and GPU
presentation still need manual testing on the target platform.

## UI assets

The generated icons and catalog are checked in, so a normal Cargo build requires
neither Python nor any reference application. To regenerate after editing:

```sh
python3 scripts/generate-icons.py
python3 scripts/generate-ui-catalog.py
cargo fmt
```

All icon drawing primitives are original code in the icon generator. Keep new
assets original and preserve the documented [clean-room boundary](CLEAN_ROOM.md).
Do not add recovered proprietary resources or native implementation bodies.

## Screenshots

The README images are reproducible captures of actual OpenCad egui paint output:

```sh
OPENCAD_UI_SNAPSHOT_DIR="$PWD/docs/screenshots" cargo test --locked --features desktop --test desktop_ui reference_start_page_preserves_dirty_drawing_and_reopens_recent_files -- --exact
```

The command above uses a POSIX shell; in PowerShell, set
`$env:OPENCAD_UI_SNAPSHOT_DIR = "$PWD/docs/screenshots"` before running the same Cargo
command. Font metrics can differ across operating systems. These software captures
exclude native window title bars and menus.

## Releases

The `Build and release` workflow tests, lints, builds both binaries, smoke-tests
the CLI and packages four native targets. Main and pull request builds retain
artifacts for 30 days. A `v*` tag publishes a GitHub Release only after every target
has passed. Tags containing a hyphen become prereleases.

Maintainers should update the Cargo package version for a new version, keep the
numeric version consistent with the intended tag, validate the change, then push:

```sh
git tag -a v0.1.0-alpha.1 -m "OpenCad 0.1.0 alpha 1"
git push origin v0.1.0-alpha.1
```

To package an existing native build locally (example: Apple Silicon macOS):

```sh
cargo build --locked --release --features desktop --bins --target aarch64-apple-darwin
python3 scripts/dependency-notices.py --target aarch64-apple-darwin
python3 scripts/package-release.py --target aarch64-apple-darwin --version 0.1.0-alpha.1 --binary-dir target/aarch64-apple-darwin/release
```

Use the actual Cargo output directory if `CARGO_TARGET_DIR` is configured. Supported
targets are `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-pc-windows-msvc`, and `x86_64-unknown-linux-gnu`. Packaging runs on the native
OS. macOS bundles are ad hoc signed; notarization and Windows signing are not
configured. Linux archives target the Ubuntu 22.04 runtime baseline.

Before announcing a release, download its packages, verify `SHA256SUMS.txt`, and
exercise native launch/open/save on the relevant operating systems.
