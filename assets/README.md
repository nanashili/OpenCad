# UI assets

`ui/catalog.json` contains reviewed functional command names, menu and toolset
organization, and visibility metadata. It contains no native implementation,
command macros or recovered artwork. See [the clean-room process](../docs/CLEAN_ROOM.md).

`icons/` contains original OpenCad geometric line icons in SVG and PNG form,
created from the primitives in `scripts/generate-icons.py`. They are covered by
the repository's MIT license. No source application images or fonts are used.

Run from the repository root to reproduce the assets and Rust declarations:

```sh
python3 scripts/generate-icons.py
python3 scripts/generate-ui-catalog.py
cargo fmt
```

Icon filenames include a hash of the case-sensitive action ID so that distinct
commands remain distinct on case-insensitive filesystems.
