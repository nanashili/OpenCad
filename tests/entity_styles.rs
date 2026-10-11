use opencad::{
    DxfDocument, Editor, Entity, Line3, Linetype, Point3, render_document_svg, write_dxf,
};

// Imported style insertion and deletion can share a source offset. Existing
// geometry and layer tests never exercise that tie, which previously panicked.
// This check uses the actual document writer without a test-only seam.
#[test]
fn style_insertions_survive_deleting_the_next_entity() {
    let point = |x| Point3::new(x, 0.0, 3.0).unwrap();
    let source = write_dxf(&[
        Line3::new(point(0.0), point(10.0)),
        Line3::new(point(20.0), point(30.0)),
    ]);
    let mut document = DxfDocument::parse(&source).unwrap();
    document.set_entity_color(0, 1).unwrap();
    document.set_entity_linetype(0, "CONTINUOUS").unwrap();
    document.set_entity_linetype_scale(0, 2.0).unwrap();
    document.remove(1).unwrap();
    let saved = document.to_dxf();
    let reloaded = DxfDocument::parse(&saved).unwrap();
    assert_eq!(reloaded.entities().len(), 1);
    let style = reloaded.entity_style(0).unwrap();
    assert_eq!(style.color_index(), 1);
    assert_eq!(style.linetype(), "CONTINUOUS");
    assert_eq!(style.linetype_scale(), 2.0);
    assert_eq!(reloaded.entities()[0], document.entities()[0]);
}

// This input was independently imported and queried in AutoCAD. Earlier layer
// fixtures have no entity overrides and cannot catch a group 6/62/48 being
// mistaken for a layer field or lost when an unrelated field changes.
#[test]
fn imported_styles_keep_source_bytes_and_patch_only_requested_fields() {
    // Actual DXFOUT pads integer values; the earlier hand-sized source fixture
    // could not reveal rejecting BYBLOCK's space-padded group 62.
    let exported = include_str!("fixtures/entity-styles-r12.dxf");
    let retained = DxfDocument::parse(exported).unwrap();
    assert_eq!(retained.to_dxf(), exported);
    assert_eq!(retained.entities().len(), 2);
    assert_eq!(retained.entity_style(1).unwrap().color_index(), 0);
    assert_eq!(retained.entity_style(1).unwrap().linetype(), "BYBLOCK");
    let source = include_str!("fixtures/entity-styles.dxf");
    let mut document = DxfDocument::parse(source).unwrap();
    assert_eq!(document.to_dxf(), source);
    assert_eq!(document.entities().len(), 7);
    assert_eq!(document.linetype_scale(), 2.0);
    let expected = [
        (256, "BYLAYER", 1.0),
        (256, "BYLAYER", 1.0),
        (256, "BYLAYER", 1.0),
        (256, "BYLAYER", 1.0),
        (0, "BYBLOCK", 1.0),
        (1, "CONTINUOUS", 2.0),
        (255, "DASHED", 0.5),
    ];
    for (index, (color, kind, scale)) in expected.into_iter().enumerate() {
        let style = document.entity_style(index).unwrap();
        assert_eq!(
            (
                style.color_index(),
                style.linetype(),
                style.linetype_scale()
            ),
            (color, kind, scale)
        );
    }
    assert_eq!(document.entity_rgb(1, true), Some([0, 255, 0]));
    assert_eq!(document.entity_rgb(4, true), Some([0, 0, 0]));
    assert_eq!(document.entity_rgb(4, false), Some([255, 255, 255]));
    assert_eq!(document.entity_rgb(5, true), Some([255, 0, 0]));
    assert_eq!(document.linetype_for_entity(1).unwrap().name(), "DASHED");
    assert_eq!(
        document.linetype_for_entity(4).unwrap().name(),
        "CONTINUOUS"
    );
    assert_eq!(document.effective_linetype_scale(6), Some(1.0));
    let geometry = document.entities().to_vec();
    document.set_entity_color(4, 4).unwrap();
    document.set_entity_linetype(4, "dashed").unwrap();
    document.set_entity_linetype_scale(6, 2.0).unwrap();
    let expected = source
        .replacen("62\n0\n", "62\n4\n", 1)
        .replacen("6\nBYBLOCK\n", "6\nDASHED\n", 1)
        .replacen("48\n0.5\n", "48\n2\n", 1);
    assert_eq!(document.to_dxf(), expected);
    let reloaded = DxfDocument::parse(&expected).unwrap();
    assert_eq!(reloaded.entities(), geometry);
    assert_eq!(reloaded.layer("DETAIL").unwrap().color_index(), 3);
    assert_eq!(reloaded.entity_style(4).unwrap().linetype(), "DASHED");
    for invalid in [
        source.replacen("48\n0.5\n", "48\n0.5\n48\n2\n", 1),
        source.replacen("62\n255\n", "62\n257\n", 1),
        source.replacen("62\n255\n", "62\n-3\n", 1),
        source.replacen("48\n0.5\n", "48\nNaN\n", 1),
        source.replacen("6\nBYBLOCK\n", "6\n\n", 1),
    ] {
        assert!(DxfDocument::parse(&invalid).is_err());
    }
    let unknown = source.replacen("6\nBYBLOCK\n", "6\nUNRESOLVED\n", 1);
    let retained = DxfDocument::parse(&unknown).unwrap();
    assert_eq!(retained.to_dxf(), unknown);
    assert_eq!(
        retained.linetype_for_entity(4).unwrap().name(),
        "CONTINUOUS"
    );
    // Source DXFIN omits nonpositive 48. Interpreting omission as 1 follows
    // Autodesk's default; the public property getter was unavailable there.
    for value in ["0", "-1"] {
        let unusual = source.replacen("48\n0.5\n", &format!("48\n{value}\n"), 1);
        let retained = DxfDocument::parse(&unusual).unwrap();
        assert_eq!(retained.to_dxf(), unusual);
        assert_eq!(retained.entity_style(6).unwrap().linetype_scale(), 1.0);
    }
    document.set_entity_linetype_scale(6, 0.0).unwrap();
    assert_eq!(document.entity_style(6).unwrap().linetype_scale(), 1.0);
    assert_eq!(
        DxfDocument::parse(&document.to_dxf())
            .unwrap()
            .entity_style(6)
            .unwrap()
            .linetype_scale(),
        1.0
    );
}

// Snapshot tests for geometry/layers cannot detect omitted copied overrides or
// style arrays shifting after deletion. This also owns selection-wide style
// atomicity and the existing layer-lock contract for these new edit operations.
#[test]
fn selection_style_history_copy_and_protection_keep_overrides_aligned() {
    let source = include_str!("fixtures/entity-styles.dxf");
    let mut editor = Editor::new(DxfDocument::parse(source).unwrap());
    editor.select(vec![4, 6]);
    editor.assign_selection_color(4).unwrap();
    assert!(editor.is_dirty());
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), source);
    assert!(!editor.is_dirty());
    assert!(editor.redo());
    editor.assign_selection_linetype("DASHED").unwrap();
    editor.assign_selection_linetype_scale(0.5).unwrap();
    editor.mark_saved();
    let styled = editor.to_dxf();
    for invalid in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(editor.assign_selection_linetype_scale(invalid).is_err());
        assert_eq!(editor.to_dxf(), styled);
    }
    assert!(editor.assign_selection_color(257).is_err());
    assert!(editor.assign_selection_linetype("MISSING").is_err());
    assert!(!editor.is_dirty());
    editor.translate_selection(10.0, -20.0, 3.0, false).unwrap();
    editor.translate_selection(0.0, 10.0, 0.0, true).unwrap();
    assert_eq!(editor.selection(), &[7, 8]);
    assert_eq!(editor.entity_style(7), editor.entity_style(4));
    assert_eq!(editor.entity_style(8), editor.entity_style(6));
    assert_eq!(editor.entity_layer(7), Some("DETAIL"));
    let copied = editor.to_dxf();
    editor.set_layer_locked("DETAIL", true).unwrap();
    let locked = editor.to_dxf();
    assert!(editor.assign_selection_color(1).is_err());
    assert_eq!(editor.to_dxf(), locked);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), copied);
    editor.delete_selection().unwrap();
    assert_eq!(editor.entities().len(), 7);
    assert!(editor.undo());
    assert_eq!(editor.to_dxf(), copied);
    let saved = DxfDocument::parse(&copied).unwrap();
    assert_eq!(saved.entity_style(7), saved.entity_style(4));
    assert_eq!(saved.entity_style(8), saved.entity_style(6));
}

// SVG is a separate consumer: stored properties can be correct while export
// still paints every entity with layer styles or ignores the entity multiplier.
#[test]
fn svg_resolves_entity_inheritance_and_multiplies_pattern_scales() {
    let point = |x, y| Point3::new(x, y, 3.0).unwrap();
    let mut document = DxfDocument::default();
    document.add_layer("DETAIL").unwrap();
    document.set_layer_color("DETAIL", 3).unwrap();
    document
        .add_linetype(Linetype::new("DASHED", vec![1.0, -0.5]).unwrap())
        .unwrap();
    document.set_layer_linetype("DETAIL", "DASHED").unwrap();
    document.set_linetype_scale(2.0).unwrap();
    for y in [0.0, 2.0, 4.0] {
        document
            .push_on_layer(
                Entity::Line(Line3::new(point(0.0, y), point(10.0, y))),
                "DETAIL",
            )
            .unwrap();
    }
    document.set_entity_linetype_scale(0, 0.5).unwrap();
    document.set_entity_color(1, 0).unwrap();
    document.set_entity_linetype(1, "BYBLOCK").unwrap();
    document.set_entity_color(2, 1).unwrap();
    document.set_entity_linetype(2, "CONTINUOUS").unwrap();
    let svg = render_document_svg(&document).unwrap();
    assert_eq!(svg.matches("stroke=\"#00ff00\"").count(), 7);
    assert_eq!(svg.matches("stroke=\"#000000\"").count(), 1);
    assert_eq!(svg.matches("stroke=\"#ff0000\"").count(), 1);
    assert_eq!(svg.matches("<line ").count(), 9);
}
