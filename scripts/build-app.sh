#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "The .app bundle script requires macOS. See docs/DEVELOPMENT.md for other platforms." >&2
  exit 1
fi
cargo build --locked --release --features desktop --bin opencad-ui
build_dir="$(cargo metadata --locked --offline --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
version="$(cargo metadata --locked --offline --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
target="$(rustc -vV | sed -n 's/^host: //p')"
python3 scripts/package-release.py --app-only --target "$target" --version "$version" --binary-dir "$build_dir/release"
