use opencad::{
    Arc3, BoundaryMode, Circle3, DxfDocument, Editor, Entity, GeometryError, Line3, Point3,
    Polyline2,
};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn point(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 7.0).unwrap()
}

fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Entity {
    Entity::Line(Line3::new(point(x1, y1), point(x2, y2)))
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
}

fn near_point(actual: Point3, expected: Point3) {
    near(actual.x(), expected.x());
    near(actual.y(), expected.y());
    assert_eq!(actual.z(), expected.z());
}

fn endpoints(entity: &Entity, start: Point3, end: Point3) {
    match entity {
        Entity::Line(line) => {
            near_point(line.start(), start);
            near_point(line.end(), end);
        }
        Entity::Arc(arc) => {
            near_point(arc.start_point().unwrap(), start);
            near_point(arc.end_point().unwrap(), end);
        }
        _ => panic!("expected open curve"),
    }
}

#[test]
fn line_trim_picks_adjacent_cuts_and_respects_boundary_extents_and_elevation() {
    let source = line(0.0, 0.0, 10.0, 0.0);
    let boundaries = [line(3.0, -2.0, 3.0, 2.0), line(7.0, -2.0, 7.0, 2.0)];
    let split = source
        .trim_xy(&boundaries, point(5.0, 0.0), BoundaryMode::NoExtend)
        .unwrap();
    assert_eq!(split, [line(0.0, 0.0, 3.0, 0.0), line(7.0, 0.0, 10.0, 0.0)]);
    assert_eq!(
        source
            .trim_xy(&boundaries, point(0.0, 0.0), BoundaryMode::NoExtend)
            .unwrap(),
        [line(3.0, 0.0, 10.0, 0.0)]
    );
    assert_eq!(
        source
            .trim_xy(&boundaries, point(10.0, 0.0), BoundaryMode::NoExtend)
            .unwrap(),
        [line(0.0, 0.0, 7.0, 0.0)]
    );
    let reversed = line(10.0, 0.0, 0.0, 0.0)
        .trim_xy(&boundaries, point(5.0, 0.0), BoundaryMode::NoExtend)
        .unwrap();
    assert_eq!(reversed.len(), 2);
    endpoints(&reversed[0], point(10.0, 0.0), point(7.0, 0.0));
    endpoints(&reversed[1], point(3.0, 0.0), point(0.0, 0.0));
    let short = [line(3.0, 2.0, 3.0, 4.0), line(7.0, 2.0, 7.0, 4.0)];
    assert_eq!(
        source.trim_xy(&short, point(5.0, 0.0), BoundaryMode::NoExtend),
        Err(GeometryError::NoBoundaryIntersection)
    );
    assert_eq!(
        source
            .trim_xy(&short, point(5.0, 0.0), BoundaryMode::Extend)
            .unwrap(),
        split
    );
    let circle = Entity::Circle(Circle3::new(point(5.0, 0.0), 2.0).unwrap());
    assert_eq!(
        source
            .trim_xy(&[circle], point(5.0, 0.0), BoundaryMode::NoExtend)
            .unwrap(),
        split
    );
    let arc = Entity::Arc(Arc3::new(point(5.0, 0.0), 2.0, FRAC_PI_2, PI).unwrap());
    assert_eq!(
        source
            .trim_xy(
                std::slice::from_ref(&arc),
                point(1.0, 0.0),
                BoundaryMode::NoExtend
            )
            .unwrap(),
        [line(3.0, 0.0, 10.0, 0.0)]
    );
    assert_eq!(
        source
            .trim_xy(&[arc], point(5.0, 0.0), BoundaryMode::Extend)
            .unwrap(),
        split
    );
    let wrong_elevation = Entity::Line(Line3::new(
        Point3::new(3.0, -2.0, 8.0).unwrap(),
        Point3::new(3.0, 2.0, 8.0).unwrap(),
    ));
    assert_eq!(
        source.trim_xy(&[wrong_elevation], point(1.0, 0.0), BoundaryMode::Extend),
        Err(GeometryError::NoBoundaryIntersection)
    );
    assert_eq!(
        source.trim_xy(&boundaries, point(3.0, 0.0), BoundaryMode::NoExtend),
        Err(GeometryError::AmbiguousCurvePick)
    );
    let curved_boundary = Entity::Polyline(
        Polyline2::with_bulges(
            vec![point(-5.0, 0.0), point(5.0, 0.0)],
            vec![1.0, 0.0],
            false,
        )
        .unwrap(),
    );
    let split = line(-10.0, -3.0, 10.0, -3.0)
        .trim_xy(&[curved_boundary], point(0.0, -3.0), BoundaryMode::NoExtend)
        .unwrap();
    endpoints(&split[0], point(-10.0, -3.0), point(-4.0, -3.0));
    endpoints(&split[1], point(4.0, -3.0), point(10.0, -3.0));
    // Fractional duplicate supports must not invent a cut from determinant rounding.
    let diagonal = line(0.1, 0.2, 1.3, 2.7);
    assert_eq!(
        diagonal.trim_xy(
            std::slice::from_ref(&diagonal),
            point(0.5, 1.0),
            BoundaryMode::NoExtend
        ),
        Err(GeometryError::NoBoundaryIntersection)
    );
}

#[test]
fn circular_trims_keep_the_unpicked_arc_and_split_wrapped_arcs() {
    let circle = Entity::Circle(Circle3::new(point(0.0, 0.0), 5.0).unwrap());
    let vertical = [line(0.0, -10.0, 0.0, 10.0)];
    for (pick, start, end) in [
        (point(5.0, 0.0), point(0.0, 5.0), point(0.0, -5.0)),
        (point(-5.0, 0.0), point(0.0, -5.0), point(0.0, 5.0)),
    ] {
        let result = circle
            .trim_xy(&vertical, pick, BoundaryMode::NoExtend)
            .unwrap();
        assert_eq!(result.len(), 1);
        endpoints(&result[0], start, end);
        let Entity::Arc(arc) = result[0] else {
            unreachable!()
        };
        near(arc.sweep(), PI);
    }
    let other = Entity::Circle(Circle3::new(point(6.0, 0.0), 5.0).unwrap());
    let result = circle
        .trim_xy(&[other], point(5.0, 0.0), BoundaryMode::NoExtend)
        .unwrap();
    endpoints(&result[0], point(3.0, 4.0), point(3.0, -4.0));
    let Entity::Arc(arc) = result[0] else {
        unreachable!()
    };
    near(arc.sweep(), TAU - 2.0 * (4.0_f64 / 3.0).atan());
    for boundary in [line(5.0, -10.0, 5.0, 10.0), circle.clone()] {
        assert_eq!(
            circle.trim_xy(&[boundary], point(-5.0, 0.0), BoundaryMode::NoExtend),
            Err(GeometryError::NoBoundaryIntersection)
        );
    }
    let arc = Entity::Arc(Arc3::new(point(0.0, 0.0), 5.0, 3.0 * FRAC_PI_2, FRAC_PI_2).unwrap());
    let result = arc
        .trim_xy(
            &[line(0.0, -3.0, 10.0, -3.0), line(0.0, 3.0, 10.0, 3.0)],
            point(5.0, 0.0),
            BoundaryMode::NoExtend,
        )
        .unwrap();
    assert_eq!(result.len(), 2);
    endpoints(&result[0], point(0.0, -5.0), point(4.0, -3.0));
    endpoints(&result[1], point(4.0, 3.0), point(0.0, 5.0));
    for part in result {
        let Entity::Arc(part) = part else {
            unreachable!()
        };
        near(part.sweep(), FRAC_PI_2 - (3.0_f64 / 4.0).atan());
    }
}

#[test]
fn extending_chooses_the_near_endpoint_and_first_boundary_on_its_path() {
    let boundaries = [
        line(-3.0, -2.0, -3.0, 2.0),
        line(5.0, -2.0, 5.0, 2.0),
        line(7.0, -2.0, 7.0, 2.0),
    ];
    let source = line(0.0, 0.0, 2.0, 0.0);
    assert_eq!(
        source
            .extend_xy(&boundaries, point(0.0, 0.0), BoundaryMode::NoExtend)
            .unwrap(),
        line(-3.0, 0.0, 2.0, 0.0)
    );
    assert_eq!(
        source
            .extend_xy(&boundaries, point(2.0, 0.0), BoundaryMode::NoExtend)
            .unwrap(),
        line(0.0, 0.0, 5.0, 0.0)
    );
    assert_eq!(
        source.extend_xy(&boundaries, point(1.0, 0.0), BoundaryMode::NoExtend),
        Err(GeometryError::AmbiguousCurvePick)
    );
    let short = [line(5.0, 2.0, 5.0, 3.0)];
    assert_eq!(
        source.extend_xy(&short, point(2.0, 0.0), BoundaryMode::NoExtend),
        Err(GeometryError::NoBoundaryIntersection)
    );
    assert_eq!(
        source
            .extend_xy(&short, point(2.0, 0.0), BoundaryMode::Extend)
            .unwrap(),
        line(0.0, 0.0, 5.0, 0.0)
    );
    let arc = Entity::Arc(Arc3::new(point(0.0, 0.0), 5.0, 0.0, FRAC_PI_2).unwrap());
    let extended_end = arc
        .extend_xy(
            &[line(-3.0, 0.0, -3.0, 10.0)],
            point(0.0, 5.0),
            BoundaryMode::NoExtend,
        )
        .unwrap();
    endpoints(&extended_end, point(5.0, 0.0), point(-3.0, 4.0));
    let extended_start = arc
        .extend_xy(
            &[line(0.0, -3.0, 10.0, -3.0)],
            point(5.0, 0.0),
            BoundaryMode::NoExtend,
        )
        .unwrap();
    endpoints(&extended_start, point(4.0, -3.0), point(0.0, 5.0));
    assert_eq!(
        Entity::Circle(Circle3::new(point(0.0, 0.0), 5.0).unwrap()).extend_xy(
            &boundaries,
            point(5.0, 0.0),
            BoundaryMode::NoExtend
        ),
        Err(GeometryError::UnsupportedExtendEntity)
    );
    assert_eq!(
        line(0.0, 0.0, 0.0, 0.0).trim_xy(&boundaries, point(1.0, 0.0), BoundaryMode::NoExtend),
        Err(GeometryError::DegenerateCurve)
    );
    let unsupported = Entity::Line(Line3::new(
        point(0.0, 0.0),
        Point3::new(2.0, 0.0, 9.0).unwrap(),
    ));
    assert_eq!(
        unsupported.trim_xy(&boundaries, point(1.0, 0.0), BoundaryMode::NoExtend),
        Err(GeometryError::UnsupportedTrimEntity)
    );
    let huge = line(-f64::MAX, 0.0, f64::MAX, 0.0);
    assert!(
        huge.trim_xy(&boundaries, point(1.0, 0.0), BoundaryMode::NoExtend)
            .is_err()
    );
}

#[test]
fn editor_splits_converts_extends_and_restores_source_records_in_one_step() {
    let mut original = DxfDocument::default();
    original.add_layer("DETAIL").unwrap();
    original
        .push_on_layer(line(0.0, 0.0, 10.0, 0.0), "DETAIL")
        .unwrap();
    original.set_entity_color(0, 3).unwrap();
    original.set_entity_linetype_scale(0, 2.5).unwrap();
    original.push(line(3.0, -2.0, 3.0, 2.0));
    original.push(line(7.0, -2.0, 7.0, 2.0));
    original
        .push_on_layer(
            Entity::Circle(Circle3::new(point(20.0, 0.0), 5.0).unwrap()),
            "DETAIL",
        )
        .unwrap();
    original.set_entity_color(3, 4).unwrap();
    original.push(line(20.0, -10.0, 20.0, 10.0));
    let source = original
        .to_dxf()
        .replacen("0\nLINE\n", "0\nLINE\n5\nABC\n999\nkeep-line\n", 1);
    let mut editor = Editor::new(DxfDocument::parse(&source).unwrap());
    editor.select(vec![0]);
    assert!(
        editor
            .trim_entity(0, &[0, 1, 2], point(5.0, 0.0), BoundaryMode::NoExtend)
            .unwrap()
    );
    assert_eq!(editor.selection(), &[0, 5]);
    for index in [0, 5] {
        assert_eq!(editor.entity_style(index), original.entity_style(0));
        assert_eq!(editor.entity_layer(index), Some("DETAIL"));
    }
    let saved = editor.to_dxf();
    assert_eq!(saved.matches("keep-line").count(), 1);
    assert!(saved.contains("5\nABC\n"));
    assert_eq!(
        DxfDocument::parse(&saved).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert_eq!(editor.selection(), &[0]);
    assert!(!editor.can_undo());
    assert!(
        editor
            .trim_entity(3, &[4], point(25.0, 0.0), BoundaryMode::NoExtend)
            .unwrap()
    );
    assert_eq!(editor.selection(), &[4]);
    assert_eq!(editor.entity_style(4), original.entity_style(3));
    assert_eq!(editor.entity_layer(4), Some("DETAIL"));
    let converted = editor.to_dxf();
    assert_eq!(
        DxfDocument::parse(&converted).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), converted);
    assert!(editor.undo());
    assert!(
        editor
            .trim_entity(0, &[1], point(10.0, 0.0), BoundaryMode::NoExtend)
            .unwrap()
    );
    assert!(
        editor
            .extend_entity(0, &[2], point(3.0, 0.0), BoundaryMode::NoExtend)
            .unwrap()
    );
    endpoints(&editor.entities()[0], point(0.0, 0.0), point(7.0, 0.0));
    assert!(editor.undo());
    endpoints(&editor.entities()[0], point(0.0, 0.0), point(3.0, 0.0));
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    for visible in [false, true] {
        let mut protected = DxfDocument::parse(&source).unwrap();
        if visible {
            protected.set_layer_locked("DETAIL", true).unwrap();
        } else {
            protected.set_layer_visible("DETAIL", false).unwrap();
        }
        let mut editor = Editor::new(protected);
        let before = editor.to_dxf();
        assert!(
            editor
                .trim_entity(0, &[1, 2], point(5.0, 0.0), BoundaryMode::NoExtend)
                .is_err()
        );
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
    }
    let mut editor = Editor::new(DxfDocument::parse(&source).unwrap());
    assert!(
        editor
            .trim_entity(0, &[1, 2], point(3.0, 0.0), BoundaryMode::NoExtend)
            .is_err()
    );
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
}
