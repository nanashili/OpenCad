use opencad::{
    Arc3, Circle3, DxfDocument, Editor, Entity, Line3, Linetype, Point3, SvgError,
    render_document_svg,
};

// The independent AutoCAD export contains opaque header/table/block metadata,
// three geometry types and a group-40 value differing from the dash-length sum.
#[test]
fn native_linetypes_roundtrip_and_edit_only_requested_values() {
    let source = include_str!("fixtures/linetypes.dxf");
    let mut document = DxfDocument::parse(source).unwrap();
    assert_eq!(document.to_dxf(), source);
    assert_eq!(document.entities().len(), 3);
    assert_eq!(document.linetype_scale(), 2.0);
    let dashdot = document.linetype("dashdot").unwrap();
    assert_eq!(dashdot.pattern(), &[1.0, -0.25, 0.0, -0.25]);
    assert_eq!(dashdot.pattern_length(), 0.5);
    assert_eq!(document.linetype("DASHED").unwrap().pattern(), &[1.0, -0.5]);
    let original_entities = document.entities().to_vec();
    assert!(document.set_layer_linetype("dashed", "dashdot").unwrap());
    assert!(!document.set_layer_linetype("DASHED", "DASHDOT").unwrap());
    assert!(document.set_linetype_scale(3.0).unwrap());
    let expected = source
        .replacen("  6\nDASHED\n", "  6\nDASHDOT\n", 1)
        .replacen("$LTSCALE\n 40\n2.0\n", "$LTSCALE\n 40\n3\n", 1);
    let saved = document.to_dxf();
    assert_eq!(saved, expected);
    let reloaded = DxfDocument::parse(&saved).unwrap();
    assert_eq!(reloaded.entities(), original_entities);
    assert_eq!(reloaded.layer("DASHED").unwrap().linetype(), "DASHDOT");
    assert_eq!(reloaded.linetype_scale(), 3.0);
    // Independent declared group 40 storage includes negative finite values.
    let negative = source.replacen(" 40\n0.5\n", " 40\n-0.5\n", 1);
    let retained = DxfDocument::parse(&negative).unwrap();
    assert_eq!(retained.linetype("DASHDOT").unwrap().pattern_length(), -0.5);
    assert_eq!(retained.to_dxf(), negative);
    for invalid in [
        source.replacen(" 73\n     4\n", " 73\n     3\n", 1),
        source.replacen(" 72\n    65\n", " 72\n    83\n", 1),
        source.replacen(" 49\n-0.25\n", " 49\nNaN\n", 1),
    ] {
        assert!(DxfDocument::parse(&invalid).is_err());
    }
}

// Existing layer tests do not exercise adding two symbol tables at the same
// source offset, a missing HEADER, or style-aware geometry/SVG rendering.
#[test]
fn new_linetypes_share_table_section_and_survive_history_and_svg() {
    let source = "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
    let mut editor = Editor::new(DxfDocument::parse(source).unwrap());
    editor.add_layer("DETAIL").unwrap();
    editor
        .add_linetype(Linetype::new("DASHDOT", vec![2.0, -1.0, 0.0, -1.0]).unwrap())
        .unwrap();
    editor.set_layer_linetype("DETAIL", "dashdot").unwrap();
    editor.set_linetype_scale(2.0).unwrap();
    let saved = editor.to_dxf();
    assert_eq!(saved.matches("2\nTABLES\n").count(), 1);
    assert_eq!(saved.matches("2\nHEADER\n").count(), 1);
    // R12 readers must see a type before a layer references it.
    assert!(saved.find("2\nLTYPE\n").unwrap() < saved.find("2\nLAYER\n").unwrap());
    let mut document = DxfDocument::parse(&saved).unwrap();
    assert_eq!(document.linetype("DASHDOT").unwrap().pattern_length(), 4.0);
    assert_eq!(document.linetype_scale(), 2.0);
    assert!(editor.undo());
    assert_eq!(editor.linetype_scale(), 1.0);
    assert!(editor.redo());
    assert_eq!(editor.to_dxf(), saved);
    let point = |x, y| Point3::new(x, y, 3.0).unwrap();
    document
        .push_on_layer(
            Entity::Line(Line3::new(point(0.0, 0.0), point(20.0, 0.0))),
            "DETAIL",
        )
        .unwrap();
    let svg = render_document_svg(&document).unwrap();
    assert_eq!(svg.matches("<line ").count(), 3);
    assert_eq!(svg.matches("fill=\"#000000\" stroke=\"none\"").count(), 2);
    assert!(svg.contains("x1=\"10\" y1=\"10\" x2=\"14\" y2=\"10\""));
    assert!(svg.contains("cx=\"16\" cy=\"10\" r=\"0.5\""));
    document
        .push_on_layer(
            Entity::Circle(Circle3::new(point(10.0, 10.0), 5.0).unwrap()),
            "DETAIL",
        )
        .unwrap();
    document
        .push_on_layer(
            Entity::Arc(Arc3::new(point(0.0, 10.0), 5.0, 0.0, std::f64::consts::PI).unwrap()),
            "DETAIL",
        )
        .unwrap();
    assert!(
        render_document_svg(&document)
            .unwrap()
            .matches("<path ")
            .count()
            > 2
    );
    let before = document.to_dxf();
    assert!(document.set_layer_linetype("DETAIL", "UNDEFINED").is_err());
    assert!(document.set_linetype_scale(0.0).is_err());
    assert_eq!(document.to_dxf(), before);
    document.set_linetype_scale(1e-10).unwrap();
    assert!(matches!(
        render_document_svg(&document),
        Err(SvgError::Linetype(opencad::LinetypeError::TooDense))
    ));
}

// The actual widget file was discarded by AutoCAD when LAYER preceded LTYPE.
// The new-document case above does not cover adding LTYPE to an existing table
// section while retaining every unrelated byte in that source.
#[test]
fn new_linetype_precedes_existing_layer_table_without_rewriting_source() {
    let source = "0\r\nSECTION\r\n2\r\nTABLES\r\n999\r\nretained comment\r\n0\r\nTABLE\r\n2\r\nLAYER\r\n70\r\n1\r\n0\r\nLAYER\r\n2\r\n0\r\n70\r\n0\r\n62\r\n7\r\n6\r\nCONTINUOUS\r\n0\r\nENDTAB\r\n0\r\nENDSEC\r\n0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n";
    let mut document = DxfDocument::parse(source).unwrap();
    document
        .add_linetype(Linetype::new("DOTTED", vec![0.0, -1.0]).unwrap())
        .unwrap();
    document.set_layer_linetype("0", "DOTTED").unwrap();
    let saved = document.to_dxf();
    let type_start = saved.find("0\nTABLE\n2\nLTYPE\n").unwrap();
    let layer_start = saved.find("0\r\nTABLE\r\n2\r\nLAYER\r\n").unwrap();
    assert!(type_start < layer_start);
    let reloaded = DxfDocument::parse(&saved).unwrap();
    assert_eq!(reloaded.layer("0").unwrap().linetype(), "DOTTED");
    assert_eq!(reloaded.linetype("DOTTED").unwrap().pattern(), &[0.0, -1.0]);
    assert_eq!(reloaded.to_dxf(), saved);
    let type_end =
        type_start + saved[type_start..].find("0\nENDTAB\n").unwrap() + "0\nENDTAB\n".len();
    let mut unchanged = saved;
    unchanged.replace_range(type_start..type_end, "");
    assert_eq!(
        unchanged.replacen("6\r\nDOTTED\r\n", "6\r\nCONTINUOUS\r\n", 1),
        source
    );
}
