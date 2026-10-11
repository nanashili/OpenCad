use opencad::{
    DxfDocument, Editor, Entity, GeometryError, Line3, Linetype, LinetypeError, Point3, Polyline2,
    SvgError, render_document_svg, render_entities_svg,
};

// This hand-sized fixture deliberately omits header 70 and mixes numeric
// spellings. The existing single-record fixtures cannot expose child-span or
// SEQEND ownership errors. Independent source-app output is checked separately.
const SOURCE: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n5\nA0\n8\n0\n66\n1\n10\n0.0\n20\n0\n30\n3e0\n0\nVERTEX\n5\nA1\n8\n0\n10\n0e0\n20\n0.0\n30\n0\n0\nVERTEX\n5\nA2\n8\n0\n10\n4.0\n20\n0\n30\n0\n0\nVERTEX\n5\nA3\n8\n0\n10\n4\n20\n3.0\n30\n0\n0\nSEQEND\n5\nA4\n8\n0\n0\nLINE\n5\nA5\n8\n0\n10\n20\n20\n0\n30\n3\n11\n30\n21\n0\n31\n3\n0\nENDSEC\n0\nEOF\n";

fn point(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z).unwrap()
}

fn polyline(entity: &Entity) -> &Polyline2 {
    let Entity::Polyline(polyline) = entity else {
        panic!("expected one polyline")
    };
    polyline
}

// AutoCAD independently converted these legacy sequences to LWPOLYLINE in
// memory and exported them back to R12. Padded flags, child records and common
// nonzero elevations are absent from earlier single-record native fixtures.
#[test]
fn independent_autocad_sequences_retain_geometry_and_source_bytes() {
    let input = include_str!("fixtures/polylines-autocad-input.dxf");
    let exported = include_str!("fixtures/polylines-autocad-r12.dxf");
    let document = DxfDocument::parse(input).unwrap();
    let reimported = DxfDocument::parse(exported).unwrap();
    assert_eq!(document.to_dxf(), input);
    assert_eq!(reimported.to_dxf(), exported);
    assert_eq!(document.entities(), reimported.entities());
    assert_eq!(document.entities().len(), 4);
    for (index, closed, elevation, count) in [
        (0, false, 3.0, 3),
        (1, true, 2.0, 3),
        (2, false, 0.0, 4),
        (3, false, 5.0, 3),
    ] {
        let shape = polyline(&document.entities()[index]);
        assert_eq!(shape.is_closed(), closed);
        assert_eq!(shape.elevation(), elevation);
        assert_eq!(shape.vertices().len(), count);
    }
    let repeated = polyline(&document.entities()[2]).vertices();
    assert_eq!(repeated[1], repeated[2]);
    assert_eq!(document.entity_layer(3), Some("STYLED"));
    assert_eq!(document.entity_style(3).unwrap().color_index(), 4);
    assert_eq!(document.entity_style(3).unwrap().linetype(), "DASHED");
    assert_eq!(document.entity_style(3).unwrap().linetype_scale(), 0.5);
    assert_eq!(reimported.entity_style(3).unwrap().linetype_scale(), 1.0);

    // A separate source-app probe used child Z values [0,3,9] and [0,0,0].
    // Both represent the header elevation, and untouched source bytes survive.
    let mixed = include_str!("fixtures/polylines-elevation-input.dxf");
    let normalized = include_str!("fixtures/polylines-elevation-r12.dxf");
    let mut document = DxfDocument::parse(mixed).unwrap();
    assert_eq!(document.to_dxf(), mixed);
    assert_eq!(
        document.entities(),
        DxfDocument::parse(normalized).unwrap().entities()
    );
    for entity in document.entities() {
        assert!(
            polyline(entity)
                .vertices()
                .iter()
                .all(|point| point.z() == 3.0)
        );
    }
    let mut shape = polyline(&document.entities()[0]).clone();
    shape.set_elevation(6.0).unwrap();
    document.replace_entity(0, Entity::Polyline(shape)).unwrap();
    let saved = document.to_dxf();
    assert_eq!(saved.matches("30\n6\n").count(), 4);
    assert_eq!(
        polyline(&DxfDocument::parse(&saved).unwrap().entities()[0]).elevation(),
        6.0
    );
}

// Ordered vertices must stay one history object. LINE history cannot reveal
// partial polyline translation, lost closure, or count/elevation edit failures.
#[test]
fn polyline_geometry_and_history_keep_whole_objects_atomic() {
    let mut shape = polyline(&DxfDocument::parse(SOURCE).unwrap().entities()[0]).clone();
    assert_eq!(shape.length().unwrap(), 7.0);
    shape.set_closed(true);
    assert_eq!(shape.length().unwrap(), 12.0);
    let before = shape.clone();
    assert_eq!(
        shape.set_vertices(vec![]),
        Err(GeometryError::NotEnoughVertices)
    );
    assert_eq!(
        shape.set_vertices(vec![point(0.0, 0.0, 3.0), point(1.0, 0.0, 4.0)]),
        Err(GeometryError::NonPlanarPolyline)
    );
    assert!(shape.set_elevation(f64::NAN).is_err());
    assert_eq!(shape, before);
    let mut repeated = shape.vertices().to_vec();
    repeated.insert(1, repeated[0]);
    repeated.push(repeated[0]);
    shape.set_vertices(repeated).unwrap();
    assert_eq!(shape.length().unwrap(), 12.0);
    shape.set_elevation(8.0).unwrap();
    assert!(shape.vertices().iter().all(|point| point.z() == 8.0));

    let mut editor = Editor::new(DxfDocument::parse(SOURCE).unwrap());
    editor.select(vec![0]);
    editor.translate_selection(10.0, -20.0, 4.0, false).unwrap();
    assert_eq!(
        polyline(&editor.entities()[0]).vertices()[2],
        point(14.0, -17.0, 7.0)
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), SOURCE);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    editor.translate_selection(-10.0, 20.0, -4.0, true).unwrap();
    assert_eq!(editor.entities().len(), 3);
    assert_eq!(editor.selection(), &[2]);
    assert_eq!(
        polyline(&editor.entities()[2]).vertices()[0],
        point(0.0, 0.0, 3.0)
    );
    assert!(editor.undo());
    assert_eq!(editor.entities().len(), 2);
    editor
        .add(vec![Entity::Line(Line3::new(
            point(f64::MAX, 0.0, 0.0),
            point(f64::MAX, 1.0, 0.0),
        ))])
        .unwrap();
    editor.select(vec![0, 2]);
    let snapshot = editor.to_dxf();
    assert!(
        editor
            .translate_selection(f64::MAX, 0.0, 0.0, false)
            .is_err()
    );
    assert_eq!(editor.to_dxf(), snapshot);
    editor.set_layer_locked("0", true).unwrap();
    let snapshot = editor.to_dxf();
    assert!(editor.delete_selection().is_err());
    assert_eq!(editor.to_dxf(), snapshot);
}

// Source edits cross several records. Checks cover preserved unrelated text,
// child handle identity, missing-header-field insertion, and tail ownership.
#[test]
fn vertex_edits_and_parent_styles_preserve_record_boundaries() {
    let mut document = DxfDocument::parse(SOURCE).unwrap();
    assert_eq!(document.to_dxf(), SOURCE);
    assert_eq!(document.entities().len(), 2);
    let mut shape = polyline(&document.entities()[0]).clone();
    let mut vertices = shape.vertices().to_vec();
    vertices[1] = point(8.0, 0.0, 3.0);
    shape.set_vertices(vertices).unwrap();
    document
        .replace_entity(0, Entity::Polyline(shape.clone()))
        .unwrap();
    assert_eq!(
        document.to_dxf(),
        SOURCE.replacen("10\n4.0\n", "10\n8\n", 1)
    );
    shape.set_closed(true);
    shape.set_elevation(7.0).unwrap();
    let mut vertices = shape.vertices().to_vec();
    vertices.push(point(-2.0, 1.0, 7.0));
    shape.set_vertices(vertices).unwrap();
    document
        .replace_entity(0, Entity::Polyline(shape.clone()))
        .unwrap();
    document.add_layer("DETAIL").unwrap();
    document.set_entity_layer(0, "DETAIL").unwrap();
    document.set_entity_color(0, 1).unwrap();
    document.set_entity_linetype(0, "CONTINUOUS").unwrap();
    document.set_entity_linetype_scale(0, 0.5).unwrap();
    let saved = document.to_dxf();
    assert_eq!(saved.matches("0\nVERTEX\n").count(), 4);
    assert_eq!(saved.matches("0\nSEQEND\n").count(), 1);
    assert_eq!(saved.matches("8\nDETAIL\n").count(), 6);
    assert_eq!(saved.matches("62\n1\n6\nCONTINUOUS\n48\n0.5\n").count(), 6);
    assert!(saved.contains("5\nA2\n8\nDETAIL\n10\n8\n"));
    assert!(saved.contains("5\nA5\n8\n0\n10\n20\n"));
    let reloaded = DxfDocument::parse(&saved).unwrap();
    assert_eq!(polyline(&reloaded.entities()[0]), &shape);
    let mut vertices = shape.vertices().to_vec();
    vertices.truncate(2);
    shape.set_vertices(vertices).unwrap();
    document.replace_entity(0, Entity::Polyline(shape)).unwrap();
    let saved = document.to_dxf();
    assert_eq!(saved.matches("0\nVERTEX\n").count(), 2);
    assert!(!saved.contains("5\nA3\n"));
    assert!(saved.contains("5\nA4\n"));
    assert_eq!(DxfDocument::parse(&saved).unwrap().entities().len(), 2);
    document.remove(0).unwrap();
    let saved = document.to_dxf();
    assert!(!saved.contains("VERTEX"));
    assert!(!saved.contains("SEQEND"));
    assert_eq!(DxfDocument::parse(&saved).unwrap().entities().len(), 1);

    // A missing child style is inserted at the deleted next child's start.
    let mut document = DxfDocument::parse(SOURCE).unwrap();
    let shape = Polyline2::new(
        polyline(&document.entities()[0]).vertices()[..2].to_vec(),
        true,
    )
    .unwrap();
    document.replace_entity(0, Entity::Polyline(shape)).unwrap();
    document.set_entity_color(0, 2).unwrap();
    document.remove(1).unwrap();
    let saved = document.to_dxf();
    assert_eq!(saved.matches("62\n2\n").count(), 4);
    assert_eq!(DxfDocument::parse(&saved).unwrap().entities().len(), 1);
}

// A malformed child sequence can otherwise swallow the following top-level
// object. Single-record parser tests do not cover this grammar or child handles.
#[test]
fn malformed_or_unsupported_polyline_sequences_fail_without_projection() {
    for invalid in [
        SOURCE.replacen("0\nSEQEND\n5\nA4\n8\n0\n", "", 1),
        SOURCE.replacen("5\nA3\n", "5\na1\n", 1),
        SOURCE.replacen("5\nA4\n", "5\nA0\n", 1),
        SOURCE.replacen("5\nA5\n", "5\nA2\n", 1),
        SOURCE.replacen("30\n3e0\n", "30\n3e0\n70\n8\n", 1),
        SOURCE.replacen("30\n3e0\n", "30\n3e0\n70\n128\n", 1),
        SOURCE.replacen("30\n3e0\n", "30\n3e0\n210\n0\n220\n1\n230\n0\n", 1),
        SOURCE.replacen("10\n4.0\n", "42\nNaN\n10\n4.0\n", 1),
        SOURCE.replacen("10\n4.0\n", "40\n-2\n10\n4.0\n", 1),
        SOURCE.replacen("10\n4.0\n", "30\n2\n10\n4.0\n", 1),
        SOURCE.replacen("10\n4.0\n", "10\nNaN\n", 1),
        SOURCE.replacen("0\nPOLYLINE\n", "0\nLWPOLYLINE\n", 1),
    ] {
        assert!(DxfDocument::parse(&invalid).is_err(), "accepted {invalid}");
    }
    let without_follows = SOURCE.replacen("66\n1\n", "", 1);
    assert_eq!(
        DxfDocument::parse(&without_follows).unwrap().to_dxf(),
        without_follows
    );
    let one_vertex = SOURCE
        .replace("0\nVERTEX\n5\nA2\n8\n0\n10\n4.0\n20\n0\n30\n0\n", "")
        .replace("0\nVERTEX\n5\nA3\n8\n0\n10\n4\n20\n3.0\n30\n0\n", "");
    assert!(DxfDocument::parse(&one_vertex).is_err());
}

// Whole-object bounds and per-segment patterns differ from LINE/ARC rendering.
// The aggregate density limit must also hold across many individually tiny edges.
#[test]
fn svg_uses_closure_bounds_parent_style_and_bounded_segment_patterns() {
    let mut document = DxfDocument::parse(SOURCE).unwrap();
    document.remove(1).unwrap();
    let solid = render_document_svg(&document).unwrap();
    assert!(solid.contains("<polyline "));
    let mut shape = polyline(&document.entities()[0]).clone();
    shape.set_closed(true);
    document.replace_entity(0, Entity::Polyline(shape)).unwrap();
    assert!(
        render_document_svg(&document)
            .unwrap()
            .contains("<polygon ")
    );
    document
        .add_linetype(Linetype::new("DASHED", vec![1.0, -0.5]).unwrap())
        .unwrap();
    document.set_entity_linetype(0, "DASHED").unwrap();
    document.set_entity_color(0, 3).unwrap();
    let dashed = render_document_svg(&document).unwrap();
    assert_eq!(dashed.matches("<line ").count(), 8);
    assert!(dashed.contains("#00ff00"));
    document.set_layer_visible("0", false).unwrap();
    assert!(matches!(
        render_document_svg(&document),
        Err(SvgError::EmptyDrawing)
    ));
    document.set_layer_visible("0", true).unwrap();
    let vertices = (0..4098).map(|x| point(f64::from(x), 0.0, 3.0)).collect();
    document
        .replace_entity(
            0,
            Entity::Polyline(Polyline2::new(vertices, false).unwrap()),
        )
        .unwrap();
    assert!(matches!(
        render_document_svg(&document),
        Err(SvgError::Linetype(LinetypeError::TooDense))
    ));
    let huge = Polyline2::new(
        vec![point(-f64::MAX, 0.0, 0.0), point(f64::MAX, 0.0, 0.0)],
        false,
    )
    .unwrap();
    assert!(huge.length().is_err());
    assert!(matches!(
        render_entities_svg(&[Entity::Polyline(huge)]),
        Err(SvgError::BoundsTooLarge)
    ));
}
