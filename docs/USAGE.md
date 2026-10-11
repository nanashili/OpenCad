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
**M** move, **O** offset, **T** trim, **E** extend, **P** pan, **F** fit.
**F3** toggles object snap, **F7** grid, **F8**
ortho, **F9** grid snap and **F10** polar tracking. Shift temporarily enables
ortho while drawing. Middle/right drag pans; scroll zooms.

On macOS, Command-N/O/S create/open/save and Command-Z/Shift-Command-Z undo/redo.
Command-W closes through the dirty-file prompt. Command-1 toggles Properties,
Command-3 the command area, Command-5 toolsets and F2 history. Text fields retain
their own editing shortcuts. Returning to Start keeps the active drawing.

## Rotate, scale and mirror

Select entities first, then choose **Rotate**, **Scale** or **Mirror** in Modify,
or enter `ROTATE` (`RO`), `SCALE` (`SC`) or `MIRROR` (`MI`). Base and mirror-line
points accept canvas clicks or `x,y[,z]` coordinates.

Rotate accepts an angle in degrees or a direction point from the base. Scale
accepts a positive numeric factor. After the base, **Copy** (`C`) retains the
source and **Reference** (`R`) accepts an old angle/length followed by a new
angle/length. Each reference can be measured with points. A new direction or
length point is measured from the base. For scaling with the mouse, use Reference.
Mirror asks **Erase source objects?**; **Yes** replaces them and **No** creates
copies. Enter defaults to No in OpenCad. Escape cancels without changing geometry.

Direct forms are also available:

```text
ROTATE 0,0 90 COPY
SCALE 0,0,3 2
MIRROR 0,0 0,10 YES
```

These operations support LINE, CIRCLE, ARC and 2D POLYLINE, including bulges.
Each selection transform is one undo step; copies retain source layers and styles.
OpenCad currently uses degrees counterclockwise from world +X, rotates about
world Z, and mirrors through a vertical plane containing the XY mirror line.
Rotation and mirror retain Z; uniform scaling scales XYZ about the base. Positive
scale factors and Mirror's No default are OpenCad policies. Drawing-specific UCS,
angular-unit settings, text, annotative objects and other entity types are unfinished.

## Offset

Choose **Offset** in Modify or enter `OFFSET` (`O`). Enter a positive distance or
choose **Through**, select a LINE, CIRCLE or ARC, then click or type the side or
through point. A source can also be selected by its one-based entity number.
The preview uses the same geometry as the committed result. Continue selecting
sources, or press Enter/choose **Exit** to finish. Escape cancels a pending pick.

**Erase** accepts Yes/No for removing each source. **Layer** accepts Current/Source
for the new curve's layer. Explicit source style overrides are copied; BYLAYER
properties resolve against the target layer. **Undo** (`U`) removes the most recent
offset in this command, including restoring an erased source. It stops at the
command's first offset and refuses to undo unrelated edits.

Direct forms are available:

```text
OFFSET 5 1 0,20
OFFSET THROUGH 2 10,30 ERASE CURRENT
```

OpenCad defaults to retaining sources on their source layer. Enter at the initial
distance prompt chooses Through. These defaults are OpenCad policies; AutoCAD
2027 Mac defaults have not been verified. Offsets use world XY, preserve elevation
and arc angles, and reject collapsed radii, ambiguous sides and nonrepresentable
results. Unequal-Z lines, polyline joins, other curve types, distance measurement
with two picks, Multiple mode and OFFSETDIST are unfinished.

## Trim and extend

Choose **Trim** or **Extend** in Modify, or enter `TRIM` (`TR`, `T`) or
`EXTEND` (`EX`, `E`). Select cutting/boundary edges on the canvas or enter their
one-based entity numbers, then press Enter. A preselection becomes the initial
boundary selection. Enter with no boundaries selected, or **All**, uses all
visible curves as boundaries.

Pick the portion to remove when trimming. Pick near the endpoint to move when
extending. Hold Shift to perform the other operation for that pick. The preview
highlights the retained or extended geometry; these picks use the raw cursor
position so object snap and ortho do not change the selected portion. Exact picks
can be entered as `entity-number x,y[,z]`.

**Edge** accepts Extend/NoExtend for using boundary supporting lines/full circles
or their finite extents. **Cutting edges** starts a new boundary selection.
**Undo** (`U`) reverses only operations in the current command. Enter or **Exit**
finishes, and Escape cancels a pending pick. Direct OpenCad forms are:

```text
TRIM 1 5,0 2,3
EXTEND 1 10,0 ALL EDGEEXTEND
```

Targets are equal-Z world-XY LINE, CIRCLE and ARC for trimming, and LINE/ARC for
extending. Boundaries can also be polyline segments, including bulges, at the
same elevation. Retained fragments copy source style/layer; splitting and circle
conversion undo in one step. Picks on a cut or equally near both endpoints are
rejected. Tangencies count as intersections, coincident supports are ignored,
and a circle needs two distinct cuts. These numerical choices and the initial
Standard/NoExtend mode are OpenCad policies, not verified native defaults.
Polyline targets, other entities, Quick mode, Fence/Crossing/Erase options,
projection/UCS modes and TRIMEXTENDMODE/EDGEMODE variables remain unfinished.

## Fillet and chamfer

Choose **Fillet** or **Chamfer** in Modify, or enter `FILLET` (`F`) or `CHAMFER`
(`CHA`). Pick two straight lines on the sides of the corner to keep. Exact picks
use `entity-number x,y[,z]`. Hover the second line to preview the result.

**Radius** sets the fillet radius; **Distance** sets the chamfer's first and second
distances in selection order. Enter retains the corresponding drawing value.
**Trim** offers Trim/NoTrim. **Multiple** keeps the command active after each
corner. **Undo** reverses drawing edits in the current command, including a
radius or completed distance-pair change. Escape/Exit ends the command. Holding
Shift for the second canvas pick temporarily uses zero dimensions; stored
defaults stay unchanged. Zero dimensions with NoTrim leave the drawing unchanged.

`FILLETRAD`, `CHAMFERA` and `CHAMFERB` query drawing defaults; append a nonnegative
number to set them. Defaults save in the R12 DXF header and support undo. New
drawings start at zero. Trim/NoTrim is a workspace preference, initially Trim.
These initial values are OpenCad policies. Direct forms use dimensions for that
operation without changing the drawing defaults:

```text
FILLET 5 1 20,0 2 0,20 TRIM
CHAMFER 3 6 1 20,0 2 0,20 NOTRIM
```

Supported pairs are nonparallel, equal-Z world-XY LINEs. Both retained sides must
have positive length beyond their tangent/chamfer points, including in NoTrim
mode. Sources retain their identity/style; the connector uses BYLAYER defaults
on their common layer, or the current layer when they differ. Both source edits
and the connector undo together. Near-parallel lines (normalized determinant
at most `1e-12`), ambiguous picks and precision collapse are rejected. These are
OpenCad numerical policies. Parallel-line fillets, curved/polyline sources,
chamfer Angle/Method, and 3D corners remain unfinished. The canvas **F** key fits
the drawing; `F` in the command field starts FILLET.

## Break and break at point

Choose **Break** or **Break at Point** from the Modify panel's Break drop-down,
or enter `BREAK` or `BREAKATPOINT`.

- **BREAK:** click a source curve; that raw pick becomes the first point. Pick
  the second point anywhere to remove the portion between their projections.
  **First** (`F`, including the canvas key) replaces the initial point. Enter
  `@` or `@0,0` as the second point to split an open curve without a gap.
- **BREAKATPOINT:** select an open line or arc, then specify a separate break
  point. The result is two curves meeting at the projected point.
- The command field accepts `entity-number [pick-x,y[,z]]` to select a BREAK
  source. Omitting the pick prompts for the first point. BREAKATPOINT accepts
  an entity number, then `x,y[,z]`. Subsequent canvas points use raw coordinates
  and project onto the selected curve; they do not need to hit another entity.
- Live previews show retained fragments. Enter, Exit or Escape cancels pending
  input. Each completed break is one undoable drawing edit.

Direct forms:

```text
BREAK 1 3,8 7,-8
BREAK 1 4,0 @
BREAKATPOINT 1 4,8
```

Supported sources are world-XY LINEs, ARCs and CIRCLEs; BREAKATPOINT excludes
circles. Line picks project orthogonally and clamp to endpoints. Arc picks
outside the angular span use the nearest endpoint. Circle breaks remove the
counterclockwise first-to-second portion, retaining one ARC. Source elevation
is preserved. Same-type first fragments retain their source record; additional
fragments and converted circles copy the source layer/style into new records.
Locked or hidden sources cannot be edited.

OpenCad rejects endpoint splits without a gap, equal circle directions,
ambiguous arc endpoint ties, degenerate curves and precision collapse. A gap
covering both endpoints removes an open source entirely. Fractional comparisons
use `1e-10`; these are OpenCad policies. Polyline breaks, 3D/UCS projection and
complete native tolerance/identity parity remain unfinished. Canvas **F** fits
the drawing outside an active BREAK command.

## Lengthen

Choose **Lengthen** in Modify, or enter `LENGTHEN` (`LEN`). Click a curve to
display its length and, for an arc, its included angle. Choose an edit mode:

- **DElta:** specify a signed length change. Positive extends; negative shortens.
  **Angle** changes an arc's included angle by a signed number of degrees.
- **Percent:** specify the final length as a percentage of the current length.
  `50` halves the length; `200` doubles it.
- **Total:** specify the final length. **Angle** specifies an arc's final included
  angle in degrees.
- **DYnamic:** select near an endpoint, then click its new position or drag from
  the source curve. A line projects the destination along its original direction;
  an arc projects it around the original circle. The opposite endpoint stays fixed.

In a scalar mode, click near the endpoint to change. The command field also
accepts `entity-number pick-x,y[,z]`. Dynamic accepts that selection, then a
destination `x,y[,z]`. Live previews show the proposed curve before completion.
The mode remains active for repeated edits. **Undo** (`U`, including the canvas
key) undoes the last completed length edit in this command. Enter, Exit or Escape
finishes the command; global undo/redo remains available afterward. Canvas keys
**D**, **P**, **T** and **A** choose Delta, Percent, Total and Angle.

Direct forms:

```text
LENGTHEN DELTA -5 1 30,0
LEN PERCENT 150 1 30,0
LENGTHEN TOTAL 20 1 30,0
LENGTHEN DELTA ANGLE 15 2 10,0
LENGTHEN TOTAL ANGLE 90 2 10,0
LENGTHEN DYNAMIC 1 30,0 45,12
```

Supported edits are world-XY LINEs and ARCs; closed CIRCLEs stay unchanged.
Source elevation, layer and style are retained, along with same-type DXF source
records. Each completed edit is one undo step. Locked or hidden curves cannot
be edited. OpenCad rejects ambiguous endpoint-distance ties, nonpositive final
lengths, zero/full-turn arc sweeps, degenerate curves and changes below coordinate
precision. Relative endpoint comparisons use `1e-10`; these are OpenCad policies.
Polyline/ellipse/spline lengthening, 3D/UCS behavior and complete native tolerance
parity remain unfinished.

## Explode

Choose **Explode** in Modify, or enter `EXPLODE` (`X`). A preselected polyline
explodes immediately. Otherwise, click objects or drag a selection window, then
press Enter. Picks add to the set; Shift removes. **All** selects visible
objects. The command field accepts one-based entity numbers separated by spaces.
Escape or **Exit** cancels a pending selection.

```text
EXPLODE 2 3
X
```

OpenCad decomposes open or closed 2D polylines into their LINE and ARC segments,
including a closed polyline's final edge. Bulged segments retain their circular
shape. Source elevation, layer and style are copied to each component; the
components receive new DXF records. Unrelated records remain unchanged. One
undo restores the entire batch. A locked or invalid polyline rejects the batch
before any geometry changes; simple LINE, ARC and CIRCLE sources are skipped.

The current DXF reader supports legacy R12 2D POLYLINE sequences with zero
widths. LWPOLYLINE, nonzero widths, blocks and other compound-object explosion
remain unfinished. Segment order, degenerate edges, property copying and batch
history are explicit OpenCad policies; exact native parity is not claimed.

## Join

Choose **Join** in Modify, or enter `JOIN` (`J`). Select one source and press
Enter, then select objects to join and press Enter again. Picks and windows add;
Shift removes candidates while retaining a confirmed source. **All** selects
visible objects. Escape or **Exit** cancels. A single preselected object starts
source mode; multiple preselected objects join immediately.

The first scope joins one compatible group: collinear LINEs, or ARCs sharing a
center, radius and elevation. Gaps and overlaps are allowed. Lines span the
selected endpoints in the source direction. Arcs grow counterclockwise from
the source start. Incompatible objects remain untouched. Initial multiple
selection chooses the lowest-index supported source; direct forms use their
first argument:

```text
JOIN 3 1 5
J 6 7
JOIN 6
JOIN 6 CLOSE
```

For a confirmed ARC source, **cLose** (`L`, including the canvas key) converts
only that arc to a CIRCLE. A join needing a full-turn arc is rejected; implicit
circle completion remains unresolved because the public references differ.

The source's layer, style and same-type DXF record survive a join. Close creates
a new record with the source properties. Each completed operation is one undo
step. Compatible locked or invalid sources reject the operation before mutation.
XY elevation must match exactly. Line comparisons use `1e-10` times the larger
segment length (at least 1), plus a floating-point rounding allowance; center
and radius comparisons use `1e-10` times the larger radius (at least 1).
Small accepted residuals project onto the source geometry. These are OpenCad
policies. Polyline/mixed-type joins, multiple compatible groups, 3D/UCS and
complete native tolerance parity remain unfinished.

## Polyline editing

Use Modify > Object > Polyline, the optional **Polyline Edit** toolset button,
or `PEDIT` (`PE`). Pick one XY polyline, line or arc, or enter its object number.
One preselected object is accepted. Lines and arcs ask for **Yes/No** conversion;
Enter defaults to Yes and No clears the selection. `PEDITACCEPT 1` skips this
prompt; `PEDITACCEPT 0` restores it. This preference persists between app runs.

**Close**, **Open** and **Reverse** edit the selected polyline. **Undo** walks
back only the operations made during the current PEDIT session, including
conversion. A repeated Open/Close that changes nothing adds no history. Enter
or **Exit** ends the session; Escape ends it while retaining completed edits.
Canvas keys C, O, R, U, Y, N and J activate the corresponding options. Save keeps
the session active. Starting another editing tool ends the session.

**Join** starts candidate selection for an open polyline. Pick objects, drag a
selection window, enter object numbers, or use **All** (canvas key A). Selection
is additive; Shift removes candidates while retaining the source. Enter joins
touching LINE, ARC and open POLYLINE objects at either end, then returns to the
PEDIT options. `Join 2 5` applies those candidates immediately, as does
`PEDIT 3 Join 2 5`. Close/Open/Reverse/Undo wait until candidate selection ends.
Save retains the pending selection; Exit or Escape discards it. Completed joins
remain editable and each join can be undone as one PEDIT operation.

**Edit vertex** (`Edit` or E) marks the first vertex with an X and exposes its
number and coordinates in the canvas accessibility description. **Next** (N)
and **Previous** (P) traverse vertices without wrapping, including closed
polylines. **Move** (M) starts point entry with a live preview. Enter `x,y[,z]`,
pick a canvas point with the existing snapping controls, or use `@dx,dy[,dz]`
relative to the marked vertex. Omitted absolute Z uses the polyline elevation;
omitted relative Z is zero. `Move x,y[,z]` also applies an absolute point directly.

In Move or Insert, Enter or **Exit** cancels the pending point and returns to Edit vertex.
In Edit vertex, Enter or **Exit** returns to the main PEDIT options, where Undo
can restore completed moves. Escape ends PEDIT, retaining completed operations.
Save retains the pending stage. These Enter defaults are OpenCad's choices.
Move preserves the closed flag and all stored bulges; neighboring arc centers
and radii follow the new endpoints and bulges. Source identity, layer and style
are retained. Off-plane points, protected layers, newly collapsed curved
segments and unrepresentable geometry reject before mutation; an unchanged
point adds no history. Coincident endpoints on straight segments remain allowed.

**Insert** (I) accepts the same point forms and snapping as Move. It adds a
vertex after the marked one and marks the new vertex. At the last vertex of an
open polyline it appends; at the last vertex of a closed polyline it inserts
before the closing edge. Each insertion is one PEDIT Undo step, retaining the
source object, layer, style and closed flag.

Insertion geometry is an explicit OpenCad policy: the old marked vertex's
outgoing bulge stays on its edge to the new vertex; the new vertex's edge to
the following vertex is straight. The affected arc's center and radius follow
its new endpoints and retained bulge. Other bulges shift with their vertices.
Appending to an open polyline creates a straight edge and transfers the dormant
final bulge to the new final vertex. A point equal to either adjacent vertex,
an off-plane point or an unrepresentable result rejects before mutation.
Native bulge handling and the post-insertion marker have not been verified.

**Straighten** (S) saves the marked vertex as the start of a range. Use Next or
Previous to choose the other endpoint, then **Go** (G) to remove interior
vertices and connect the retained endpoints with a straight segment. The saved
start has a circle and label; the canvas accessibility description exposes its
vertex number. A preview shows the proposed geometry before Go.

With the same marked vertex, Go straightens its outgoing arc. OpenCad treats an
already straight edge or the last vertex of an open polyline as a no-op; the
last vertex of a closed polyline straightens its closing edge. A range uses
stored vertex order between the lower and higher indices, including backward
navigation, and preserves the closing edge. Go clears the saved range and
marks its lower endpoint. Other vertices, bulges, closed state and source
properties are retained. Changed geometry is one Undo step; a no-op adds none.

Enter or Exit cancels Straighten and retains the current marker; Save retains
the pending range. Escape ends PEDIT while retaining completed edits. Range
direction, closing-edge handling, marker placement and these defaults are
OpenCad policies; exact native parity for those cases remains unverified.

```text
PE 2
Yes
Close
Reverse
Undo
Exit
PEDIT 3 Open
Join 2 5
Edit vertex
Next
Move
@5,10
Insert 15,10
Straighten
Next
Go
Exit
Undo
```

Conversion creates a zero-width legacy R12 POLYLINE with source layer/style and
a new record, removing the source. Lines retain endpoint order and exact XY
elevation; arcs become one counterclockwise bulged segment. Closing activates
the final vertex's stored outgoing bulge; opening retains that dormant value.
Same-type edits preserve the source DXF record. Protected layers, nonplanar
lines, degenerate arcs and unrepresentable results reject before mutation.
These geometry, identity and history choices are explicit OpenCad policies.

Join retains the source record, layer and style and removes consumed records.
Candidates must have exactly the source elevation. Closed, off-plane and
unsupported objects are skipped. Endpoint matching permits a numerical
allowance of `1e-10 + 8 * f64::EPSILON * max(abs(endpoint XY coordinates))`
drawing units, retaining the source junction coordinate. This allowance is
OpenCad's policy. There is no gap bridge, trimming or extension. Candidate
reversal preserves signed curved segments. Document order resolves branches;
repeated scans find chains regardless of selection order. A path returning to
its start remains open until Close is chosen. Invalid geometry in an otherwise
eligible candidate, or a protected source/consumed layer, rejects the whole
join before mutation. Hidden candidates are ignored by All and window selection.

Multiple selection, fuzz-distance/Jointype joining, width, vertex Break,
Tangent and Regen within PEDIT, curve fitting,
DELOBJ retention choices, PLINETYPE/PLINECONVERTMODE conversion settings and
general UCS editing remain unfinished. Existing property controls still edit
polyline vertices and bulges independently of PEDIT.

## Reordering toolset panels

Click **Reorder panels** at the bottom left of the Drafting toolset. Drag a panel
name within the list to move it; the drawing toolset updates immediately. Press
Escape or click outside the list to close it. Assistive technology can use the
**Move panel up** and **Move panel down** actions on each eligible row.

Panel order is saved after each move and applies in both full and slim views.
Both system and custom Drafting panels can be reordered. Editing the Modeling
toolset remains unfinished.

## Custom toolset panels

Click **Add panel** at the bottom left, then the new panel's gear to customize
it. Rename the panel, choose one of six layouts, and search the command browser.
Drag commands into the outline; double-click or use **Add command to panel**
with assistive technology to insert at the selection. **Add Drop-down** creates
a named group of commands. Drag commands into or out of it, or select an item
and click **Delete** to remove it. Unimplemented commands remain unavailable.

Panel edits save immediately. Escape closes the editor and cancels an active
drag. The header's delete control asks for confirmation before removing a user
panel. Custom drop-down buttons expose an **Open command menu** accessibility
action.

## Workspace preferences

OpenCad remembers palette visibility, full/slim toolset width, panel order,
collapsed toolset sections, command visibility, status-item visibility, recent
files and the recent-file list/grid presentation. Preferences are saved on
normal exit and every 30 seconds; panel edits and moves also save immediately. Recent
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
