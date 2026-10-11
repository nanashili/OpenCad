use opencad::{
    Corner, CornerParameter, DxfDocument, Editor, Entity, EntityStyle, GeometryError, Line3, Point3,
};
use std::f64::consts::{FRAC_PI_2, PI};

fn point(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 7.0).unwrap()
}
fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Line3 {
    Line3::new(point(x1, y1), point(x2, y2))
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1.0e-9, "{a} != {b}");
}
fn near_point(a: Point3, b: Point3) {
    near(a.x(), b.x());
    near(a.y(), b.y());
    assert_eq!(a.z(), b.z());
}
fn near_line(a: Line3, b: Line3) {
    near_point(a.start(), b.start());
    near_point(a.end(), b.end());
}
fn connector_line(entity: Option<&Entity>) -> Line3 {
    let Some(Entity::Line(line)) = entity else {
        panic!("expected chamfer line")
    };
    *line
}

#[test]
fn chamfer_angle_respects_first_pick_distance_and_relative_angle() {
    for (x, y) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        for reversed in [false, true] {
            let a = line(-20.0, 0.0, 20.0, 0.0);
            let b = line(0.0, 20.0, 0.0, -20.0);
            let (a, b) = if reversed {
                (
                    Line3::new(a.end(), a.start()),
                    Line3::new(b.end(), b.start()),
                )
            } else {
                (a, b)
            };
            let result = Corner::chamfer_angle(3.0, PI / 6.0)
                .unwrap()
                .between_lines(a, point(x * 10.0, 0.0), b, point(0.0, y * 10.0))
                .unwrap();
            let connector = connector_line(result.connector());
            near_point(connector.start(), point(x * 3.0, 0.0));
            near_point(connector.end(), point(0.0, y * 3.0_f64.sqrt()));
            let dx = connector.end().x() - connector.start().x();
            let dy = connector.end().y() - connector.start().y();
            near(((-x * dy).abs()).atan2(-x * dx), PI / 6.0);
            for (trimmed, source) in [(result.first(), a), (result.second(), b)] {
                let dx = trimmed.end().x() - trimmed.start().x();
                let dy = trimmed.end().y() - trimmed.start().y();
                assert!(
                    dx * (source.end().x() - source.start().x())
                        + dy * (source.end().y() - source.start().y())
                        > 0.0
                );
            }
        }
    }
    let result = Corner::chamfer_angle(3.0, PI / 6.0)
        .unwrap()
        .between_lines(
            line(0.0, 20.0, 0.0, 6.0),
            point(0.0, 10.0),
            line(5.0, 0.0, 20.0, 0.0),
            point(10.0, 0.0),
        )
        .unwrap();
    let connector = connector_line(result.connector());
    near_point(connector.start(), point(0.0, 3.0));
    near_point(connector.end(), point(3.0_f64.sqrt(), 0.0));
}

#[test]
fn chamfer_angle_handles_oblique_lines_zero_values_and_numeric_scales() {
    // A 30 degree wedge admits a 120 degree bevel angle from the first line.
    let result = Corner::chamfer_angle(2.0, 2.0 * PI / 3.0)
        .unwrap()
        .between_lines(
            line(5.0, 0.0, 20.0, 0.0),
            point(10.0, 0.0),
            line(0.0, 0.0, 10.0 * 3.0_f64.sqrt(), 10.0),
            point(5.0 * 3.0_f64.sqrt(), 5.0),
        )
        .unwrap();
    let connector = connector_line(result.connector());
    near_point(connector.start(), point(2.0, 0.0));
    near_point(connector.end(), point(3.0, 3.0_f64.sqrt()));
    for (distance, angle) in [(0.0, 0.0), (0.0, PI / 4.0), (2.0, 0.0)] {
        let result = Corner::chamfer_angle(distance, angle)
            .unwrap()
            .between_lines(
                line(5.0, 0.0, 20.0, 0.0),
                point(10.0, 0.0),
                line(0.0, 20.0, 0.0, 6.0),
                point(0.0, 10.0),
            )
            .unwrap();
        near_point(result.first().start(), point(distance, 0.0));
        near_point(result.second().end(), point(0.0, 0.0));
        assert_eq!(result.connector().is_none(), distance == 0.0);
    }
    for scale in [1.0e-150, 1.0, 1.0e150] {
        let result = Corner::chamfer_angle(3.0 * scale, PI / 4.0)
            .unwrap()
            .between_lines(
                line(0.0, 0.0, 20.0 * scale, 0.0),
                point(10.0 * scale, 0.0),
                line(0.0, 20.0 * scale, 0.0, 0.0),
                point(0.0, 10.0 * scale),
            )
            .unwrap();
        let connector = connector_line(result.connector());
        near(connector.start().x() / scale, 3.0);
        near(connector.end().y() / scale, 3.0);
    }
}

#[test]
fn chamfer_angle_rejects_invalid_values_and_impossible_picked_sides() {
    for angle in [-1.0, PI, f64::NAN, f64::INFINITY] {
        assert_eq!(
            Corner::chamfer_angle(2.0, angle).unwrap_err(),
            GeometryError::InvalidChamferAngle
        );
    }
    assert_eq!(
        Corner::chamfer_angle(-1.0, PI / 4.0).unwrap_err(),
        GeometryError::InvalidCornerDimension
    );
    for angle in [FRAC_PI_2, 2.0 * PI / 3.0] {
        assert_eq!(
            Corner::chamfer_angle(2.0, angle)
                .unwrap()
                .between_lines(
                    line(5.0, 0.0, 20.0, 0.0),
                    point(10.0, 0.0),
                    line(0.0, 20.0, 0.0, 6.0),
                    point(0.0, 10.0),
                )
                .unwrap_err(),
            GeometryError::IncompatibleChamferAngle
        );
    }
    assert_eq!(
        Corner::chamfer_angle(20.0, PI / 4.0)
            .unwrap()
            .between_lines(
                line(5.0, 0.0, 20.0, 0.0),
                point(10.0, 0.0),
                line(0.0, 20.0, 0.0, 6.0),
                point(0.0, 10.0),
            )
            .unwrap_err(),
        GeometryError::CornerOutsideSources
    );
    let origin = 1.0e16;
    assert_eq!(
        Corner::chamfer_angle(0.9, 166.0_f64.to_radians())
            .unwrap()
            .between_lines(
                line(origin, origin, origin + 1000.0, origin),
                point(origin + 500.0, origin),
                line(origin, origin, origin + 984.0, origin + 174.0),
                point(origin + 984.0, origin + 174.0),
            )
            .unwrap_err(),
        GeometryError::UnrepresentableCorner
    );
    assert_eq!(
        Corner::chamfer_angle(f64::MIN_POSITIVE, f64::from_bits(1))
            .unwrap()
            .between_lines(
                line(0.0, 0.0, 20.0, 0.0),
                point(10.0, 0.0),
                line(0.0, 20.0, 0.0, 0.0),
                point(0.0, 10.0),
            )
            .unwrap_err(),
        GeometryError::UnrepresentableCorner
    );
}

#[test]
fn fillet_is_tangent_in_each_quadrant_and_preserves_source_order() {
    for (x, y) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
        let a = line(-20.0, 0.0, 20.0, 0.0);
        let b = line(0.0, 20.0, 0.0, -20.0);
        let result = Corner::fillet(3.0)
            .unwrap()
            .between_lines(a, point(x * 10.0, 0.0), b, point(0.0, y * 10.0))
            .unwrap();
        let Entity::Arc(arc) = result.connector().unwrap() else {
            panic!("expected arc")
        };
        near_point(arc.center(), point(x * 3.0, y * 3.0));
        near(arc.radius(), 3.0);
        near(arc.sweep(), FRAC_PI_2);
        let endpoints = [arc.start_point().unwrap(), arc.end_point().unwrap()];
        for expected in [point(x * 3.0, 0.0), point(0.0, y * 3.0)] {
            assert!(
                endpoints
                    .iter()
                    .any(|p| (p.x() - expected.x()).hypot(p.y() - expected.y()) < 1e-9)
            );
        }
        if x > 0.0 {
            assert_eq!(result.first().end(), a.end());
            near_point(result.first().start(), point(3.0, 0.0));
        } else {
            assert_eq!(result.first().start(), a.start());
            near_point(result.first().end(), point(-3.0, 0.0));
        }
        if y > 0.0 {
            assert_eq!(result.second().start(), b.start());
            near_point(result.second().end(), point(0.0, 3.0));
        } else {
            assert_eq!(result.second().end(), b.end());
            near_point(result.second().start(), point(0.0, -3.0));
        }
    }
    // 60 degree wedge: tangent distance sqrt(3)*r, 120 degree arc.
    let result = Corner::fillet(2.0)
        .unwrap()
        .between_lines(
            line(5.0, 0.0, 20.0, 0.0),
            point(10.0, 0.0),
            line(10.0, 10.0 * 3.0_f64.sqrt(), 0.0, 0.0),
            point(5.0, 5.0 * 3.0_f64.sqrt()),
        )
        .unwrap();
    near(result.first().start().x(), 2.0 * 3.0_f64.sqrt());
    let Entity::Arc(arc) = result.connector().unwrap() else {
        panic!("expected arc")
    };
    near(arc.sweep(), 2.0 * PI / 3.0);
    near_point(arc.center(), point(2.0 * 3.0_f64.sqrt(), 2.0));
}

#[test]
fn chamfer_distances_follow_pick_order_and_zero_builds_a_sharp_intersection() {
    let a = line(5.0, 0.0, 20.0, 0.0);
    let b = line(0.0, 20.0, 0.0, 6.0);
    let result = Corner::chamfer(2.0, 4.0)
        .unwrap()
        .between_lines(a, point(10.0, 0.0), b, point(0.0, 10.0))
        .unwrap();
    near_line(result.first(), line(2.0, 0.0, 20.0, 0.0));
    near_line(result.second(), line(0.0, 20.0, 0.0, 4.0));
    near_line(connector_line(result.connector()), line(2.0, 0.0, 0.0, 4.0));
    for corner in [
        Corner::fillet(0.0).unwrap(),
        Corner::chamfer(0.0, 0.0).unwrap(),
    ] {
        let sharp = corner
            .between_lines(a, point(10.0, 0.0), b, point(0.0, 10.0))
            .unwrap();
        near_line(sharp.first(), line(0.0, 0.0, 20.0, 0.0));
        near_line(sharp.second(), line(0.0, 20.0, 0.0, 0.0));
        assert!(sharp.connector().is_none());
    }
    let one_zero = Corner::chamfer(0.0, 4.0)
        .unwrap()
        .between_lines(a, point(10.0, 0.0), b, point(0.0, 10.0))
        .unwrap();
    near_line(
        connector_line(one_zero.connector()),
        line(0.0, 0.0, 0.0, 4.0),
    );
}

#[test]
fn invalid_ambiguous_degenerate_or_unrepresentable_corners_are_rejected() {
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            Corner::fillet(value),
            Err(GeometryError::InvalidCornerDimension)
        ));
        assert!(Corner::chamfer(1.0, value).is_err());
        assert!(Corner::chamfer(value, 1.0).is_err());
    }
    let a = line(0.0, 0.0, 10.0, 0.0);
    let b = line(0.0, 0.0, 0.0, 10.0);
    let radius = Corner::fillet(2.0).unwrap();
    for other in [
        a,
        line(0.0, 2.0, 10.0, 2.0),
        line(0.0, 2.0, 10.0, 2.0 + 1e-13),
    ] {
        assert_eq!(
            radius.between_lines(a, point(5.0, 0.0), other, point(5.0, 2.0)),
            Err(GeometryError::ParallelCorner)
        );
    }
    assert_eq!(
        radius.between_lines(a, point(0.0, 0.0), b, point(0.0, 5.0)),
        Err(GeometryError::AmbiguousCornerPick)
    );
    assert_eq!(
        radius.between_lines(a, point(-5.0, 0.0), b, point(0.0, 5.0)),
        Err(GeometryError::CornerOutsideSources)
    );
    assert_eq!(
        radius.between_lines(
            a,
            point(5.0, 0.0),
            line(0.0, 0.0, 0.0, 0.0),
            point(0.0, 5.0)
        ),
        Err(GeometryError::DegenerateCurve)
    );
    let elevated = Line3::new(
        Point3::new(0.0, 0.0, 8.0).unwrap(),
        Point3::new(0.0, 10.0, 8.0).unwrap(),
    );
    assert_eq!(
        radius.between_lines(a, point(5.0, 0.0), elevated, point(0.0, 5.0)),
        Err(GeometryError::UnsupportedCornerEntities)
    );
    assert_eq!(
        Corner::chamfer(10.0, 2.0)
            .unwrap()
            .between_lines(a, point(5.0, 0.0), b, point(0.0, 5.0)),
        Err(GeometryError::CornerOutsideSources)
    );
    assert!(
        Corner::fillet(f64::MAX)
            .unwrap()
            .between_lines(a, point(5.0, 0.0), b, point(0.0, 5.0))
            .is_err()
    );
    let origin = 1.0e16;
    assert_eq!(
        Corner::fillet(0.01).unwrap().between_lines(
            line(origin, origin, origin + 100.0, origin),
            point(origin + 50.0, origin),
            line(origin, origin, origin, origin + 100.0),
            point(origin, origin + 50.0)
        ),
        Err(GeometryError::UnrepresentableCorner)
    );
}

#[test]
fn editor_corners_are_atomic_preserve_records_and_obey_layer_and_trim_rules() {
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document
        .push_on_layer(Entity::Line(line(0.0, 0.0, 20.0, 0.0)), "DETAIL")
        .unwrap();
    document
        .push_on_layer(Entity::Line(line(0.0, 20.0, 0.0, 0.0)), "DETAIL")
        .unwrap();
    document.set_entity_color(0, 3).unwrap();
    document.set_entity_linetype_scale(1, 2.5).unwrap();
    let source = document
        .to_dxf()
        .replacen("0\nLINE\n", "0\nLINE\n5\nABC\n999\nkeep-source\n", 1);
    let picks = [(0, point(10.0, 0.0)), (1, point(0.0, 10.0))];
    let mut editor = Editor::new(DxfDocument::parse(&source).unwrap());
    editor.select(vec![1]);
    assert!(
        editor
            .corner_lines(picks, Corner::fillet(3.0).unwrap(), true)
            .unwrap()
    );
    assert_eq!(editor.selection(), &[0, 1, 2]);
    assert_eq!(editor.entity_style(0), document.entity_style(0));
    assert_eq!(editor.entity_style(1), document.entity_style(1));
    assert_eq!(editor.entity_style(2), Some(&EntityStyle::default()));
    assert_eq!(editor.entity_layer(2), Some("DETAIL"));
    let saved = editor.to_dxf();
    assert!(saved.contains("5\nABC\n"));
    assert_eq!(saved.matches("keep-source").count(), 1);
    assert_eq!(
        DxfDocument::parse(&saved).unwrap().entities(),
        editor.entities()
    );
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert_eq!(editor.selection(), &[1]);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), saved);
    assert!(editor.undo());
    assert!(
        editor
            .corner_lines(picks, Corner::chamfer(2.0, 4.0).unwrap(), false)
            .unwrap()
    );
    assert_eq!(&editor.entities()[..2], document.entities());
    assert!(editor.undo());
    assert!(
        !editor
            .corner_lines(picks, Corner::fillet(0.0).unwrap(), false)
            .unwrap()
    );
    assert!(!editor.can_undo());
    for bad_picks in [[picks[0], picks[0]], [(99, picks[0].1), picks[1]]] {
        assert!(
            editor
                .corner_lines(bad_picks, Corner::fillet(2.0).unwrap(), true)
                .is_err()
        );
        assert_eq!(editor.to_dxf(), source);
        assert!(!editor.can_undo());
    }
    for visible in [true, false] {
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
                .corner_lines(picks, Corner::fillet(2.0).unwrap(), true)
                .is_err()
        );
        assert!(
            editor
                .corner_lines(picks, Corner::fillet(2.0).unwrap(), false)
                .is_err()
        );
        assert_eq!(editor.to_dxf(), before);
        assert!(!editor.can_undo());
    }
    // Different source layers place the new connector on the current layer.
    document.set_entity_layer(1, "0").unwrap();
    document.set_layer_locked("DETAIL", true).unwrap();
    let mut editor = Editor::new(document.clone());
    assert!(
        editor
            .corner_lines(picks, Corner::chamfer(2.0, 4.0).unwrap(), false)
            .unwrap()
    );
    assert_eq!(editor.entity_layer(2), Some("0"));
    assert_eq!(&editor.entities()[..2], document.entities());
}

#[test]
fn drawing_corner_dimensions_preserve_header_text_validate_and_share_new_header() {
    use CornerParameter::{ChamferFirst, ChamferSecond, FilletRadius};
    let minimal = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
    let mut document = DxfDocument::parse(minimal).unwrap();
    assert_eq!(document.corner_value(FilletRadius), 0.0);
    assert!(!document.set_corner_value(FilletRadius, 0.0).unwrap());
    document.set_linetype_scale(2.5).unwrap();
    for (parameter, value) in [
        (FilletRadius, 3.0),
        (ChamferFirst, 4.0),
        (ChamferSecond, 5.0),
    ] {
        document.set_corner_value(parameter, value).unwrap();
    }
    let saved = document.to_dxf();
    assert_eq!(saved.matches("2\nHEADER\n").count(), 1);
    let parsed = DxfDocument::parse(&saved).unwrap();
    assert_eq!(parsed.linetype_scale(), 2.5);
    assert_eq!(parsed.corner_value(ChamferSecond), 5.0);
    let source = saved
        .replace(
            "$FILLETRAD\n40\n3\n",
            "$FILLETRAD\n40\n3.0000\n999\nkeep-corner\n",
        )
        .replace("9\n$CHAMFERA", "9\n$CHAMFERC\n40\n17.25\n9\n$CHAMFERA");
    let mut editor = Editor::new(DxfDocument::parse(&source).unwrap());
    assert_eq!(editor.to_dxf(), source);
    assert!(editor.set_corner_value(FilletRadius, 6.0).unwrap());
    assert!(editor.to_dxf().contains("$CHAMFERC\n40\n17.25\n"));
    assert!(editor.to_dxf().contains("keep-corner"));
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.can_undo());
    assert!(editor.set_corner_value(FilletRadius, -1.0).is_err());
    assert!(!editor.can_undo());
    for variable in [
        "9\n$FILLETRAD\n40\n-1\n",
        "9\n$CHAMFERA\n40\nNaN\n",
        "9\n$CHAMFERB\n70\n1\n",
        "9\n$CHAMFERA\n40\n1\n40\n2\n",
        "9\n$FILLETRAD\n40\n1\n9\n$FILLETRAD\n40\n2\n",
    ] {
        let invalid = format!("0\nSECTION\n2\nHEADER\n{variable}0\nENDSEC\n{minimal}");
        assert!(DxfDocument::parse(&invalid).is_err(), "accepted {variable}");
    }
}
