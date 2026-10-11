use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, GeometryError, Lengthen, Line3, Point3, Polyline2,
};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 7.0).unwrap()
}
fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Entity {
    Entity::Line(Line3::new(p(x1, y1), p(x2, y2)))
}
fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}
fn near_point(actual: Point3, expected: Point3) {
    near(actual.x(), expected.x());
    near(actual.y(), expected.y());
    assert_eq!(actual.z(), expected.z());
}

#[test]
fn lengthen_lines_change_only_the_picked_endpoint_and_project_dynamic_points() {
    let cases = [
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(10.0, 1.0),
            Lengthen::Delta(5.0),
            p(0.0, 0.0),
            p(15.0, 0.0),
            false,
        ),
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(-1.0, 0.0),
            Lengthen::Delta(-3.0),
            p(3.0, 0.0),
            p(10.0, 0.0),
            true,
        ),
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(0.0, 0.0),
            Lengthen::Percent(50.0),
            p(5.0, 0.0),
            p(10.0, 0.0),
            true,
        ),
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(10.0, 0.0),
            Lengthen::Percent(200.0),
            p(0.0, 0.0),
            p(20.0, 0.0),
            false,
        ),
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(0.0, 0.0),
            Lengthen::Total(4.0),
            p(6.0, 0.0),
            p(10.0, 0.0),
            true,
        ),
        (
            line(10.0, 0.0, 0.0, 0.0),
            p(10.0, 0.0),
            Lengthen::Delta(5.0),
            p(15.0, 0.0),
            p(0.0, 0.0),
            true,
        ),
        (
            line(10.0, 0.0, 0.0, 0.0),
            p(0.0, 0.0),
            Lengthen::Delta(-4.0),
            p(10.0, 0.0),
            p(4.0, 0.0),
            false,
        ),
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(10.0, 0.0),
            Lengthen::Dynamic(Point3::new(16.0, 8.0, -50.0).unwrap()),
            p(0.0, 0.0),
            p(16.0, 0.0),
            false,
        ),
        (
            line(0.0, 0.0, 10.0, 0.0),
            p(0.0, 0.0),
            Lengthen::Dynamic(p(-5.0, 12.0)),
            p(-5.0, 0.0),
            p(10.0, 0.0),
            true,
        ),
        (
            line(1.0, 2.0, 7.0, 10.0),
            p(1.0, 2.0),
            Lengthen::Delta(5.0),
            p(-2.0, -2.0),
            p(7.0, 10.0),
            true,
        ),
        (
            line(1.0, 2.0, 7.0, 10.0),
            p(7.0, 10.0),
            Lengthen::Percent(200.0),
            p(1.0, 2.0),
            p(13.0, 18.0),
            false,
        ),
        (
            line(1.0, 2.0, 7.0, 10.0),
            p(7.0, 10.0),
            Lengthen::Dynamic(p(13.0, 7.0)),
            p(1.0, 2.0),
            p(7.72, 10.96),
            false,
        ),
    ];
    for (source, pick, operation, start, end, moved_start) in cases {
        let Entity::Line(original) = source else {
            unreachable!()
        };
        let Entity::Line(result) = source.lengthen_xy(pick, operation).unwrap() else {
            panic!("lengthen must retain the source type");
        };
        near_point(result.start(), start);
        near_point(result.end(), end);
        if moved_start {
            assert_eq!(result.end(), original.end());
        } else {
            assert_eq!(result.start(), original.start());
        }
    }
}

#[test]
fn lengthen_arcs_preserve_radius_and_opposite_angle_across_zero() {
    let original = Arc3::new(p(3.0, 4.0), 10.0, 3.0 * FRAC_PI_2, FRAC_PI_2).unwrap();
    let source = Entity::Arc(original);
    let operations = [
        (Lengthen::Delta(-5.0 * PI), FRAC_PI_2),
        (Lengthen::Percent(50.0), FRAC_PI_2),
        (Lengthen::Total(5.0 * PI), FRAC_PI_2),
        (Lengthen::DeltaAngle(PI / 4.0), 5.0 * PI / 4.0),
        (Lengthen::TotalAngle(3.0 * FRAC_PI_2), 3.0 * FRAC_PI_2),
        (Lengthen::Dynamic(p(53.0, 4.0)), FRAC_PI_2),
        (Lengthen::Dynamic(p(-47.0, 4.0)), 3.0 * FRAC_PI_2),
    ];
    for (operation, sweep) in operations {
        for start in [true, false] {
            let pick = if start { p(3.0, -6.0) } else { p(3.0, 14.0) };
            let Entity::Arc(result) = source.lengthen_xy(pick, operation).unwrap() else {
                panic!("expected arc");
            };
            assert_eq!(result.center(), original.center());
            assert_eq!(result.radius(), original.radius());
            near(result.sweep(), sweep);
            near(result.length().unwrap(), 10.0 * sweep);
            if start {
                assert_eq!(result.end_angle(), original.end_angle());
                assert_eq!(result.end_point().unwrap(), original.end_point().unwrap());
                near(
                    result.start_angle(),
                    (original.end_angle() - sweep).rem_euclid(TAU),
                );
            } else {
                assert_eq!(result.start_angle(), original.start_angle());
                assert_eq!(
                    result.start_point().unwrap(),
                    original.start_point().unwrap()
                );
                near(
                    result.end_angle(),
                    (original.start_angle() + sweep).rem_euclid(TAU),
                );
            }
        }
    }
}

#[test]
fn lengthen_rejects_invalid_ambiguous_and_unrepresentable_changes_and_keeps_noops() {
    let straight = line(0.0, 0.0, 10.0, 0.0);
    let round = Entity::Arc(Arc3::new(p(0.0, 0.0), 10.0, 0.0, FRAC_PI_2).unwrap());
    for source in [&straight, &round] {
        for operation in [Lengthen::Delta(0.0), Lengthen::Percent(100.0)] {
            assert_eq!(
                source.lengthen_xy(p(10.0, 0.0), operation).unwrap(),
                *source
            );
        }
        for operation in [
            Lengthen::Percent(0.0),
            Lengthen::Percent(-1.0),
            Lengthen::Total(0.0),
            Lengthen::Total(-1.0),
            Lengthen::Delta(f64::NAN),
            Lengthen::Delta(f64::INFINITY),
            Lengthen::TotalAngle(0.0),
            Lengthen::TotalAngle(TAU),
            Lengthen::DeltaAngle(f64::NAN),
        ] {
            assert_eq!(
                source.lengthen_xy(p(10.0, 0.0), operation),
                Err(GeometryError::InvalidLengthenValue)
            );
        }
    }
    assert_eq!(
        straight
            .lengthen_xy(p(10.0, 0.0), Lengthen::Total(10.0))
            .unwrap(),
        straight
    );
    assert_eq!(
        straight
            .lengthen_xy(p(10.0, 0.0), Lengthen::Dynamic(p(10.0, 99.0)))
            .unwrap(),
        straight
    );
    for operation in [
        Lengthen::Delta(-10.0),
        Lengthen::Delta(-20.0),
        Lengthen::Dynamic(p(0.0, 5.0)),
        Lengthen::Dynamic(p(-1.0, 0.0)),
    ] {
        assert_eq!(
            straight.lengthen_xy(p(10.0, 0.0), operation),
            Err(GeometryError::InvalidLengthenValue)
        );
    }
    assert_eq!(
        straight.lengthen_xy(p(5.0, 99.0), Lengthen::Delta(1.0)),
        Err(GeometryError::AmbiguousCurvePick)
    );
    assert_eq!(
        straight.lengthen_xy(p(10.0, 0.0), Lengthen::TotalAngle(PI)),
        Err(GeometryError::UnsupportedLengthenAngle)
    );
    assert_eq!(
        round
            .lengthen_xy(p(10.0, 0.0), Lengthen::DeltaAngle(0.0))
            .unwrap(),
        round
    );
    assert_eq!(
        round
            .lengthen_xy(p(10.0, 0.0), Lengthen::TotalAngle(FRAC_PI_2))
            .unwrap(),
        round
    );
    for operation in [
        Lengthen::DeltaAngle(-FRAC_PI_2),
        Lengthen::Total(TAU * 10.0),
        Lengthen::Percent(400.0),
        Lengthen::Dynamic(p(0.0, 10.0)),
    ] {
        assert_eq!(
            round.lengthen_xy(p(10.0, 0.0), operation),
            Err(GeometryError::InvalidLengthenValue)
        );
    }
    assert_eq!(
        round.lengthen_xy(p(10.0, 0.0), Lengthen::Dynamic(p(0.0, 0.0))),
        Err(GeometryError::AmbiguousCurvePick)
    );
    assert_eq!(
        round.lengthen_xy(p(10.0, 0.0), Lengthen::DeltaAngle(f64::MIN_POSITIVE)),
        Err(GeometryError::UnrepresentableLengthen)
    );
    assert_eq!(
        straight.lengthen_xy(p(10.0, 0.0), Lengthen::Delta(f64::EPSILON)),
        Err(GeometryError::UnrepresentableLengthen)
    );
    assert_eq!(
        line(1e16, 0.0, 1e16 + 10.0, 0.0).lengthen_xy(p(1e16 + 10.0, 0.0), Lengthen::Delta(0.1)),
        Err(GeometryError::UnrepresentableLengthen)
    );
    assert_eq!(
        line(-1e308, 0.0, 1e308, 0.0).lengthen_xy(p(1e308, 0.0), Lengthen::Delta(1.0)),
        Err(GeometryError::NonFiniteResult)
    );
    for source in [
        line(1.0, 1.0, 1.0, 1.0),
        Entity::Arc(Arc3::new(p(0.0, 0.0), 10.0, 0.0, 0.0).unwrap()),
    ] {
        assert_eq!(
            source.lengthen_xy(p(10.0, 0.0), Lengthen::Delta(1.0)),
            Err(GeometryError::DegenerateCurve)
        );
    }
    for source in [
        Entity::Line(Line3::new(
            p(0.0, 0.0),
            Point3::new(10.0, 0.0, 8.0).unwrap(),
        )),
        Entity::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false).unwrap()),
    ] {
        assert_eq!(
            source.lengthen_xy(p(10.0, 0.0), Lengthen::Delta(1.0)),
            Err(GeometryError::UnsupportedLengthenEntity)
        );
    }
    let circle = Entity::Circle(Circle3::new(p(0.0, 0.0), 10.0).unwrap());
    assert_eq!(
        circle
            .lengthen_xy(p(0.0, 0.0), Lengthen::Percent(200.0))
            .unwrap(),
        circle
    );
}

#[test]
fn editor_lengthen_preserves_identity_styles_and_elevation_with_atomic_history() {
    let mut document = DxfDocument::default();
    document.push(line(0.0, 0.0, 10.0, 0.0));
    document.push(Entity::Arc(
        Arc3::new(p(20.0, 0.0), 5.0, 0.0, FRAC_PI_2).unwrap(),
    ));
    document.push(Entity::Circle(Circle3::new(p(40.0, 0.0), 5.0).unwrap()));
    document.add_layer("DETAIL").unwrap();
    for index in 0..3 {
        document.set_entity_layer(index, "DETAIL").unwrap();
        document.set_entity_color(index, 3).unwrap();
        document.set_entity_linetype(index, "CONTINUOUS").unwrap();
        document.set_entity_linetype_scale(index, 2.0).unwrap();
    }
    let source = document
        .to_dxf()
        .replacen("0\nLINE\n", "0\nLINE\n5\nAB\n999\nkeep-lengthen\n", 1);
    let document = DxfDocument::parse(&source).unwrap();
    let style = document.entity_style(0).unwrap().clone();
    let mut editor = Editor::new(document.clone());
    assert!(
        !editor
            .lengthen_entity(99, p(10.0, 0.0), Lengthen::Delta(5.0))
            .unwrap()
    );
    assert!(
        editor
            .lengthen_entity(0, p(5.0, 0.0), Lengthen::Delta(5.0))
            .is_err()
    );
    assert!(
        !editor
            .lengthen_entity(0, p(10.0, 0.0), Lengthen::Delta(0.0))
            .unwrap()
    );
    assert!(
        !editor
            .lengthen_entity(2, p(45.0, 0.0), Lengthen::Percent(200.0))
            .unwrap()
    );
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
    assert!(
        editor
            .lengthen_entity(0, p(10.0, 0.0), Lengthen::Delta(5.0))
            .unwrap()
    );
    assert_eq!(editor.entities().len(), 3);
    assert_eq!(editor.selection(), [0]);
    assert_eq!(editor.entities()[0], line(0.0, 0.0, 15.0, 0.0));
    assert_eq!(editor.entity_layer(0), Some("DETAIL"));
    assert_eq!(editor.entity_style(0), Some(&style));
    let saved = editor.to_dxf();
    assert!(saved.contains("5\nAB\n"));
    assert!(saved.contains("keep-lengthen"));
    assert_eq!(
        DxfDocument::parse(&saved).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), saved);
    assert!(
        editor
            .lengthen_entity(1, p(25.0, 0.0), Lengthen::DeltaAngle(FRAC_PI_2))
            .unwrap()
    );
    let Entity::Arc(result) = editor.entities()[1] else {
        panic!("expected arc")
    };
    assert_eq!(result.end_angle(), FRAC_PI_2);
    near(result.sweep(), PI);
    assert_eq!(editor.entity_style(1), Some(&style));
    assert_eq!(
        DxfDocument::parse(&editor.to_dxf()).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), saved);
    for visible in [true, false] {
        let mut protected = document.clone();
        if visible {
            protected.set_layer_locked("DETAIL", true).unwrap();
        } else {
            protected.set_layer_visible("DETAIL", false).unwrap();
        }
        let mut editor = Editor::new(protected);
        let before = editor.to_dxf();
        assert!(
            editor
                .lengthen_entity(0, p(10.0, 0.0), Lengthen::Delta(5.0))
                .is_err()
        );
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
    }
}
