# Third party notices

OpenCad's original code, documentation and generated icon artwork are licensed
under MIT. Rust dependencies retain their own licenses; exact versions and
transitive dependencies are recorded in `Cargo.lock`. Their notices are included
in each binary release as `DEPENDENCY_LICENSES.txt`.

The desktop application uses eframe/egui, winit, AccessKit, image, rfd and their
dependencies. eframe includes its default fonts under the licenses provided by
that dependency. OpenCad may use fonts provided by the operating system at
runtime; those system fonts are not bundled in this repository or its archives.

The synthetic DXF exchange fixtures exercise documented CAD geometry and format
behavior. Some fixtures were exported through AutoCAD during interoperability
checks. They contain test geometry and exchange metadata, not application code.

AutoCAD and Autodesk are trademarks of Autodesk, Inc. OpenCad is an independent
project and is not affiliated with, sponsored by, or endorsed by Autodesk.
