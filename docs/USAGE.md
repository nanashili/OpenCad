# Usage and format support

## Desktop

Start with **New**, **Open**, or the included `examples/courtyard.dxf`. Enter exact
coordinates in the command field or draw on the canvas. The current viewport is
an XY top view; supported Z/elevation values are retained in the model and DXF.

Useful command examples:

```text
LINE 0,0,3 10,20,3
CIRCLE 20,20,3 5
PLINE 0,0 10,0 10,10 CLOSE
SELECT ALL
MOVE 10,0,0
UNDO
REDO
```

Use the toolsets and Properties palette to inspect and edit supported entities.
Enter finishes a line chain or open polyline; Esc cancels a draft. A polyline is
one selection and undo object. Bulges specify curved segments: zero is straight,
+1 is a counterclockwise semicircle, and -1 is clockwise. The last vertex's bulge
controls the closing segment when closed.

Tool keys: **V** select, **L** line, **C** circle, **A** arc, **R** rectangle,
**M** move, **P** pan, **F** fit. **F3** toggles object snap, **F7** grid, **F8**
ortho, **F9** grid snap and **F10** polar tracking. Shift temporarily enables
ortho while drawing. Middle/right drag pans; scroll zooms.

On macOS, Command-N/O/S create/open/save and Command-Z/Shift-Command-Z undo/redo.
Command-W closes through the dirty-file prompt. Command-1 toggles Properties,
Command-3 the command area, Command-5 toolsets and F2 history. Text fields retain
their own editing shortcuts. Returning to Start keeps the active drawing.

## Reordering toolset panels

Click **Reorder panels** at the bottom left of the Drafting toolset. Drag a panel
name within the list to move it; the drawing toolset updates immediately. Press
Escape or click outside the list to close it. Assistive technology can use the
**Move panel up** and **Move panel down** actions on each eligible row.

Panel order is saved after each move and applies in both full and slim views.
The current implementation reorders the existing Drafting panels. Creating
custom panels and editing the Modeling toolset remain unfinished; **Add panel**
is disabled.

## Workspace preferences

OpenCad remembers palette visibility, full/slim toolset width, panel order,
collapsed toolset sections, command visibility, status-item visibility, recent
files and the recent-file list/grid presentation. Preferences are saved on
normal exit and every 30 seconds; panel moves also save immediately. Recent
files retain their absolute paths and most-recent-first order, with up to 20
entries. Missing files remain listed and report an error if
opened. Drawing contents and unsaved edits are not stored in preferences.

The native app stores preferences and window geometry in `app.ron`:

| Platform | Location |
| --- | --- |
| macOS | `~/Library/Application Support/org.opendesk.opencad/app.ron` |
| Windows | `%APPDATA%\org.opendesk.opencad\data\app.ron` |
| Linux | `$XDG_DATA_HOME/org.opendesk.opencad/app.ron`, or `~/.local/share/org.opendesk.opencad/app.ron` |

To reset preferences and window geometry, close OpenCad and remove that file.
Unreadable workspace settings fall back to defaults. The Connect panel remains
an interface placeholder; account sign-in and online services are not connected.

## DXF boundary

The editor models a limited subset of **R12 ASCII DXF**:

- LINE with XYZ endpoints.
- CIRCLE and ARC in the world XY plane, retaining supported elevation.
- Legacy 2D POLYLINE with straight or bulged XY segments at one elevation.
- Named layers with visibility, locking, indexed colors and simple linetypes.
- Supported entity style overrides and global/entity linetype scale.

The document model preserves validated source text and patches edited fields and
records. Validated header, table and block metadata can survive copy/edit
roundtrips without being editable or rendered. Unsupported entity types in the
ENTITIES section are rejected. Successful exchange of these fixtures does not establish general
AutoCAD compatibility. DWG, arbitrary entity planes, 3D polylines, widths, complex
linetypes, dimensions, solids and broad block workflows are not supported.

## CLI and library

```sh
opencad --help
opencad shape.svg 0 0 30 0 30 20 --close
opencad line3 line.dxf 0 0 3 10 20 3
opencad dxf-copy input.dxf output.dxf
opencad dxf-set-line input.dxf output.dxf 1 0 0 3 10 20 3
```

`dxf-set-line` addresses LINE entities by their one-based LINE index. The SVG
command accepts `--undo`, `--close` and `--break` to build separate chains, with
positive Y upward. Input is validated before writing output.

The Rust library exposes geometry types, the source-preserving `DxfDocument`,
and an `Editor` with validation and undo/redo. `read_dxf` is a LINE-only convenience
API; use `DxfDocument` for supported mixed drawings. `write_dxf` writes minimal
LINE documents. See the public types and tests for precise contracts.
