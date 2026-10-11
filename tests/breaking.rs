use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, GeometryError, Line3, Point3, Polyline2,
};
use std::f64::consts::{FRAC_PI_2, PI};

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 7.0).unwrap()
}
fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Entity {
    Entity::Line(Line3::new(p(x1, y1), p(x2, y2)))
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
fn near_point(a: Point3, b: Point3) {
    near(a.x(), b.x());
    near(a.y(), b.y());
    assert_eq!(a.z(), b.z());
}
fn arc(entity: &Entity) -> Arc3 {
    let Entity::Arc(arc) = entity else {
        panic!("expected arc")
    };
    *arc
}

fn near_lines(actual: &[Entity], expected: &[Entity]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        let (Entity::Line(actual), Entity::Line(expected)) = (actual, expected) else {
            panic!("expected lines");
        };
        near_point(actual.start(), expected.start());
        near_point(actual.end(), expected.end());
    }
}

#[test]
fn break_lines_project_sort_preserve_direction_and_split_or_remove_ends() {
    let source = line(0.0, 0.0, 10.0, 0.0);
    let parts = source
        .break_xy(p(7.0, 30.0), Point3::new(3.0, -9.0, -100.0).unwrap())
        .unwrap();
    assert_eq!(parts, [line(0.0, 0.0, 3.0, 0.0), line(7.0, 0.0, 10.0, 0.0)]);
    near_lines(
        &line(10.0, 0.0, 0.0, 0.0)
            .break_xy(p(3.0, 2.0), p(7.0, 2.0))
            .unwrap(),
        &[line(10.0, 0.0, 7.0, 0.0), line(3.0, 0.0, 0.0, 0.0)],
    );
    assert_eq!(
        source.break_at_xy(p(4.0, -20.0)).unwrap(),
        [line(0.0, 0.0, 4.0, 0.0), line(4.0, 0.0, 10.0, 0.0)]
    );
    assert_eq!(
        source.break_xy(p(4.0, 2.0), p(4.0, -2.0)).unwrap(),
        source.break_at_xy(p(4.0, 0.0)).unwrap()
    );
    assert_eq!(
        source.break_xy(p(4.0, 2.0), p(20.0, 2.0)).unwrap(),
        [line(0.0, 0.0, 4.0, 0.0)]
    );
    assert_eq!(
        source.break_xy(p(-20.0, 2.0), p(4.0, 2.0)).unwrap(),
        [line(4.0, 0.0, 10.0, 0.0)]
    );
    assert!(
        source
            .break_xy(p(-20.0, 2.0), p(20.0, 2.0))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        source.break_at_xy(p(0.0, 4.0)),
        Err(GeometryError::InvalidBreakPoints)
    );
    assert_eq!(
        source.break_at_xy(p(30.0, 4.0)),
        Err(GeometryError::InvalidBreakPoints)
    );
    let diagonal = line(10.0, 10.0, 0.0, 0.0);
    let split = diagonal.break_at_xy(p(7.0, 3.0)).unwrap();
    near_lines(
        &split,
        &[line(10.0, 10.0, 5.0, 5.0), line(5.0, 5.0, 0.0, 0.0)],
    );
}

#[test]
fn break_arcs_project_to_nearest_extent_and_circles_remove_ccw_gap() {
    let source = Entity::Arc(Arc3::new(p(0.0, 0.0), 10.0, 0.0, FRAC_PI_2).unwrap());
    let parts = source.break_at_xy(p(1.0, 1.0)).unwrap();
    assert_eq!(parts.len(), 2);
    near(arc(&parts[0]).sweep(), PI / 4.0);
    near(arc(&parts[1]).sweep(), PI / 4.0);
    near_point(
        arc(&parts[0]).end_point().unwrap(),
        p(5.0 * 2.0_f64.sqrt(), 5.0 * 2.0_f64.sqrt()),
    );
    near_point(
        arc(&parts[0]).end_point().unwrap(),
        arc(&parts[1]).start_point().unwrap(),
    );
    let shortened = source.break_xy(p(1.0, 1.0), p(-1.0, 11.0)).unwrap();
    assert_eq!(shortened.len(), 1);
    near(arc(&shortened[0]).sweep(), PI / 4.0);
    let shortened = source.break_xy(p(11.0, -1.0), p(1.0, 1.0)).unwrap();
    assert_eq!(shortened.len(), 1);
    near(arc(&shortened[0]).start_angle(), PI / 4.0);
    assert_eq!(
        source.break_at_xy(p(-1.0, -1.0)),
        Err(GeometryError::AmbiguousCurvePick)
    );
    let wrapped = Entity::Arc(Arc3::new(p(0.0, 0.0), 10.0, 3.0 * FRAC_PI_2, FRAC_PI_2).unwrap());
    let parts = wrapped.break_xy(p(1.0, -1.0), p(1.0, 1.0)).unwrap();
    assert_eq!(parts.len(), 2);
    near(arc(&parts[0]).sweep(), PI / 4.0);
    near(arc(&parts[1]).sweep(), PI / 4.0);
    let circle = Entity::Circle(Circle3::new(p(0.0, 0.0), 10.0).unwrap());
    let remaining = circle.break_xy(p(20.0, 0.0), p(0.0, 2.0)).unwrap();
    assert_eq!(remaining.len(), 1);
    near(arc(&remaining[0]).start_angle(), FRAC_PI_2);
    near(arc(&remaining[0]).sweep(), 3.0 * FRAC_PI_2);
    let remaining = circle.break_xy(p(0.0, 2.0), p(20.0, 0.0)).unwrap();
    near(arc(&remaining[0]).start_angle(), 0.0);
    near(arc(&remaining[0]).sweep(), FRAC_PI_2);
    assert_eq!(
        circle.break_xy(p(2.0, 0.0), p(20.0, 0.0)),
        Err(GeometryError::InvalidBreakPoints)
    );
    assert_eq!(
        circle.break_at_xy(p(10.0, 0.0)),
        Err(GeometryError::UnsupportedBreakAtPoint)
    );
}

#[test]
fn break_rejects_degenerate_unsupported_ambiguous_and_unrepresentable_geometry() {
    for source in [
        line(1.0, 1.0, 1.0, 1.0),
        Entity::Circle(Circle3::new(p(0.0, 0.0), 0.0).unwrap()),
        Entity::Arc(Arc3::new(p(0.0, 0.0), 10.0, 0.0, 0.0).unwrap()),
    ] {
        assert_eq!(
            source.break_xy(p(2.0, 0.0), p(0.0, 2.0)),
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
            source.break_xy(p(2.0, 0.0), p(8.0, 0.0)),
            Err(GeometryError::UnsupportedBreakEntity)
        );
        assert_eq!(
            source.break_at_xy(p(5.0, 0.0)),
            Err(GeometryError::UnsupportedBreakAtPoint)
        );
    }
    let circle = Entity::Circle(Circle3::new(p(0.0, 0.0), 10.0).unwrap());
    assert_eq!(
        circle.break_xy(p(0.0, 0.0), p(10.0, 0.0)),
        Err(GeometryError::AmbiguousCurvePick)
    );
    assert_eq!(
        line(-1e308, 0.0, 1e308, 0.0).break_at_xy(p(0.0, 0.0)),
        Err(GeometryError::NonFiniteResult)
    );
    let tiny = Entity::Circle(Circle3::new(p(1e16, 1e16), 0.1).unwrap());
    assert_eq!(
        tiny.break_xy(p(1e16 + 2.0, 1e16), p(1e16, 1e16 + 2.0)),
        Err(GeometryError::UnrepresentableBreak)
    );
}

#[test]
fn editor_break_preserves_metadata_is_atomic_and_roundtrips_converted_circles() {
    let mut document = DxfDocument::default();
    document.push(line(0.0, 0.0, 10.0, 0.0));
    document.push(Entity::Circle(Circle3::new(p(20.0, 0.0), 5.0).unwrap()));
    document.add_layer("DETAIL").unwrap();
    for index in 0..2 {
        document.set_entity_layer(index, "DETAIL").unwrap();
        document.set_entity_color(index, 3).unwrap();
        document.set_entity_linetype(index, "CONTINUOUS").unwrap();
        document.set_entity_linetype_scale(index, 2.0).unwrap();
    }
    let style = document.entity_style(0).unwrap().clone();
    let source = document
        .to_dxf()
        .replacen("0\nLINE\n", "0\nLINE\n5\nAB\n999\nkeep-break\n", 1);
    let document = DxfDocument::parse(&source).unwrap();
    let mut editor = Editor::new(document.clone());
    assert!(!editor.break_entity(99, p(3.0, 0.0), p(7.0, 0.0)).unwrap());
    assert!(editor.break_entity_at(0, p(0.0, 0.0)).is_err());
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
    assert!(editor.break_entity(0, p(3.0, 0.0), p(7.0, 0.0)).unwrap());
    assert_eq!(editor.selection(), [0, 2]);
    for index in [0, 2] {
        assert_eq!(editor.entity_layer(index), Some("DETAIL"));
        assert_eq!(editor.entity_style(index), Some(&style));
    }
    let split = editor.to_dxf();
    assert!(split.contains("5\nAB\n"));
    assert!(split.contains("keep-break"));
    let reloaded = DxfDocument::parse(&split).unwrap();
    assert_eq!(reloaded.entities(), editor.entities());
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), split);
    editor.undo();
    assert!(editor.break_entity(1, p(25.0, 0.0), p(20.0, 5.0)).unwrap());
    assert_eq!(editor.selection(), [1]);
    assert_eq!(editor.entity_layer(1), Some("DETAIL"));
    assert_eq!(editor.entity_style(1), Some(&style));
    near(arc(&editor.entities()[1]).sweep(), 3.0 * FRAC_PI_2);
    assert_eq!(
        DxfDocument::parse(&editor.to_dxf()).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(editor.break_entity(0, p(-5.0, 0.0), p(15.0, 0.0)).unwrap());
    assert_eq!(editor.entities().len(), 1);
    assert!(editor.selection().is_empty());
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    for visible in [true, false] {
        let mut protected = document.clone();
        if visible {
            protected.set_layer_locked("DETAIL", true).unwrap();
        } else {
            protected.set_layer_visible("DETAIL", false).unwrap();
        }
        let mut editor = Editor::new(protected);
        let before = editor.to_dxf();
        assert!(editor.break_entity(0, p(3.0, 0.0), p(7.0, 0.0)).is_err());
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
    }
}
