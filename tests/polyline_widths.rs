use opencad::{
    DxfDocument, Editor, Entity, GeometryError, Point3, Polyline2, PolylineWidth, Transform,
};

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 3.0).unwrap()
}
fn width(start: f64, end: f64) -> PolylineWidth {
    PolylineWidth::new(start, end).unwrap()
}
fn shape(entity: &Entity) -> &Polyline2 {
    let Entity::Polyline(polyline) = entity else {
        panic!("expected polyline")
    };
    polyline
}

// Hand-authored legacy DXF: inherited defaults, explicit zero, a one-component
// override and a dormant final pair. No OpenCad writer generates this fixture.
const SOURCE: &str = "0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n5\nA0\n8\n0\n66\n1\n10\n0\n20\n0\n30\n3\n40\n2e0\n41\n4.00\n0\nVERTEX\n5\nA1\n8\n0\n10\n0\n20\n0\n0\nVERTEX\n5\nA2\n8\n0\n10\n10\n20\n0\n40\n0.0\n0\nVERTEX\n5\nA3\n8\n0\n10\n20\n20\n0\n41\n6.00\n0\nSEQEND\n5\nA4\n8\n0\n0\nENDSEC\n0\nEOF\n";

#[test]
fn width_inheritance_explicit_zero_and_source_spelling_survive_edits_and_history() {
    let document = DxfDocument::parse(SOURCE).unwrap();
    assert_eq!(document.to_dxf(), SOURCE);
    assert_eq!(
        shape(&document.entities()[0]).widths(),
        &[width(2.0, 4.0), width(0.0, 4.0), width(2.0, 6.0)]
    );
    let mut editor = Editor::new(document);
    let mut polyline = shape(&editor.entities()[0]).clone();
    polyline.set_uniform_width(0.0).unwrap();
    assert!(editor.update(0, Entity::Polyline(polyline)).unwrap());
    let saved = editor.to_dxf();
    assert!(saved.contains("40\n2e0\n41\n4.00\n")); // defaults stay source-backed
    assert_eq!(saved.matches("40\n0\n").count(), 2);
    assert!(saved.contains("40\n0.0\n")); // untouched explicit zero keeps its spelling
    assert_eq!(saved.matches("41\n0\n").count(), 3);
    assert_eq!(saved.matches("5\nA1\n").count(), 1);
    assert!(
        shape(&DxfDocument::parse(&saved).unwrap().entities()[0])
            .widths()
            .iter()
            .all(|pair| *pair == PolylineWidth::default())
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), SOURCE);
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), saved);

    let mut document = DxfDocument::parse(SOURCE).unwrap();
    let mut polyline = shape(&document.entities()[0]).clone();
    let mut vertices = polyline.vertices().to_vec();
    vertices.push(p(30.0, 0.0));
    polyline.set_vertices(vertices).unwrap(); // new vertex defaults to zero, not parent defaults
    document
        .replace_entity(0, Entity::Polyline(polyline))
        .unwrap();
    let saved = document.to_dxf();
    assert_eq!(
        DxfDocument::parse(&saved).unwrap().entities(),
        document.entities()
    );
    let mut fresh = DxfDocument::default();
    fresh.push(document.entities()[0].clone());
    assert_eq!(
        DxfDocument::parse(&fresh.to_dxf()).unwrap().entities(),
        fresh.entities()
    );
    for (old, new) in [
        ("2e0", "-1"),
        ("4.00", "NaN"),
        ("0.0", "-0.1"),
        ("6.00", "inf"),
    ] {
        assert!(DxfDocument::parse(&SOURCE.replacen(old, new, 1)).is_err());
    }
}

#[test]
fn width_validation_transforms_and_direction_changes_are_atomic() {
    let mut polyline = shape(&DxfDocument::parse(SOURCE).unwrap().entities()[0]).clone();
    let before = polyline.clone();
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            polyline.set_uniform_width(invalid),
            Err(GeometryError::InvalidPolylineWidth)
        );
        assert_eq!(
            polyline.set_width(0, 2.0, invalid),
            Err(GeometryError::InvalidPolylineWidth)
        );
        assert_eq!(polyline, before);
    }
    assert_eq!(
        polyline.set_widths(vec![]),
        Err(GeometryError::InvalidWidthCount)
    );
    assert_eq!(
        polyline.set_width(9, 1.0, 1.0),
        Err(GeometryError::InvalidVertexIndex)
    );
    assert_eq!(polyline, before);
    let entity = Entity::Polyline(polyline.clone());
    let translated = entity.translated(5.0, -2.0, 1.0).unwrap();
    assert_eq!(shape(&translated).widths(), polyline.widths());
    for transform in [
        Transform::rotate_xy(p(0.0, 0.0), 0.7).unwrap(),
        Transform::mirror_xy(p(0.0, 0.0), p(10.0, 0.0)).unwrap(),
    ] {
        assert_eq!(
            shape(&entity.transformed(transform).unwrap()).widths(),
            polyline.widths()
        );
    }
    let scaled = entity
        .transformed(Transform::scale(p(0.0, 0.0), 2.0).unwrap())
        .unwrap();
    assert_eq!(
        shape(&scaled).widths(),
        &[width(4.0, 8.0), width(0.0, 8.0), width(4.0, 12.0)]
    );
    for closed in [false, true] {
        polyline = before.clone();
        polyline.set_closed(closed);
        let original = polyline.clone();
        polyline.reverse();
        assert_eq!(
            polyline.widths(),
            &[width(4.0, 0.0), width(4.0, 2.0), width(6.0, 2.0)]
        );
        polyline.reverse();
        assert_eq!(polyline, original);
        polyline.reverse_with_widths(false);
        assert_eq!(polyline.widths(), original.widths());
    }
    let mut huge = before;
    huge.set_uniform_width(f64::MAX).unwrap();
    assert!(
        Entity::Polyline(huge)
            .transformed(Transform::scale(p(0.0, 0.0), 2.0).unwrap())
            .is_err()
    );
}

#[test]
fn break_join_and_vertex_resize_preserve_outgoing_width_ownership() {
    let mut polyline = shape(&DxfDocument::parse(SOURCE).unwrap().entities()[0]).clone();
    let parts = polyline.break_vertices(1, 1).unwrap();
    assert_eq!(parts[0].widths(), &polyline.widths()[..2]);
    assert_eq!(parts[1].widths(), &polyline.widths()[1..]);
    let joined = parts[0]
        .join_endpoints_xy(&Entity::Polyline(parts[1].clone()))
        .unwrap()
        .unwrap();
    assert_eq!(joined, polyline);
    let mut reversed = parts[1].clone();
    reversed.reverse();
    assert_eq!(
        parts[0]
            .join_endpoints_xy(&Entity::Polyline(reversed))
            .unwrap()
            .unwrap(),
        polyline
    );
    polyline.set_closed(true);
    let closed = polyline.break_vertices(1, 1).unwrap().remove(0);
    assert_eq!(
        closed.widths(),
        &[
            width(0.0, 4.0),
            width(2.0, 6.0),
            width(2.0, 4.0),
            width(0.0, 4.0)
        ]
    );
    let widths = polyline.widths().to_vec();
    let mut vertices = polyline.vertices().to_vec();
    vertices.push(p(30.0, 0.0));
    polyline.set_vertices(vertices).unwrap();
    assert_eq!(&polyline.widths()[..3], &widths);
    assert_eq!(polyline.widths()[3], PolylineWidth::default());
    polyline
        .set_vertices(polyline.vertices()[..2].to_vec())
        .unwrap();
    assert_eq!(polyline.widths(), &widths[..2]);
    // EXPLODE intentionally returns only centerlines.
    assert_eq!(polyline.explode().unwrap().len(), 2);
}

#[test]
fn tapered_lines_and_signed_arc_strips_share_finite_svg_and_display_geometry() {
    use opencad::{Linetype, render_document_svg, render_entities_svg};
    let mut line = Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap();
    line.set_width(0, 2.0, 6.0).unwrap();
    line.set_width(1, 100.0, 100.0).unwrap(); // dormant value must not enlarge the open strip
    assert_eq!(
        line.width_quads_xy().unwrap(),
        vec![[p(0.0, 1.0), p(10.0, 3.0), p(10.0, -3.0), p(0.0, -1.0)]]
    );
    let svg = render_entities_svg(&[Entity::Polyline(line.clone())]).unwrap();
    assert!(svg.contains("viewBox=\"0 0 30 26\""));
    assert!(svg.contains("points=\"10,12 20,10 20,16 10,14\" fill=\"black\" stroke=\"none\""));
    line.set_closed(true);
    assert_eq!(line.width_quads_xy().unwrap().len(), 2);
    for bulge in [1.0, -1.0, 2.0, -2.0] {
        let mut arc =
            Polyline2::with_bulges(vec![p(0.0, 0.0), p(10.0, 0.0)], vec![bulge, 0.0], false)
                .unwrap();
        arc.set_width(0, 2.0, 6.0).unwrap();
        let quads = arc.width_quads_xy().unwrap();
        let opencad::PolylineSegment::Arc { arc: circle, .. } =
            arc.segments().next().unwrap().unwrap()
        else {
            panic!("bulged segment")
        };
        let distance = |point: Point3| {
            (point.x() - circle.center().x()).hypot(point.y() - circle.center().y())
        };
        assert!((distance(quads[0][0]) - circle.radius() - 1.0).abs() < 1e-10);
        assert!((distance(quads.last().unwrap()[1]) - circle.radius() - 3.0).abs() < 1e-10);
        let rendered = render_entities_svg(&[Entity::Polyline(arc.clone())]).unwrap();
        assert_eq!(rendered.matches("<polygon ").count(), quads.len());
        assert!(!rendered.contains("NaN") && !rendered.contains("inf"));
        arc.set_uniform_width(100.0).unwrap();
        for quad in arc.width_quads_xy().unwrap() {
            assert!((distance(quad[2])).abs() < 1e-10);
            assert!((distance(quad[3])).abs() < 1e-10);
        }
    }
    let mut document = DxfDocument::default();
    line.set_closed(false);
    line.set_width(0, 2.0, 6.0).unwrap();
    document.push(Entity::Polyline(line));
    document
        .add_linetype(Linetype::new("DASHED", vec![2.0, -1.0]).unwrap())
        .unwrap();
    document.set_entity_linetype(0, "DASHED").unwrap();
    document.set_entity_color(0, 1).unwrap();
    let dashed = render_document_svg(&document).unwrap();
    assert!(dashed.matches("<polygon ").count() > 1);
    assert!(dashed.contains("fill=\"#ff0000\" stroke=\"none\""));
    let mut coincident = Polyline2::new(vec![p(0.0, 0.0); 2], false).unwrap();
    coincident.set_uniform_width(10.0).unwrap();
    assert!(coincident.width_quads_xy().unwrap().is_empty());
}

#[test]
fn pline_default_width_is_drawing_scoped_source_preserved_and_undoable() {
    let minimal = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
    let source = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$PLINEWID\n40\n2.500000e0\n999\nkeep-width-comment\n9\n$PLINEGEN\n70\n1\n0\nENDSEC\n{minimal}"
    );
    let mut editor = Editor::new(DxfDocument::parse(&source).unwrap());
    assert_eq!(editor.pline_width(), 2.5);
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.set_pline_width(2.5).unwrap());
    assert!(!editor.can_undo());
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(editor.set_pline_width(invalid).is_err());
        assert_eq!(editor.to_dxf(), source);
        assert!(!editor.can_undo());
    }
    assert!(editor.set_pline_width(4.0).unwrap());
    let updated = source.replace("2.500000e0", "4");
    assert_eq!(editor.to_dxf(), updated);
    assert_eq!(DxfDocument::parse(&updated).unwrap().pline_width(), 4.0);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), updated);

    let mut document = DxfDocument::parse(minimal).unwrap();
    assert_eq!(document.pline_width(), 0.0);
    assert!(!document.set_pline_width(0.0).unwrap());
    assert_eq!(document.to_dxf(), minimal);
    document.set_linetype_scale(2.0).unwrap();
    document
        .set_corner_value(opencad::CornerParameter::FilletRadius, 3.0)
        .unwrap();
    document.set_pline_width(4.0).unwrap();
    let saved = document.to_dxf();
    assert_eq!(saved.matches("2\nHEADER\n").count(), 1);
    let parsed = DxfDocument::parse(&saved).unwrap();
    assert_eq!(parsed.pline_width(), 4.0);
    assert_eq!(parsed.linetype_scale(), 2.0);
    assert_eq!(
        parsed.corner_value(opencad::CornerParameter::FilletRadius),
        3.0
    );
    for fields in [
        "40\n-1\n",
        "40\nNaN\n",
        "40\ninf\n",
        "70\n1\n",
        "40\n1\n40\n2\n",
        "40\n1\n9\n$PLINEWID\n40\n2\n",
    ] {
        let invalid = format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$PLINEWID\n{fields}0\nENDSEC\n{minimal}"
        );
        assert!(DxfDocument::parse(&invalid).is_err(), "accepted {fields}");
    }
}
