use opencad::{Arc3, Circle3, DxfDocument, Editor, Entity, Line3, Point3, Polyline2};
use std::f64::consts::{FRAC_PI_2, PI};

fn p(x: f64, y: f64) -> Point3 {
    Point3::new(x, y, 7.0).unwrap()
}
fn line(x1: f64, y1: f64, x2: f64, y2: f64) -> Entity {
    Entity::Line(Line3::new(p(x1, y1), p(x2, y2)))
}
fn near_point(actual: Point3, expected: Point3) {
    assert!((actual.x() - expected.x()).abs() < 1e-9);
    assert!((actual.y() - expected.y()).abs() < 1e-9);
    assert_eq!(actual.z(), expected.z());
}
fn polyline(closed: bool) -> Polyline2 {
    Polyline2::with_bulges(
        vec![p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)],
        vec![1.0, 0.0, -1.0],
        closed,
    )
    .unwrap()
}

#[test]
fn explode_emits_open_closed_and_signed_bulged_centerlines_at_source_elevation() {
    let open = polyline(false).explode().unwrap();
    assert_eq!(open.len(), 2); // The unused last bulge does not create a closing segment.
    let Entity::Arc(arc) = open[0] else {
        panic!("expected arc")
    };
    assert_eq!(arc.center(), p(5.0, 0.0));
    assert_eq!(arc.radius(), 5.0);
    assert_eq!(arc.sweep(), PI);
    near_point(arc.start_point().unwrap(), p(0.0, 0.0));
    near_point(arc.end_point().unwrap(), p(10.0, 0.0));
    near_point(arc.point_at_fraction(0.5).unwrap(), p(5.0, -5.0));
    assert_eq!(open[1], line(10.0, 0.0, 10.0, 10.0));
    let closed = polyline(true).explode().unwrap();
    assert_eq!(closed.len(), 3);
    assert_eq!(&closed[..2], open);
    let Entity::Arc(closing) = closed[2] else {
        panic!("expected closing arc")
    };
    near_point(closing.center(), p(5.0, 5.0));
    near_point(closing.start_point().unwrap(), p(0.0, 0.0));
    near_point(closing.end_point().unwrap(), p(10.0, 10.0));
    for (bulge, center_y, radius, middle_y, sweep) in [
        (-1.0, 0.0, 5.0, 5.0, PI),
        (2.0, -3.75, 6.25, -10.0, 4.0 * 2.0_f64.atan()),
        (-2.0, 3.75, 6.25, 10.0, 4.0 * 2.0_f64.atan()),
    ] {
        let source =
            Polyline2::with_bulges(vec![p(0.0, 0.0), p(10.0, 0.0)], vec![bulge, 0.0], false)
                .unwrap();
        let Entity::Arc(arc) = source.explode().unwrap()[0] else {
            panic!("expected arc")
        };
        near_point(arc.center(), p(5.0, center_y));
        assert!((arc.radius() - radius).abs() < 1e-9);
        assert!((arc.sweep() - sweep).abs() < 1e-9);
        near_point(arc.point_at_fraction(0.5).unwrap(), p(5.0, middle_y));
        let (start, end) = if bulge < 0.0 {
            (p(10.0, 0.0), p(0.0, 0.0))
        } else {
            (p(0.0, 0.0), p(10.0, 0.0))
        };
        near_point(arc.start_point().unwrap(), start);
        near_point(arc.end_point().unwrap(), end);
    }
    let coincident = Polyline2::with_bulges(
        vec![p(0.0, 0.0), p(0.0, 0.0), p(10.0, 0.0)],
        vec![1.0, 0.0, 0.0],
        false,
    )
    .unwrap();
    assert_eq!(
        coincident.explode().unwrap(),
        [line(0.0, 0.0, 0.0, 0.0), line(0.0, 0.0, 10.0, 0.0)]
    );
    let two_closed = Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], true).unwrap();
    assert_eq!(
        two_closed.explode().unwrap(),
        [line(0.0, 0.0, 10.0, 0.0), line(10.0, 0.0, 0.0, 0.0)]
    );
}

#[test]
fn editor_explode_batches_sources_preserves_unrelated_records_and_undoes_once() {
    let mut document = DxfDocument::default();
    document.push(line(-20.0, 0.0, -10.0, 0.0));
    document.push(Entity::Polyline(polyline(false)));
    document.push(Entity::Circle(Circle3::new(p(30.0, 0.0), 5.0).unwrap()));
    document.push(Entity::Polyline(polyline(true)));
    document.push(Entity::Arc(
        Arc3::new(p(50.0, 0.0), 5.0, 0.0, FRAC_PI_2).unwrap(),
    ));
    document.add_layer("DETAIL").unwrap();
    document.add_layer("FRAME").unwrap();
    for (index, layer, color, scale) in [(1, "DETAIL", 3, 2.0), (3, "FRAME", 5, 0.5)] {
        document.set_entity_layer(index, layer).unwrap();
        document.set_entity_color(index, color).unwrap();
        document.set_entity_linetype(index, "CONTINUOUS").unwrap();
        document.set_entity_linetype_scale(index, scale).unwrap();
    }
    let source = document
        .to_dxf()
        .replacen("0\nLINE\n", "0\nLINE\n5\nAB\n999\nkeep-unrelated\n", 1)
        .replacen(
            "0\nPOLYLINE\n",
            "0\nPOLYLINE\n5\nB1\n999\nsource-polyline-only\n",
            1,
        );
    let document = DxfDocument::parse(&source).unwrap();
    let first_style = document.entity_style(1).unwrap().clone();
    let second_style = document.entity_style(3).unwrap().clone();
    let mut editor = Editor::new(document.clone());
    editor.select(vec![0, 2, 4]);
    assert_eq!(editor.explode_selection().unwrap(), 0);
    assert_eq!(editor.selection(), [0, 2, 4]);
    assert!(!editor.can_undo());
    editor.select(vec![3, 0, 1, 3]);
    assert_eq!(editor.explode_selection().unwrap(), 2);
    assert_eq!(editor.entities().len(), 8);
    assert_eq!(editor.selection(), [3, 4, 5, 6, 7]);
    assert_eq!(
        &editor.entities()[..3],
        &[
            document.entities()[0].clone(),
            document.entities()[2].clone(),
            document.entities()[4].clone()
        ]
    );
    assert_eq!(&editor.entities()[3..5], polyline(false).explode().unwrap());
    assert_eq!(&editor.entities()[5..8], polyline(true).explode().unwrap());
    for index in 3..8 {
        let (layer, style) = if index < 5 {
            ("DETAIL", &first_style)
        } else {
            ("FRAME", &second_style)
        };
        assert_eq!(editor.entity_layer(index), Some(layer));
        assert_eq!(editor.entity_style(index), Some(style));
    }
    let saved = editor.to_dxf();
    assert!(saved.contains("5\nAB\n999\nkeep-unrelated\n"));
    assert!(!saved.contains("source-polyline-only"));
    assert!(!saved.contains("5\nB1\n"));
    let reloaded = DxfDocument::parse(&saved).unwrap();
    assert_eq!(reloaded.entities(), editor.entities());
    for index in 0..8 {
        assert_eq!(reloaded.entity_style(index), editor.entity_style(index));
        assert_eq!(reloaded.entity_layer(index), editor.entity_layer(index));
    }
    assert_eq!(editor.explode_selection().unwrap(), 0);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert_eq!(editor.selection(), [0, 1, 3]);
    assert!(!editor.can_undo());
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), saved);
}

#[test]
fn editor_explode_rejects_an_invalid_or_locked_source_before_any_batch_mutation() {
    let valid = Entity::Polyline(polyline(false));
    let bad = Entity::Polyline(
        Polyline2::with_bulges(
            vec![p(0.0, 0.0), p(10.0, 0.0)],
            vec![f64::MIN_POSITIVE, 0.0],
            false,
        )
        .unwrap(),
    );
    for invalid_geometry in [true, false] {
        let mut document = DxfDocument::default();
        document.push(valid.clone());
        document.push(if invalid_geometry {
            bad.clone()
        } else {
            valid.clone()
        });
        if !invalid_geometry {
            document.add_layer("LOCKED").unwrap();
            document.set_entity_layer(1, "LOCKED").unwrap();
            document.set_layer_locked("LOCKED", true).unwrap();
        }
        let mut editor = Editor::new(document);
        editor.select(vec![0, 1]);
        let before = editor.to_dxf();
        assert!(editor.explode_selection().is_err());
        assert_eq!(editor.to_dxf(), before);
        assert_eq!(editor.selection(), [0, 1]);
        assert!(!editor.can_undo());
    }
    let mut document = DxfDocument::default();
    document.add_layer("HIDDEN").unwrap();
    document.push_on_layer(valid, "HIDDEN").unwrap();
    document.set_layer_visible("HIDDEN", false).unwrap();
    let mut editor = Editor::new(document);
    editor.select(vec![0]);
    assert!(editor.selection().is_empty());
    let before = editor.to_dxf();
    assert_eq!(editor.explode_selection().unwrap(), 0);
    assert_eq!(editor.to_dxf(), before);
    assert!(!editor.can_undo());
}
