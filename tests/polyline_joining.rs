use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, GeometryError, Line3, Point3, Polyline2,
};

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 7.0).unwrap()
}
fn line(a: f64, b: f64) -> Entity {
    Entity::Line(Line3::new(p(a, 0.0), p(b, 0.0)))
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn endpoint_join_attaches_at_both_ends_and_reverses_candidate_bulges() {
    let source =
        Polyline2::with_bulges(vec![p(0.0, 0.0), p(10.0, 0.0)], vec![0.2, 0.75], false).unwrap();
    for (vertices, bulges, expected_vertices, expected_bulges) in [
        (
            vec![p(10.0, 0.0), p(20.0, 0.0), p(20.0, 10.0)],
            vec![-1.0, 0.5, 0.25],
            vec![p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0), p(20.0, 10.0)],
            vec![0.2, -1.0, 0.5, 0.25],
        ),
        (
            vec![p(-20.0, 10.0), p(-10.0, 0.0), p(0.0, 0.0)],
            vec![0.5, -1.0, 0.33],
            vec![p(-20.0, 10.0), p(-10.0, 0.0), p(0.0, 0.0), p(10.0, 0.0)],
            vec![0.5, -1.0, 0.2, 0.75],
        ),
    ] {
        let candidate = Polyline2::with_bulges(vertices, bulges, false).unwrap();
        for reversed in [false, true] {
            let mut input = candidate.clone();
            if reversed {
                input.reverse();
            }
            let result = source
                .join_endpoints_xy(&Entity::Polyline(input))
                .unwrap()
                .unwrap();
            assert_eq!(result.vertices(), expected_vertices);
            // Reversing a candidate changes its dormant final bulge; active
            // segments must still describe exactly the original curved path.
            assert_eq!(
                &result.bulges()[..result.bulges().len() - 1],
                &expected_bulges[..expected_bulges.len() - 1]
            );
            near(
                result.length().unwrap(),
                source.length().unwrap() + candidate.length().unwrap(),
            );
            assert!(!result.is_closed());
        }
    }
}

#[test]
fn endpoint_join_preserves_minor_major_and_wrapped_arc_loci_in_both_directions() {
    for (start, end) in [(0.0_f64, 90.0_f64), (0.0, 270.0), (300.0, 20.0)] {
        let arc = Arc3::new(p(30.0, -20.0), 10.0, start.to_radians(), end.to_radians()).unwrap();
        for reversed in [false, true] {
            let junction = if reversed {
                arc.end_point().unwrap()
            } else {
                arc.start_point().unwrap()
            };
            let source = Polyline2::new(
                vec![p(junction.x() - 10.0, junction.y() - 10.0), junction],
                false,
            )
            .unwrap();
            let result = source
                .join_endpoints_xy(&Entity::Arc(arc))
                .unwrap()
                .unwrap();
            near(
                result.length().unwrap(),
                source.length().unwrap() + arc.length().unwrap(),
            );
            let segment = result.segments().nth(1).unwrap().unwrap();
            assert_eq!(result.bulges()[1] < 0.0, reversed);
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let expected = arc
                    .point_at_fraction(if reversed { 1.0 - fraction } else { fraction })
                    .unwrap();
                let actual = segment.point_at_fraction(fraction).unwrap();
                near(actual.x(), expected.x());
                near(actual.y(), expected.y());
                assert_eq!(actual.z(), 7.0);
            }
        }
    }
}

#[test]
fn endpoint_join_uses_numerical_allowance_without_bridging_gaps_or_implicitly_closing() {
    let source = Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap();
    let near_endpoint = Entity::Line(Line3::new(p(10.0 + 5e-11, 0.0), p(20.0, 0.0)));
    let result = source.join_endpoints_xy(&near_endpoint).unwrap().unwrap();
    assert_eq!(
        result.vertices(),
        &[p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)]
    );
    assert_eq!(result.length().unwrap(), 20.0);
    let mut closed = source.clone();
    closed.set_closed(true);
    for incompatible in [
        line(10.00001, 20.0),
        Entity::Line(Line3::new(
            p(10.0, 0.0),
            Point3::new(20.0, 0.0, 8.0).unwrap(),
        )),
        Entity::Line(Line3::new(
            Point3::new(10.0, 0.0, 8.0).unwrap(),
            Point3::new(20.0, 0.0, 8.0).unwrap(),
        )),
        Entity::Circle(Circle3::new(p(10.0, 0.0), 10.0).unwrap()),
        Entity::Polyline(closed.clone()),
    ] {
        assert_eq!(source.join_endpoints_xy(&incompatible).unwrap(), None);
    }
    assert_eq!(
        closed.join_endpoints_xy(&line(10.0, 20.0)),
        Err(GeometryError::ClosedPolylineJoin)
    );
    assert_eq!(
        source.join_endpoints_xy(&Entity::Arc(
            Arc3::new(p(10.0, 0.0), 0.0, 0.0, 1.0).unwrap()
        )),
        Err(GeometryError::DegenerateCurve)
    );
    let looped = source.join_endpoints_xy(&line(10.0, 0.0)).unwrap().unwrap();
    assert_eq!(looped.vertices().first(), looped.vertices().last());
    assert!(!looped.is_closed());
    let large = Polyline2::new(vec![p(1e9, 1e9), p(1e9 + 10.0, 1e9)], false).unwrap();
    let candidate = Entity::Line(Line3::new(p(1e9 + 10.0 + 1e-6, 1e9), p(1e9 + 20.0, 1e9)));
    assert!(large.join_endpoints_xy(&candidate).unwrap().is_some());
    assert!(
        large
            .join_endpoints_xy(&Entity::Line(Line3::new(
                p(1e9 + 10.01, 1e9),
                p(1e9 + 20.0, 1e9)
            )))
            .unwrap()
            .is_none()
    );
    let huge = Polyline2::new(vec![p(-1e308, 0.0), p(0.0, 0.0)], false).unwrap();
    assert_eq!(
        huge.join_endpoints_xy(&line(0.0, 1e308)),
        Err(GeometryError::NonFiniteResult)
    );
}

#[test]
fn editor_polyline_join_consumes_unordered_chains_preserves_source_record_and_undoes_once() {
    let mut document = DxfDocument::default();
    document.add_layer("SOURCE").unwrap();
    document.push(line(30.0, 20.0));
    document.push(Entity::Circle(Circle3::new(p(100.0, 100.0), 5.0).unwrap()));
    document
        .push_on_layer(
            Entity::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap()),
            "SOURCE",
        )
        .unwrap();
    document.set_entity_color(2, 3).unwrap();
    document.set_entity_linetype_scale(2, 2.0).unwrap();
    document.push(line(20.0, 10.0));
    document.push(line(0.0, -10.0));
    document.push(Entity::Polyline(
        Polyline2::with_bulges(
            vec![p(30.0, 0.0), p(40.0, 0.0), p(40.0, 10.0)],
            vec![0.0, 1.0, 0.0],
            false,
        )
        .unwrap(),
    ));
    let original = document
        .to_dxf()
        .replacen("POLYLINE\n", "POLYLINE\n5\nA1\n999\nsource-identity\n", 1)
        .replacen("CIRCLE\n", "CIRCLE\n999\nunrelated-identity\n", 1);
    for candidates in [vec![5, 4, 3, 2, 1, 0, 3, 999], vec![0, 1, 2, 3, 4, 5]] {
        let mut editor = Editor::new(DxfDocument::parse(&original).unwrap());
        let style = editor.entity_style(2).unwrap().clone();
        editor.select(vec![2]);
        assert_eq!(editor.join_polyline_entities(2, &candidates).unwrap(), 4);
        assert_eq!(editor.entities().len(), 2);
        assert_eq!(editor.selection(), &[1]);
        let Entity::Polyline(result) = &editor.entities()[1] else {
            panic!("polyline")
        };
        assert_eq!(
            result.vertices(),
            &[
                p(-10.0, 0.0),
                p(0.0, 0.0),
                p(10.0, 0.0),
                p(20.0, 0.0),
                p(30.0, 0.0),
                p(40.0, 0.0),
                p(40.0, 10.0)
            ]
        );
        near(result.length().unwrap(), 50.0 + 5.0 * std::f64::consts::PI);
        assert_eq!(editor.entity_layer(1), Some("SOURCE"));
        assert_eq!(editor.entity_style(1), Some(&style));
        let joined = editor.to_dxf();
        assert!(joined.contains("source-identity"));
        assert!(joined.contains("unrelated-identity"));
        assert_eq!(
            DxfDocument::parse(&joined).unwrap().entities(),
            editor.entities()
        );
        assert_eq!(editor.join_polyline_entities(1, &[0, 1, 999]).unwrap(), 0);
        assert!(editor.undo());
        assert_eq!(editor.to_dxf(), original);
        assert_eq!(editor.selection(), &[2]);
        assert!(!editor.can_undo());
        assert!(editor.redo());
        assert_eq!(editor.to_dxf(), joined);
    }
}

#[test]
fn editor_polyline_join_preflights_protection_invalid_geometry_and_branch_order() {
    let mut document = DxfDocument::default();
    document.add_layer("PROTECTED").unwrap();
    document.push(Entity::Polyline(
        Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap(),
    ));
    document.push(line(10.0, 20.0));
    document
        .push_on_layer(line(20.0, 30.0), "PROTECTED")
        .unwrap();
    document.set_layer_locked("PROTECTED", true).unwrap();
    let mut editor = Editor::new(document.clone());
    editor.select(vec![0, 1, 2]);
    let original = editor.to_dxf();
    assert!(editor.join_polyline_entities(0, &[1, 2]).is_err());
    assert_eq!(editor.to_dxf(), original);
    assert_eq!(editor.selection(), &[0, 1, 2]);
    assert!(!editor.can_undo());
    for hidden in [false, true] {
        let mut protected_source = document.clone();
        protected_source.set_entity_layer(0, "PROTECTED").unwrap();
        if hidden {
            protected_source
                .set_layer_locked("PROTECTED", false)
                .unwrap();
            protected_source
                .set_layer_visible("PROTECTED", false)
                .unwrap();
        }
        let mut protected_editor = Editor::new(protected_source);
        let before = protected_editor.to_dxf();
        assert!(protected_editor.join_polyline_entities(0, &[1]).is_err());
        assert_eq!(protected_editor.to_dxf(), before);
        assert!(!protected_editor.can_undo());
    }
    document.set_layer_visible("PROTECTED", false).unwrap();
    let mut editor = Editor::new(document);
    assert_eq!(editor.join_polyline_entities(0, &[1, 2]).unwrap(), 1);
    assert_eq!(editor.entities().len(), 2);
    assert_eq!(editor.join_polyline_entities(999, &[0]).unwrap(), 0);

    let mut invalid = DxfDocument::default();
    invalid.push(Entity::Polyline(
        Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap(),
    ));
    invalid.push(line(10.0, 20.0));
    invalid.push(Entity::Arc(Arc3::new(p(20.0, 0.0), 0.0, 0.0, 1.0).unwrap()));
    let mut editor = Editor::new(invalid);
    let original = editor.to_dxf();
    assert!(editor.join_polyline_entities(0, &[1, 2]).is_err());
    assert_eq!(editor.to_dxf(), original);
    assert!(!editor.can_undo());

    let mut branch = DxfDocument::default();
    branch.push(line(10.0, 20.0));
    branch.push(Entity::Line(Line3::new(p(10.0, 0.0), p(10.0, 10.0))));
    branch.push(Entity::Polyline(
        Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap(),
    ));
    let mut editor = Editor::new(branch);
    assert_eq!(editor.join_polyline_entities(2, &[1, 0]).unwrap(), 1);
    assert_eq!(
        editor.entities()[0],
        Entity::Line(Line3::new(p(10.0, 0.0), p(10.0, 10.0)))
    );
    let Entity::Polyline(result) = &editor.entities()[1] else {
        panic!("polyline")
    };
    assert_eq!(result.vertices().last(), Some(&p(20.0, 0.0)));
}
