use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, GeometryError, Line3, Point3, Polyline2,
    PolylineSegment,
};

fn p(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z).unwrap()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn polyline_conversion_retains_line_orientation_elevation_and_existing_polyline_data() {
    for (a, b) in [
        (p(9.0, -2.0, 7.0), p(-3.0, 4.0, 7.0)),
        (p(1.0, 2.0, 3.0), p(1.0, 2.0, 3.0)),
    ] {
        let polyline = Entity::Line(Line3::new(a, b)).to_polyline_xy().unwrap();
        assert_eq!(polyline.vertices(), &[a, b]);
        assert_eq!(polyline.bulges(), &[0.0, 0.0]);
        assert!(!polyline.is_closed());
    }
    let original = Polyline2::with_bulges(
        vec![p(-10.0, 0.0, 5.0), p(10.0, 0.0, 5.0), p(0.0, 20.0, 5.0)],
        vec![-1.0, 0.5, 0.25],
        true,
    )
    .unwrap();
    assert_eq!(
        Entity::Polyline(original.clone()).to_polyline_xy().unwrap(),
        original
    );
}

#[test]
fn arc_conversion_and_polyline_reversal_preserve_minor_major_and_wrapped_loci() {
    for (start, end) in [
        (0.0_f64, 90.0_f64),
        (0.0, 180.0),
        (0.0, 270.0),
        (300.0, 20.0),
    ] {
        let arc = Arc3::new(
            p(3.0, -4.0, 9.0),
            10.0,
            start.to_radians(),
            end.to_radians(),
        )
        .unwrap();
        let mut polyline = Entity::Arc(arc).to_polyline_xy().unwrap();
        assert_eq!(
            polyline.vertices(),
            &[arc.start_point().unwrap(), arc.end_point().unwrap()]
        );
        assert_eq!(polyline.elevation(), 9.0);
        assert_eq!(polyline.bulges()[1], 0.0);
        near(polyline.bulges()[0], (arc.sweep() / 4.0).tan());
        near(polyline.length().unwrap(), arc.length().unwrap());
        let segment = polyline.segments().next().unwrap().unwrap();
        let PolylineSegment::Arc {
            arc: result,
            clockwise: false,
        } = segment
        else {
            panic!("CCW arc")
        };
        near(result.center().x(), 3.0);
        near(result.center().y(), -4.0);
        near(result.radius(), 10.0);
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let actual = segment.point_at_fraction(fraction).unwrap();
            let expected = arc.point_at_fraction(fraction).unwrap();
            near(actual.x(), expected.x());
            near(actual.y(), expected.y());
        }
        polyline.reverse();
        let reversed = polyline.segments().next().unwrap().unwrap();
        assert!(matches!(
            reversed,
            PolylineSegment::Arc {
                clockwise: true,
                ..
            }
        ));
        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let actual = reversed.point_at_fraction(fraction).unwrap();
            let expected = arc.point_at_fraction(1.0 - fraction).unwrap();
            near(actual.x(), expected.x());
            near(actual.y(), expected.y());
        }
        polyline.set_closed(true);
        near(
            polyline.length().unwrap(),
            arc.length().unwrap()
                + Line3::new(arc.start_point().unwrap(), arc.end_point().unwrap())
                    .length()
                    .unwrap(),
        );
        polyline.set_closed(false);
        near(polyline.length().unwrap(), arc.length().unwrap());
        polyline.reverse();
        assert_eq!(polyline, Entity::Arc(arc).to_polyline_xy().unwrap());
    }
}

#[test]
fn polyline_conversion_rejects_unsupported_nonplanar_and_unrepresentable_inputs() {
    assert_eq!(
        Entity::Circle(Circle3::new(p(0.0, 0.0, 0.0), 1.0).unwrap()).to_polyline_xy(),
        Err(GeometryError::UnsupportedPolylineConversion)
    );
    assert_eq!(
        Entity::Line(Line3::new(p(0.0, 0.0, 0.0), p(1.0, 2.0, 3.0))).to_polyline_xy(),
        Err(GeometryError::NonPlanarPolyline)
    );
    for arc in [
        Arc3::new(p(0.0, 0.0, 0.0), 0.0, 0.0, 1.0).unwrap(),
        Arc3::new(p(0.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap(),
    ] {
        assert_eq!(
            Entity::Arc(arc).to_polyline_xy(),
            Err(GeometryError::DegenerateCurve)
        );
    }
    let collapsed = Arc3::new(p(1e30, 1e30, 0.0), 1.0, 0.0, 1.0).unwrap();
    assert_eq!(
        Entity::Arc(collapsed).to_polyline_xy(),
        Err(GeometryError::UnrepresentableBulge)
    );
    assert!(
        Entity::Line(Line3::new(p(-f64::MAX, 0.0, 0.0), p(f64::MAX, 0.0, 0.0)))
            .to_polyline_xy()
            .is_err()
    );
    let huge = Polyline2::with_bulges(
        vec![p(-10.0, 0.0, 0.0), p(10.0, 0.0, 0.0)],
        vec![f64::MAX, 0.0],
        false,
    )
    .unwrap();
    assert!(Entity::Polyline(huge).to_polyline_xy().is_err());
}

#[test]
fn editor_conversion_copies_properties_replaces_only_the_source_and_rolls_back_protected_layers() {
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document
        .push_on_layer(
            Entity::Line(Line3::new(p(0.0, 0.0, 7.0), p(10.0, 5.0, 7.0))),
            "DETAIL",
        )
        .unwrap();
    document.set_entity_color(0, 3).unwrap();
    document.set_entity_linetype_scale(0, 2.0).unwrap();
    document.push(Entity::Circle(
        Circle3::new(p(20.0, 30.0, 7.0), 5.0).unwrap(),
    ));
    let original = document
        .to_dxf()
        .replacen("LINE\n", "LINE\n5\nB1\n999\nsource-private-tag\n", 1)
        .replacen("CIRCLE\n", "CIRCLE\n5\nB2\n999\nunrelated-tag\n", 1);
    let mut editor = Editor::new(DxfDocument::parse(&original).unwrap());
    let source_style = editor.entity_style(0).unwrap().clone();
    editor.select(vec![0]);
    assert!(editor.convert_to_polyline(0).unwrap());
    let converted = editor.to_dxf();
    assert_eq!(editor.selection(), &[1]);
    assert!(matches!(editor.entities()[0], Entity::Circle(_)));
    assert!(matches!(editor.entities()[1], Entity::Polyline(_)));
    assert_eq!(editor.entity_layer(1), Some("DETAIL"));
    assert_eq!(editor.entity_style(1), Some(&source_style));
    assert!(!converted.contains("source-private-tag"));
    assert!(converted.contains("unrelated-tag"));
    assert_eq!(
        DxfDocument::parse(&converted).unwrap().entities(),
        editor.entities()
    );
    assert!(!editor.convert_to_polyline(1).unwrap());
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), original);
    assert_eq!(editor.selection(), &[0]);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), converted);
    for hidden in [false, true] {
        let mut protected = document.clone();
        if hidden {
            protected.set_layer_visible("DETAIL", false).unwrap();
        } else {
            protected.set_layer_locked("DETAIL", true).unwrap();
        }
        let mut editor = Editor::new(protected);
        let before = editor.to_dxf();
        assert!(editor.convert_to_polyline(0).is_err());
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
        assert!(!editor.convert_to_polyline(99).unwrap());
    }
}

#[test]
fn vertex_break_open_ranges_split_truncate_and_retain_signed_arc_loci() {
    let vertices = vec![
        p(-40.0, -20.0, 7.0),
        p(-20.0, 10.0, 7.0),
        p(0.0, 0.0, 7.0),
        p(20.0, 10.0, 7.0),
        p(40.0, -20.0, 7.0),
    ];
    let original =
        Polyline2::with_bulges(vertices, vec![0.5, -0.25, 1.0, -0.5, 9.0], false).unwrap();
    let original_segments: Vec<_> = original.segments().map(Result::unwrap).collect();
    for (first, second, expected) in [
        (0, 2, vec![vec![2, 3, 4]]),
        (2, 4, vec![vec![0, 1, 2]]),
        (1, 3, vec![vec![0, 1], vec![3, 4]]),
        (2, 2, vec![vec![0, 1, 2], vec![2, 3, 4]]),
        (1, 2, vec![vec![0, 1], vec![2, 3, 4]]),
    ] {
        let parts = original.break_vertices(first, second).unwrap();
        assert_eq!(parts, original.break_vertices(second, first).unwrap());
        assert_eq!(parts.len(), expected.len());
        for (part, indices) in parts.iter().zip(expected) {
            assert!(!part.is_closed());
            assert_eq!(
                part.vertices(),
                indices
                    .iter()
                    .map(|&i| original.vertices()[i])
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                part.bulges(),
                indices
                    .iter()
                    .map(|&i| original.bulges()[i])
                    .collect::<Vec<_>>()
            );
            for (segment, &index) in part.segments().zip(&indices) {
                let segment = segment.unwrap();
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    assert_eq!(
                        segment.point_at_fraction(fraction).unwrap(),
                        original_segments[index]
                            .point_at_fraction(fraction)
                            .unwrap()
                    );
                }
            }
        }
    }
    for (first, second) in [(0, 0), (4, 4), (0, 4), (4, 0)] {
        assert_eq!(
            original.break_vertices(first, second),
            Err(GeometryError::InvalidPolylineBreak)
        );
    }
    assert_eq!(
        original.break_vertices(5, 2),
        Err(GeometryError::InvalidVertexIndex)
    );
}

#[test]
fn vertex_break_closed_ranges_keep_the_complement_and_open_at_a_single_marker() {
    let original = Polyline2::with_bulges(
        vec![
            p(-40.0, -20.0, 7.0),
            p(-20.0, 10.0, 7.0),
            p(0.0, 0.0, 7.0),
            p(20.0, 10.0, 7.0),
            p(40.0, -20.0, 7.0),
        ],
        vec![0.5, -0.25, 1.0, -0.5, 0.25],
        true,
    )
    .unwrap();
    let segments: Vec<_> = original.segments().map(Result::unwrap).collect();
    for (first, second, indices) in [
        (1, 3, vec![3, 4, 0, 1]),
        (3, 1, vec![3, 4, 0, 1]),
        (2, 2, vec![2, 3, 4, 0, 1, 2]),
        (0, 4, vec![4, 0]),
        (0, 0, vec![0, 1, 2, 3, 4, 0]),
        (4, 4, vec![4, 0, 1, 2, 3, 4]),
    ] {
        let parts = original.break_vertices(first, second).unwrap();
        assert_eq!(parts.len(), 1);
        let part = &parts[0];
        assert!(!part.is_closed());
        assert_eq!(
            part.vertices(),
            indices
                .iter()
                .map(|&i| original.vertices()[i])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            part.bulges(),
            indices
                .iter()
                .map(|&i| original.bulges()[i])
                .collect::<Vec<_>>()
        );
        for (segment, &index) in part.segments().zip(&indices) {
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                assert_eq!(
                    segment.unwrap().point_at_fraction(fraction).unwrap(),
                    segments[index].point_at_fraction(fraction).unwrap()
                );
            }
        }
        if first == second {
            near(part.length().unwrap(), original.length().unwrap());
        }
    }
    let two = Polyline2::with_bulges(
        vec![p(0.0, 0.0, 0.0), p(10.0, 0.0, 0.0)],
        vec![1.0, -1.0],
        true,
    )
    .unwrap();
    assert_eq!(
        two.break_vertices(0, 1).unwrap()[0].vertices(),
        &[two.vertices()[1], two.vertices()[0]]
    );
    assert_eq!(two.break_vertices(1, 1).unwrap()[0].vertices().len(), 3);
}

#[test]
fn vertex_break_editor_preserves_source_and_fragment_properties_with_atomic_history() {
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.push(Entity::Circle(
        Circle3::new(p(50.0, 0.0, 7.0), 5.0).unwrap(),
    ));
    document
        .push_on_layer(
            Entity::Polyline(
                Polyline2::with_bulges(
                    vec![
                        p(0.0, 0.0, 7.0),
                        p(10.0, 10.0, 7.0),
                        p(20.0, 0.0, 7.0),
                        p(30.0, 10.0, 7.0),
                    ],
                    vec![0.5, -0.5, 1.0, 9.0],
                    false,
                )
                .unwrap(),
            ),
            "DETAIL",
        )
        .unwrap();
    document.set_entity_color(1, 3).unwrap();
    document.set_entity_linetype_scale(1, 2.0).unwrap();
    let original = document.to_dxf().replacen(
        "POLYLINE\n",
        "POLYLINE\n5\nD4\n999\nvertex-break-source\n",
        1,
    );
    let mut editor = Editor::new(DxfDocument::parse(&original).unwrap());
    let style = editor.entity_style(1).unwrap().clone();
    editor.select(vec![1]);
    assert!(editor.break_polyline_vertices(1, 1, 2).unwrap());
    let broken = editor.to_dxf();
    assert_eq!(editor.selection(), &[1, 2]);
    assert_eq!(editor.entities()[0], document.entities()[0]);
    assert_eq!(broken.matches("vertex-break-source").count(), 1);
    assert!(broken.contains("5\nD4\n"));
    for index in [1, 2] {
        assert_eq!(editor.entity_layer(index), Some("DETAIL"));
        assert_eq!(editor.entity_style(index), Some(&style));
    }
    assert_eq!(
        DxfDocument::parse(&broken).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), original);
    assert_eq!(editor.selection(), &[1]);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), broken);
    for hidden in [false, true] {
        let mut protected = document.clone();
        if hidden {
            protected.set_layer_visible("DETAIL", false).unwrap();
        } else {
            protected.set_layer_locked("DETAIL", true).unwrap();
        }
        let mut editor = Editor::new(protected);
        let before = editor.to_dxf();
        assert!(editor.break_polyline_vertices(1, 1, 2).is_err());
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
    }
    let mut editor = Editor::new(document);
    let before = editor.to_dxf();
    for (first, second) in [(0, 0), (0, 3), (3, 3), (1, 99)] {
        assert!(editor.break_polyline_vertices(1, first, second).is_err());
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
    }
    assert!(editor.break_polyline_vertices(0, 1, 2).is_err());
    assert!(!editor.break_polyline_vertices(99, 1, 2).unwrap());
    let invalid = Polyline2::with_bulges(
        vec![p(-1e308, 0.0, 7.0), p(0.0, 1.0, 7.0), p(1e308, 0.0, 7.0)],
        vec![1e308, 0.0, 0.0],
        false,
    )
    .unwrap();
    assert!(invalid.break_vertices(1, 1).is_err());
}
