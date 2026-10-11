use opencad::{Arc3, Circle3, DxfDocument, Editor, Entity, Line3, Point3, Polyline2, Transform};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn point(x: f64, y: f64, z: f64) -> Point3 {
    Point3::new(x, y, z).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
}

fn near_point(actual: Point3, expected: Point3) {
    near(actual.x(), expected.x());
    near(actual.y(), expected.y());
    near(actual.z(), expected.z());
}

// Translation tests cannot reveal rotated arc parameters or a wrong rotation
// center. These expectations use independently calculated quarter-turn points.
#[test]
fn rotation_about_an_offset_base_preserves_z_and_curves() {
    let transform = Transform::rotate_xy(point(1.0, 2.0, 3.0), FRAC_PI_2).unwrap();
    let Entity::Line(line) = Entity::Line(Line3::new(point(4.0, 2.0, 8.0), point(1.0, 5.0, -2.0)))
        .transformed(transform)
        .unwrap()
    else {
        panic!("expected line")
    };
    near_point(line.start(), point(1.0, 5.0, 8.0));
    near_point(line.end(), point(-2.0, 2.0, -2.0));
    assert_eq!(line.start(), point(1.0, 5.0, 8.0));
    assert_eq!(line.end(), point(-2.0, 2.0, -2.0));

    let Entity::Circle(circle) = Entity::Circle(Circle3::new(point(4.0, 2.0, 9.0), 2.0).unwrap())
        .transformed(transform)
        .unwrap()
    else {
        panic!("expected circle")
    };
    near_point(circle.center(), point(1.0, 5.0, 9.0));
    near(circle.radius(), 2.0);

    let Entity::Arc(arc) =
        Entity::Arc(Arc3::new(point(4.0, 2.0, 9.0), 2.0, 0.0, FRAC_PI_2).unwrap())
            .transformed(transform)
            .unwrap()
    else {
        panic!("expected arc")
    };
    near_point(arc.start_point().unwrap(), point(1.0, 7.0, 9.0));
    near_point(arc.end_point().unwrap(), point(-1.0, 5.0, 9.0));
    near(arc.sweep(), FRAC_PI_2);

    for closed in [false, true] {
        let source = Polyline2::with_bulges(
            vec![
                point(4.0, 2.0, 9.0),
                point(1.0, 5.0, 9.0),
                point(1.0, 2.0, 9.0),
            ],
            vec![0.5, -1.0, 2.0],
            closed,
        )
        .unwrap();
        let Entity::Polyline(rotated) = Entity::Polyline(source.clone())
            .transformed(transform)
            .unwrap()
        else {
            panic!("expected polyline")
        };
        for (actual, expected) in rotated.vertices().iter().zip([
            point(1.0, 5.0, 9.0),
            point(-2.0, 2.0, 9.0),
            point(1.0, 2.0, 9.0),
        ]) {
            near_point(*actual, expected);
        }
        assert_eq!(rotated.bulges(), source.bulges());
        assert_eq!(rotated.is_closed(), closed);
        near(rotated.length().unwrap(), source.length().unwrap());
    }
}

// Scaling must change radius, elevation and curved path length together.
#[test]
fn uniform_scale_preserves_bulges_and_scales_xyz_about_the_base() {
    let transform = Transform::scale(point(1.0, 2.0, 3.0), 2.0).unwrap();
    let source = Polyline2::with_bulges(
        vec![point(1.0, 2.0, 7.0), point(11.0, 2.0, 7.0)],
        vec![1.0, -0.5],
        false,
    )
    .unwrap();
    let Entity::Polyline(scaled) = Entity::Polyline(source.clone())
        .transformed(transform)
        .unwrap()
    else {
        panic!("expected polyline")
    };
    near_point(scaled.vertices()[0], point(1.0, 2.0, 11.0));
    near_point(scaled.vertices()[1], point(21.0, 2.0, 11.0));
    near_point(
        scaled
            .segments()
            .next()
            .unwrap()
            .unwrap()
            .point_at_fraction(0.5)
            .unwrap(),
        point(11.0, -8.0, 11.0),
    );
    near(scaled.length().unwrap(), 2.0 * source.length().unwrap());
    assert_eq!(scaled.bulges(), source.bulges());

    let Entity::Circle(circle) = Entity::Circle(Circle3::new(point(4.0, 6.0, 7.0), 3.0).unwrap())
        .transformed(transform)
        .unwrap()
    else {
        panic!("expected circle")
    };
    near_point(circle.center(), point(7.0, 10.0, 11.0));
    near(circle.radius(), 6.0);
    let Entity::Arc(arc) =
        Entity::Arc(Arc3::new(point(4.0, 6.0, 7.0), 3.0, PI, FRAC_PI_2).unwrap())
            .transformed(transform)
            .unwrap()
    else {
        panic!("expected arc")
    };
    near(arc.radius(), 6.0);
    near(arc.sweep(), 3.0 * FRAC_PI_2);
    near_point(arc.start_point().unwrap(), point(1.0, 10.0, 11.0));
}

// A reflected arc must retain its locus, not become the complementary sweep.
// The polyline checks inspect the actual curve, including its inactive bulge.
#[test]
fn mirrors_reflect_endpoints_and_reverse_arc_and_bulge_orientation() {
    for (second, expected) in [
        (point(4.0, 2.0, 8.0), point(3.0, -1.0, 7.0)),
        (point(1.0, 5.0, 8.0), point(-1.0, 5.0, 7.0)),
        (point(4.0, 5.0, 8.0), point(4.0, 4.0, 7.0)),
    ] {
        let mirror = Transform::mirror_xy(point(1.0, 2.0, -8.0), second).unwrap();
        let Entity::Line(line) =
            Entity::Line(Line3::new(point(3.0, 5.0, 7.0), point(1.0, 2.0, 4.0)))
                .transformed(mirror)
                .unwrap()
        else {
            panic!("expected line")
        };
        near_point(line.start(), expected);
        near_point(line.end(), point(1.0, 2.0, 4.0));
    }
    let mirror = Transform::mirror_xy(point(0.0, 2.0, 0.0), point(10.0, 2.0, 0.0)).unwrap();
    let source = Arc3::new(point(3.0, 4.0, 7.0), 2.0, 3.0 * FRAC_PI_2, FRAC_PI_2).unwrap();
    let Entity::Arc(reflected) = Entity::Arc(source).transformed(mirror).unwrap() else {
        panic!("expected arc")
    };
    near(reflected.sweep(), PI);
    near_point(reflected.center(), point(3.0, 0.0, 7.0));
    near_point(reflected.start_point().unwrap(), point(3.0, -2.0, 7.0));
    near_point(reflected.end_point().unwrap(), point(3.0, 2.0, 7.0));
    let source = Polyline2::with_bulges(
        vec![point(0.0, 2.0, 7.0), point(10.0, 2.0, 7.0)],
        vec![1.0, -0.5],
        false,
    )
    .unwrap();
    let Entity::Polyline(reflected) = Entity::Polyline(source.clone())
        .transformed(mirror)
        .unwrap()
    else {
        panic!("expected polyline")
    };
    assert_eq!(reflected.bulges(), &[-1.0, 0.5]);
    near_point(
        reflected
            .segments()
            .next()
            .unwrap()
            .unwrap()
            .point_at_fraction(0.5)
            .unwrap(),
        point(5.0, 7.0, 7.0),
    );
    assert_eq!(
        Entity::Polyline(reflected).transformed(mirror).unwrap(),
        Entity::Polyline(source)
    );
}

// Existing MOVE tests don't cover radius overflow, invalid factors or transform
// identities. Failed selections must leave the exact source and history intact.
#[test]
fn invalid_and_overflowing_transforms_do_not_partially_mutate_or_create_history() {
    let origin = point(0.0, 0.0, 0.0);
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(Transform::scale(origin, factor).is_err());
    }
    assert!(Transform::rotate_xy(origin, f64::NAN).is_err());
    assert!(Transform::mirror_xy(origin, point(0.0, 0.0, 9.0)).is_err());
    let mut document = DxfDocument::default();
    document.push(Entity::Line(Line3::new(origin, point(1.0, 2.0, 3.0))));
    document.push(Entity::Circle(Circle3::new(origin, f64::MAX).unwrap()));
    let mut editor = Editor::new(document);
    editor.select(vec![0, 1]);
    let saved = editor.to_dxf();
    for copy in [false, true] {
        assert!(
            editor
                .transform_selection(Transform::scale(origin, 2.0).unwrap(), copy)
                .is_err()
        );
        assert_eq!(editor.to_dxf(), saved);
        assert_eq!(editor.selection(), &[0, 1]);
        assert!(!editor.can_undo());
        assert!(!editor.is_dirty());
    }
    for identity in [
        Transform::rotate_xy(origin, TAU).unwrap(),
        Transform::scale(origin, 1.0).unwrap(),
    ] {
        assert!(!editor.transform_selection(identity, false).unwrap());
        assert_eq!(editor.to_dxf(), saved);
        assert!(!editor.can_undo());
    }
}

// Record association and style arrays are separate from geometry. Check real
// imported records through edits, copies, undo/redo and a saved DXF reparse.
#[test]
fn transformed_copies_keep_layers_styles_and_undo_as_one_transaction() {
    let source = include_str!("fixtures/entity-styles.dxf");
    let original_document = DxfDocument::parse(source).unwrap();
    let mut editor = Editor::new(original_document.clone());
    editor.select(vec![4, 6]);
    let originals: Vec<_> = editor.entities().to_vec();
    let styles = [
        editor.entity_style(4).unwrap().clone(),
        editor.entity_style(6).unwrap().clone(),
    ];
    let layers = [
        editor.entity_layer(4).unwrap().to_owned(),
        editor.entity_layer(6).unwrap().to_owned(),
    ];
    let mirror = Transform::mirror_xy(point(0.0, 0.0, 0.0), point(0.0, 10.0, 0.0)).unwrap();
    assert!(editor.transform_selection(mirror, true).unwrap());
    assert_eq!(&editor.entities()[..7], originals);
    assert_eq!(editor.selection(), &[7, 8]);
    for (copy, original) in [(7, 4), (8, 6)] {
        assert_eq!(editor.entity_layer(copy), editor.entity_layer(original));
        assert_eq!(editor.entity_style(copy), editor.entity_style(original));
    }
    let saved = editor.to_dxf();
    let reloaded = DxfDocument::parse(&saved).unwrap();
    assert_eq!(reloaded.entities(), editor.entities());
    for (offset, index) in [7, 8].into_iter().enumerate() {
        assert_eq!(reloaded.entity_style(index), Some(&styles[offset]));
        assert_eq!(reloaded.entity_layer(index), Some(layers[offset].as_str()));
    }
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert_eq!(editor.selection(), &[4, 6]);
    assert!(!editor.is_dirty());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), saved);
    assert_eq!(editor.selection(), &[7, 8]);
    editor.set_layer_locked(&layers[0], true).unwrap();
    let locked = editor.to_dxf();
    assert!(
        editor
            .transform_selection(Transform::scale(point(0.0, 0.0, 0.0), 2.0).unwrap(), false)
            .is_err()
    );
    assert_eq!(editor.to_dxf(), locked);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), saved);

    let mut editor = Editor::new(DxfDocument::parse(source).unwrap());
    editor.select(vec![4]);
    assert!(
        editor
            .transform_selection(Transform::scale(point(0.0, 0.0, 0.0), 2.0).unwrap(), false)
            .unwrap()
    );
    let edited = DxfDocument::parse(&editor.to_dxf()).unwrap();
    for (index, original) in originals.iter().enumerate() {
        if index != 4 {
            assert_eq!(&edited.entities()[index], original);
        }
        assert_eq!(
            edited.entity_style(index),
            original_document.entity_style(index)
        );
    }
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
}
