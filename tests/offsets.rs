use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, GeometryError, Line3, Offset, OffsetLayer,
    OffsetOptions, Point3, Polyline2,
};
use std::f64::consts::{FRAC_PI_2, PI};

fn point(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
}

fn line_points(entity: Entity, start: Point3, end: Point3) {
    let Entity::Line(line) = entity else {
        panic!("expected line")
    };
    for (actual, expected) in [(line.start(), start), (line.end(), end)] {
        near(actual.x(), expected.x());
        near(actual.y(), expected.y());
        assert_eq!(actual.z(), expected.z());
    }
}

#[test]
fn line_offsets_use_perpendicular_distance_and_through_infinite_line() {
    let horizontal = Entity::Line(Line3::new(point(1.0, 2.0, 7.0), point(11.0, 2.0, 7.0)));
    for (side, expected_y) in [(20.0, 5.0), (-20.0, -1.0)] {
        line_points(
            horizontal
                .offset_xy(Offset::distance(3.0, point(100.0, side, -9.0)).unwrap())
                .unwrap(),
            point(1.0, expected_y, 7.0),
            point(11.0, expected_y, 7.0),
        );
    }
    // The pick lies beyond the finite segment. Through uses its supporting line.
    line_points(
        horizontal
            .offset_xy(Offset::through(point(-100.0, -4.0, -9.0)))
            .unwrap(),
        point(1.0, -4.0, 7.0),
        point(11.0, -4.0, 7.0),
    );
    let diagonal = Entity::Line(Line3::new(point(0.0, 0.0, 4.0), point(3.0, 4.0, 4.0)));
    line_points(
        diagonal
            .offset_xy(Offset::distance(5.0, point(-4.0, 3.0, 0.0)).unwrap())
            .unwrap(),
        point(-4.0, 3.0, 4.0),
        point(-1.0, 7.0, 4.0),
    );
    line_points(
        diagonal
            .offset_xy(Offset::through(point(4.0, -3.0, 0.0)))
            .unwrap(),
        point(4.0, -3.0, 4.0),
        point(7.0, 1.0, 4.0),
    );
}

#[test]
fn circular_offsets_keep_center_elevation_and_arc_sweep() {
    let center = point(2.0, 3.0, 8.0);
    for source in [
        Entity::Circle(Circle3::new(center, 5.0).unwrap()),
        Entity::Arc(Arc3::new(center, 5.0, PI, FRAC_PI_2).unwrap()),
    ] {
        for (offset, radius) in [
            (Offset::distance(2.0, point(12.0, 3.0, 0.0)).unwrap(), 7.0),
            (Offset::distance(2.0, center).unwrap(), 3.0),
            (Offset::through(point(2.0, -1.0, -10.0)), 4.0),
        ] {
            match source.offset_xy(offset).unwrap() {
                Entity::Circle(circle) => {
                    assert_eq!(circle.center(), center);
                    near(circle.radius(), radius);
                }
                Entity::Arc(arc) => {
                    assert_eq!(arc.center(), center);
                    near(arc.radius(), radius);
                    assert_eq!(arc.start_angle(), PI);
                    assert_eq!(arc.end_angle(), FRAC_PI_2);
                    near(arc.sweep(), 3.0 * FRAC_PI_2);
                }
                _ => panic!("expected original curve type"),
            }
        }
    }
}

#[test]
fn invalid_degenerate_collapsed_and_unrepresentable_offsets_fail() {
    let origin = point(0.0, 0.0, 0.0);
    for distance in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(Offset::distance(distance, origin).is_err());
    }
    let circle = Entity::Circle(Circle3::new(origin, 5.0).unwrap());
    for offset in [
        Offset::distance(5.0, origin).unwrap(),
        Offset::distance(6.0, origin).unwrap(),
        Offset::through(origin),
    ] {
        assert_eq!(circle.offset_xy(offset), Err(GeometryError::OffsetCollapse));
    }
    assert_eq!(
        circle.offset_xy(Offset::distance(1.0, point(5.0, 0.0, 0.0)).unwrap()),
        Err(GeometryError::UndefinedOffsetSide)
    );
    let line = Entity::Line(Line3::new(origin, point(10.0, 0.0, 0.0)));
    assert_eq!(
        line.offset_xy(Offset::through(point(20.0, 0.0, 9.0))),
        Err(GeometryError::UndefinedOffsetSide)
    );
    assert_eq!(
        Entity::Line(Line3::new(origin, origin)).offset_xy(Offset::through(point(0.0, 1.0, 0.0))),
        Err(GeometryError::DegenerateLine)
    );
    for unsupported in [
        Entity::Line(Line3::new(origin, point(10.0, 0.0, 1.0))),
        Entity::Polyline(Polyline2::new(vec![origin, point(10.0, 0.0, 0.0)], false).unwrap()),
    ] {
        assert_eq!(
            unsupported.offset_xy(Offset::through(point(0.0, 1.0, 0.0))),
            Err(GeometryError::UnsupportedOffsetEntity)
        );
    }
    let huge = Entity::Line(Line3::new(point(0.0, 1e30, 0.0), point(10.0, 1e30, 0.0)));
    assert_eq!(
        huge.offset_xy(Offset::distance(1.0, point(0.0, 1e30 + 1e20, 0.0)).unwrap()),
        Err(GeometryError::UnrepresentableOffset)
    );
    assert!(
        Entity::Circle(Circle3::new(origin, f64::MAX).unwrap())
            .offset_xy(Offset::through(point(f64::MAX, f64::MAX, 0.0)))
            .is_err()
    );
}

#[test]
fn editor_offsets_preserve_styles_and_apply_layer_erase_and_history_atomically() {
    let mut original = DxfDocument::default();
    original.add_layer("DETAIL").unwrap();
    original
        .push_on_layer(
            Entity::Line(Line3::new(point(0.0, 0.0, 3.0), point(10.0, 0.0, 3.0))),
            "DETAIL",
        )
        .unwrap();
    original.set_entity_color(0, 3).unwrap();
    original.set_entity_linetype_scale(0, 2.5).unwrap();
    original.push(Entity::Circle(
        Circle3::new(point(20.0, 0.0, 0.0), 3.0).unwrap(),
    ));
    let source = original.to_dxf();
    let offset = Offset::distance(2.0, point(0.0, 5.0, 0.0)).unwrap();
    for erase_source in [false, true] {
        for layer in [OffsetLayer::Source, OffsetLayer::Current] {
            let mut editor = Editor::new(DxfDocument::parse(&source).unwrap());
            editor.select(vec![0]);
            editor.set_current_layer("0").unwrap();
            assert!(
                editor
                    .offset_entity(
                        0,
                        offset,
                        OffsetOptions {
                            erase_source,
                            layer
                        }
                    )
                    .unwrap()
            );
            let created = if erase_source { 1 } else { 2 };
            assert_eq!(editor.selection(), &[created]);
            assert_eq!(editor.entities().len(), created + 1);
            assert_eq!(editor.entity_style(created), original.entity_style(0));
            assert_eq!(
                editor.entity_layer(created),
                Some(if layer == OffsetLayer::Source {
                    "DETAIL"
                } else {
                    "0"
                })
            );
            line_points(
                editor.entities()[created].clone(),
                point(0.0, 2.0, 3.0),
                point(10.0, 2.0, 3.0),
            );
            assert_eq!(
                editor.entities()[usize::from(!erase_source)],
                original.entities()[1]
            );
            let saved = editor.to_dxf();
            let reloaded = DxfDocument::parse(&saved).unwrap();
            assert_eq!(reloaded.entities(), editor.entities());
            assert_eq!(reloaded.entity_style(created), original.entity_style(0));
            assert!(editor.undo());
            assert_eq!(editor.to_dxf(), source);
            assert_eq!(editor.selection(), &[0]);
            assert!(!editor.is_dirty());
            assert!(!editor.can_undo());
            assert!(editor.redo());
            assert_eq!(editor.to_dxf(), saved);
        }
    }
    for (protected_layer, visible) in [
        ("DETAIL", true),
        ("0", true),
        ("DETAIL", false),
        ("0", false),
    ] {
        let mut protected = original.clone();
        if visible {
            protected.set_layer_locked(protected_layer, true).unwrap();
        } else {
            protected.set_layer_visible(protected_layer, false).unwrap();
        }
        let mut editor = Editor::new(protected);
        editor.select(vec![0]);
        let selection = editor.selection().to_vec();
        let before = editor.to_dxf();
        assert!(
            editor
                .offset_entity(
                    0,
                    offset,
                    OffsetOptions {
                        erase_source: true,
                        layer: OffsetLayer::Current
                    }
                )
                .is_err()
        );
        assert_eq!(editor.to_dxf(), before);
        assert_eq!(editor.selection(), selection);
        assert!(!editor.can_undo());
    }
    let mut editor = Editor::new(original);
    let before = editor.to_dxf();
    assert!(
        editor
            .offset_entity(
                1,
                Offset::through(point(20.0, 0.0, 0.0)),
                OffsetOptions::default()
            )
            .is_err()
    );
    assert!(
        !editor
            .offset_entity(99, offset, OffsetOptions::default())
            .unwrap()
    );
    assert_eq!(editor.to_dxf(), before);
    assert!(!editor.can_undo());
}
