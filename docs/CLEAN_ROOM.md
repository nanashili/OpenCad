# Clean-room implementation process

OpenCad is an independent clean-room implementation of a growing subset of
AutoCAD workflows, written in Rust. The process separates investigation of the
reference application from implementation. It is a development method, not a
claim of complete compatibility or a legal certification.

## Information flow

1. A reference investigation lane observes UI behavior, controlled exchanges,
   functional UI configuration, and native behavior. Some private investigations
   included native binary analysis. Analysts working with native implementation
   bodies are kept separate from the Rust implementation lane.
2. The investigation produces behavior contracts: inputs, outputs, geometry
   rules, failure cases and externally visible UI organization. Native source
   text, disassembly and decompiler bodies do not enter implementation work.
3. The implementation lane writes original Rust code and tests against those
   contracts and synthetic DXF fixtures. Reviewers of the Rust implementation
   do not read native implementation bodies.
4. Independent readback of controlled DXF exchanges checks supported workflows.
   UI tests drive OpenCad's actual widgets through accessibility and keyboard
   events. Passing these checks establishes only the behavior they exercise.

## Public repository boundary

This repository contains original Rust implementation, original generated icon
artwork, functional UI metadata, measured indexed-color facts and synthetic
exchange fixtures. The public
catalog records names and organization reviewed from UI configuration; it omits
native command macros and implementation bodies.

Early local UI exploration used recovered reference artwork. Those resources
were replaced with original OpenCad icons before publication. The public build
does not ship Autodesk binaries, recovered icons, private analysis archives or
fonts extracted from AutoCAD. It does not require an AutoCAD installation.

## Contributions

Describe observable behavior using small examples and synthetic data. Keep
native investigation material out of code, issues and pull requests. Do not
contribute decompiled implementations or proprietary artwork. When documenting
a compatibility issue, distinguish expected reference behavior from what has
actually been observed and tested.

AutoCAD and Autodesk are trademarks of Autodesk, Inc. This project is independent
and is not affiliated with or endorsed by Autodesk.
