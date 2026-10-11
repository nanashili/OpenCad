use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, GeometryError, Line3, Point3, Polyline2,
};

fn point(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z).unwrap()
}
fn line(a: (f64, f64), b: (f64, f64), z: f64) -> Entity {
    Entity::Line(Line3::new(point(a.0, a.1, z), point(b.0, b.1, z)))
}
fn arc(start: f64, end: f64) -> Entity {
    Entity::Arc(
        Arc3::new(
            point(3.0, -4.0, 9.0),
            10.0,
            start.to_radians(),
            end.to_radians(),
        )
        .unwrap(),
    )
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

#[test]
fn join_lines_fills_gaps_merges_overlaps_and_retains_source_direction_and_elevation() {
    let source = line((10.0, 5.0), (20.0, 5.0), 7.0);
    let others = [
        line((-5.0, 5.0), (0.0, 5.0), 7.0),
        line((40.0, 5.0), (30.0, 5.0), 7.0),
        line((12.0, 5.0), (18.0, 5.0), 7.0),
    ];
    assert_eq!(
        source.join_xy(&others).unwrap(),
        line((-5.0, 5.0), (40.0, 5.0), 7.0)
    );
    assert_eq!(
        line((20.0, 5.0), (10.0, 5.0), 7.0)
            .join_xy(&others)
            .unwrap(),
        line((40.0, 5.0), (-5.0, 5.0), 7.0)
    );
    assert_eq!(
        source
            .join_xy(&[source.clone(), others[2].clone()])
            .unwrap(),
        source
    );
    assert_eq!(source.join_xy(&[]).unwrap(), source);
    let Entity::Line(diagonal) = line((0.0, 0.0), (3.0, 4.0), -2.0)
        .join_xy(&[line((9.0, 12.0), (6.0, 8.0), -2.0)])
        .unwrap()
    else {
        panic!("line")
    };
    near(diagonal.end().x(), 9.0);
    near(diagonal.end().y(), 12.0);
    assert_eq!(diagonal.start(), point(0.0, 0.0, -2.0));
    assert_eq!(diagonal.end().z(), -2.0);
    let far = line((0.0, 0.0), (1.0, 1.0), 0.0)
        .join_xy(&[line((1e9, 1e9), (1e9 + 1.0, 1e9 + 1.0), 0.0)])
        .unwrap();
    let Entity::Line(far) = far else {
        panic!("line")
    };
    assert!((far.end().x() - (1e9 + 1.0)).abs() < 1e-6);
    assert!((far.end().x() - far.end().y()).abs() < 1e-6);
}

#[test]
fn join_arcs_keeps_the_source_start_and_circle_across_gaps_overlap_and_zero_wrap() {
    let source = arc(300.0, 330.0);
    let Entity::Arc(joined) = source
        .join_xy(&[arc(350.0, 20.0), arc(310.0, 320.0)])
        .unwrap()
    else {
        panic!("arc")
    };
    assert_eq!(joined.center(), point(3.0, -4.0, 9.0));
    assert_eq!(joined.radius(), 10.0);
    near(joined.start_angle().to_degrees(), 300.0);
    near(joined.end_angle().to_degrees(), 20.0);
    near(joined.sweep().to_degrees(), 80.0);
    let Entity::Arc(prior) = source.join_xy(&[arc(250.0, 270.0)]).unwrap() else {
        panic!("arc")
    };
    near(prior.sweep().to_degrees(), 330.0);
    assert_eq!(
        source.join_xy(std::slice::from_ref(&source)).unwrap(),
        source
    );
    let Entity::Circle(closed) = source.close_arc_xy().unwrap() else {
        panic!("circle")
    };
    assert_eq!(closed.center(), joined.center());
    assert_eq!(closed.radius(), joined.radius());
}

#[test]
fn join_rejects_incompatible_degenerate_full_turn_and_unrepresentable_geometry() {
    let source = line((0.0, 0.0), (10.0, 0.0), 7.0);
    for candidate in [
        line((20.0, 0.1), (30.0, 0.1), 7.0),
        line((20.0, 0.0), (20.0, 10.0), 7.0),
        line((20.0, 0.0), (30.0, 0.0), 8.0),
        arc(0.0, 90.0),
    ] {
        assert_eq!(
            source.join_xy(&[candidate]),
            Err(GeometryError::IncompatibleJoinEntities)
        );
    }
    assert_eq!(
        source.join_xy(&[line((20.0, 0.0), (20.0, 0.0), 7.0)]),
        Err(GeometryError::DegenerateCurve)
    );
    for unsupported in [
        Entity::Circle(Circle3::new(point(0.0, 0.0, 7.0), 2.0).unwrap()),
        Entity::Polyline(
            Polyline2::new(vec![point(0.0, 0.0, 7.0), point(10.0, 0.0, 7.0)], false).unwrap(),
        ),
        Entity::Line(Line3::new(point(0.0, 0.0, 7.0), point(10.0, 0.0, 8.0))),
    ] {
        assert_eq!(
            unsupported.join_xy(&[]),
            Err(GeometryError::UnsupportedJoinEntity)
        );
    }
    assert_eq!(
        source.close_arc_xy(),
        Err(GeometryError::UnsupportedJoinClose)
    );
    for (source, candidate) in [
        (arc(0.0, 90.0), arc(90.0, 0.0)),
        (arc(30.0, 60.0), arc(0.0, 40.0)),
    ] {
        assert_eq!(
            source.join_xy(&[candidate]),
            Err(GeometryError::JoinRequiresFullTurn)
        );
    }
    let invalid = Entity::Arc(Arc3::new(point(0.0, 0.0, 0.0), 10.0, 0.0, 0.0).unwrap());
    assert_eq!(invalid.close_arc_xy(), Err(GeometryError::DegenerateCurve));
    let collapsed = Entity::Arc(Arc3::new(point(1e20, 1e20, 0.0), 1.0, 0.0, 1.0).unwrap());
    assert_eq!(
        collapsed.join_xy(&[]),
        Err(GeometryError::UnrepresentableJoin)
    );
    assert!(
        line((-1e308, 0.0), (-9e307, 0.0), 0.0)
            .join_xy(&[line((9e307, 0.0), (1e308, 0.0), 0.0)])
            .is_err()
    );
    let circle_overflow = Entity::Arc(
        Arc3::new(
            point(1e308, 0.0, 0.0),
            1e308,
            120_f64.to_radians(),
            150_f64.to_radians(),
        )
        .unwrap(),
    );
    assert!(circle_overflow.close_arc_xy().is_err());
}

#[test]
fn editor_join_retains_source_records_properties_and_selection_through_one_step_history() {
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.push(line((-5.0, 5.0), (0.0, 5.0), 7.0));
    document.push(Entity::Circle(
        Circle3::new(point(50.0, 0.0, 7.0), 2.0).unwrap(),
    ));
    document
        .push_on_layer(line((10.0, 5.0), (20.0, 5.0), 7.0), "DETAIL")
        .unwrap();
    document.set_entity_color(2, 3).unwrap();
    document.set_entity_linetype_scale(2, 2.0).unwrap();
    document.push(line((0.0, 10.0), (5.0, 10.0), 7.0));
    document.push(line((40.0, 5.0), (30.0, 5.0), 7.0));
    let text = document
        .to_dxf()
        .replacen("0\nLINE\n", "0\nLINE\n5\nA1\n999\nconsumed record\n", 1)
        .replacen(
            "0\nCIRCLE\n",
            "0\nCIRCLE\n5\nA2\n999\nunrelated record\n",
            1,
        );
    let source_marker = "0\nLINE\n8\nDETAIL\n";
    let text = text.replacen(
        source_marker,
        "0\nLINE\n5\nB1\n999\nsource record\n8\nDETAIL\n",
        1,
    );
    assert!(text.contains("source record"));
    let mut editor = Editor::new(DxfDocument::parse(&text).unwrap());
    editor.select(vec![4, 2, 0, 1, 3]);
    let original_selection = editor.selection().to_vec();
    assert_eq!(
        editor.join_entities(2, &[4, 2, 0, 1, 3, 4, 999]).unwrap(),
        2
    );
    assert_eq!(editor.entities().len(), 3);
    assert_eq!(editor.selection(), &[1]);
    assert_eq!(editor.entities()[1], line((-5.0, 5.0), (40.0, 5.0), 7.0));
    assert_eq!(editor.entity_layer(1), Some("DETAIL"));
    assert_eq!(editor.entity_style(1).unwrap().color_index(), 3);
    assert_eq!(editor.entity_style(1).unwrap().linetype_scale(), 2.0);
    let joined = editor.to_dxf();
    assert!(joined.contains("B1\n999\nsource record"));
    assert!(joined.contains("A2\n999\nunrelated record"));
    assert!(!joined.contains("consumed record"));
    let reloaded = DxfDocument::parse(&joined).unwrap();
    assert_eq!(reloaded.entities(), editor.entities());
    assert_eq!(reloaded.entity_style(1), editor.entity_style(1));
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), text);
    assert_eq!(editor.selection(), original_selection);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), joined);
    assert_eq!(editor.join_entities(1, &[0, 1, 2]).unwrap(), 0);
    assert_eq!(editor.to_dxf(), joined);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), text);
    assert!(!editor.can_undo());
}

#[test]
fn editor_join_and_arc_close_preflight_protected_and_invalid_sources_without_partial_edits() {
    let mut document = DxfDocument::default();
    document.add_layer("LOCKED").unwrap();
    document.push(line((0.0, 0.0), (10.0, 0.0), 7.0));
    document.push(line((20.0, 0.0), (30.0, 0.0), 7.0));
    document
        .push_on_layer(line((40.0, 0.0), (50.0, 0.0), 7.0), "LOCKED")
        .unwrap();
    document.set_layer_locked("LOCKED", true).unwrap();
    let mut editor = Editor::new(document);
    editor.select(vec![0, 1, 2]);
    let original = editor.to_dxf();
    assert!(editor.join_entities(0, &[1, 2]).is_err());
    assert_eq!(editor.to_dxf(), original);
    assert_eq!(editor.selection(), &[0, 1, 2]);
    assert!(!editor.can_undo());
    editor.set_layer_visible("LOCKED", false).unwrap();
    editor.mark_saved();
    assert_eq!(editor.join_entities(0, &[1, 2]).unwrap(), 1);
    assert_eq!(editor.entities().len(), 2);
    assert!(editor.undo());
    assert_eq!(editor.entities().len(), 3);

    let mut invalid = DxfDocument::default();
    invalid.push(line((0.0, 0.0), (10.0, 0.0), 0.0));
    invalid.push(line((20.0, 0.0), (30.0, 0.0), 0.0));
    invalid.push(line((40.0, 0.0), (40.0, 0.0), 0.0));
    let mut editor = Editor::new(invalid);
    let original = editor.to_dxf();
    assert!(editor.join_entities(0, &[1, 2]).is_err());
    assert_eq!(editor.to_dxf(), original);
    assert!(!editor.can_undo());

    let mut arcs = DxfDocument::default();
    arcs.add_layer("DETAIL").unwrap();
    arcs.push(line((0.0, 0.0), (10.0, 0.0), 0.0));
    arcs.push_on_layer(arc(300.0, 20.0), "DETAIL").unwrap();
    arcs.set_entity_color(1, 5).unwrap();
    let original = arcs
        .to_dxf()
        .replacen("0\nARC\n", "0\nARC\n5\nC1\n999\narc source\n", 1);
    let mut editor = Editor::new(DxfDocument::parse(&original).unwrap());
    editor.select(vec![1]);
    assert!(editor.close_arc_entity(1).unwrap());
    assert!(matches!(editor.entities()[1], Entity::Circle(_)));
    assert_eq!(editor.entity_layer(1), Some("DETAIL"));
    assert_eq!(editor.entity_style(1).unwrap().color_index(), 5);
    let closed = editor.to_dxf();
    assert!(!closed.contains("arc source"));
    assert_eq!(
        DxfDocument::parse(&closed).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), original);
    assert_eq!(editor.selection(), &[1]);
    assert!(!editor.can_undo());
    editor.set_layer_locked("DETAIL", true).unwrap();
    let protected = editor.to_dxf();
    assert!(editor.close_arc_entity(1).is_err());
    assert_eq!(editor.to_dxf(), protected);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), original);
    assert!(!editor.can_undo());
}
