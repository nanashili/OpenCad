use super::commands::{number, parse_point};
use super::*;
use crate::{
    DxfDocument, Entity,
    layout::{Orientation, PageSetup, Viewport},
};
use std::fs;

impl OpenCadApp {
    pub(super) fn feature_layout_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "LAYOUT" => self.layout_command(tokens),
            "PAGESETUP" => self.pagesetup_command(tokens),
            "MVIEW" => self.mview_command(tokens),
            "PLOT" => self.plot_command(tokens),
            "EXPORTPDF" => self.plot_pdf_command(tokens),
            _ => return None,
        };
        Some(result.inspect(|_| {
            if matches!(
                verb.to_ascii_uppercase().as_str(),
                "LAYOUT" | "PAGESETUP" | "MVIEW"
            ) {
                self.editor.set_layouts(self.layouts.clone());
            }
        }))
    }
    fn layout_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.is_empty() {
            return Err("LAYOUT syntax: NEW|SET|COPY|RENAME|DELETE|LIST ...".into());
        };
        match t[0].to_ascii_uppercase().as_str() {
            "NEW" => {
                if t.len() != 2 {
                    return Err("LAYOUT NEW name.".into());
                };
                self.layouts
                    .create(
                        t[1],
                        PageSetup::new(210.0, 297.0).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())
            }
            "SET" => {
                if t.len() != 2 {
                    return Err("LAYOUT SET name.".into());
                };
                self.layouts.set_active(t[1]).map_err(|e| e.to_string())
            }
            "COPY" => {
                if t.len() != 3 {
                    return Err("LAYOUT COPY source target.".into());
                };
                self.layouts.copy(t[1], t[2]).map_err(|e| e.to_string())
            }
            "RENAME" => {
                if t.len() != 3 {
                    return Err("LAYOUT RENAME old new.".into());
                };
                self.layouts.rename(t[1], t[2]).map_err(|e| e.to_string())
            }
            "DELETE" => {
                if t.len() != 2 {
                    return Err("LAYOUT DELETE name.".into());
                };
                self.layouts.delete(t[1]).map_err(|e| e.to_string())
            }
            "LIST" => {
                self.message(self.layouts.names().collect::<Vec<_>>().join(", "), false);
                Ok(())
            }
            _ => Err("Unknown LAYOUT option.".into()),
        }
    }
    fn pagesetup_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 4 {
            return Err("PAGESETUP width height PORTRAIT|LANDSCAPE scale.".into());
        };
        let w = number(t[0])?;
        let h = number(t[1])?;
        let mut s = PageSetup::new(w, h).map_err(|e| e.to_string())?;
        s.set_orientation(if t[2].eq_ignore_ascii_case("LANDSCAPE") {
            Orientation::Landscape
        } else if t[2].eq_ignore_ascii_case("PORTRAIT") {
            Orientation::Portrait
        } else {
            return Err("Orientation must be PORTRAIT or LANDSCAPE.".into());
        });
        s.set_scale(number(t[3])?).map_err(|e| e.to_string())?;
        self.layouts.active_mut().set_setup(s);
        Ok(())
    }
    fn mview_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 2 {
            return Err("MVIEW min-point max-point.".into());
        };
        let a = parse_point(t[0], self.drawing_elevation())?;
        let b = parse_point(t[1], a.z())?;
        let world =
            crate::layout::world_bounds(self.editor.entities()).map_err(|e| e.to_string())?;
        let scale = self.layouts.active().setup().scale();
        self.layouts.active_mut().add_viewport(
            Viewport::new(world, [a.x(), a.y(), b.x(), b.y()], scale).map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn plot_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 1 {
            return Err("PLOT output.svg.".into());
        };
        let inner =
            crate::render_document_svg(self.editor.document()).map_err(|e| e.to_string())?;
        let setup = self.layouts.active().setup();
        let [w, h] = setup.dimensions();
        let m = setup.margins();
        let s = setup.scale();
        let body = inner
            .lines()
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with("</svg>"))
            .collect::<Vec<_>>()
            .join("\n");
        let transform = if let Some(v) = self.layouts.active().viewports().first() {
            let q = v.world();
            let p = v.paper();
            format!(
                "translate({} {}) scale({} {}) translate({} {})",
                p[0],
                p[1],
                v.scale(),
                v.scale(),
                -q[0],
                -q[1]
            )
        } else {
            format!("translate({} {}) scale({s} {s})", m[0], m[1])
        };
        let plotted = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-label=\"Plotted CAD layout\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><g transform=\"{transform}\">{body}</g></svg>\n"
        );
        fs::write(t[0], plotted).map_err(|e| format!("Cannot write plot: {e}"))
    }
    fn plot_pdf_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 1 {
            return Err("EXPORTPDF output.pdf.".into());
        };
        let layout = self.layouts.active();
        let bytes = vector_pdf(self.editor.document(), layout.setup(), layout.viewports())
            .map_err(|e| e.to_string())?;
        fs::write(t[0], bytes).map_err(|e| format!("Cannot write PDF: {e}"))
    }
}

fn vector_pdf(
    document: &DxfDocument,
    setup: PageSetup,
    viewports: &[Viewport],
) -> Result<Vec<u8>, String> {
    let entities = flatten_entities(document)?;
    let b = crate::layout::world_bounds(&entities).map_err(|e| e.to_string())?;
    let [w, h] = setup.dimensions();
    let m = setup.margins();
    let scale = setup.scale();
    let mut content = String::from("1 w 0 0 0 RG\n");
    for e in &entities {
        let p = |x: f64, y: f64| {
            if let Some(v) = viewports.first() {
                let q = v.map([x, y]);
                (q[0], q[1])
            } else {
                ((x - b[0]) * scale + m[0], (y - b[1]) * scale + m[1])
            }
        };
        match e {
            Entity::Line(l) => {
                let (x1, y1) = p(l.start().x(), l.start().y());
                let (x2, y2) = p(l.end().x(), l.end().y());
                content.push_str(&format!("{x1} {y1} m {x2} {y2} l S\n"));
            }
            Entity::Polyline(pl) => {
                for seg in pl.segments() {
                    let seg = seg.map_err(|e| e.to_string())?;
                    let a = seg.point_at_fraction(0.0).map_err(|e| e.to_string())?;
                    let z = seg.point_at_fraction(1.0).map_err(|e| e.to_string())?;
                    let (x1, y1) = p(a.x(), a.y());
                    let (x2, y2) = p(z.x(), z.y());
                    content.push_str(&format!("{x1} {y1} m {x2} {y2} l S\n"));
                }
            }
            Entity::Circle(c) => append_curve(
                &mut content,
                (0..=48).map(|i| {
                    let a = std::f64::consts::TAU * f64::from(i) / 48.0;
                    (
                        c.center().x() + c.radius() * a.cos(),
                        c.center().y() + c.radius() * a.sin(),
                    )
                }),
                &p,
            ),
            Entity::Arc(a) => append_curve(
                &mut content,
                (0..=32)
                    .map(|i| {
                        a.point_at_fraction(f64::from(i) / 32.0)
                            .map(|q| (q.x(), q.y()))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?
                    .into_iter(),
                &p,
            ),
            Entity::Ellipse(e) => append_curve(
                &mut content,
                (0..=64)
                    .map(|i| {
                        e.point_at_fraction(f64::from(i) / 64.0)
                            .map(|q| (q.x(), q.y()))
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?
                    .into_iter(),
                &p,
            ),
            Entity::Spline(s) => {
                let (lo, hi) = s.parameter_range();
                append_curve(
                    &mut content,
                    (0..=96)
                        .map(|i| {
                            s.point_at(lo + (hi - lo) * f64::from(i) / 96.0)
                                .map(|q| (q.x(), q.y()))
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string())?
                        .into_iter(),
                    &p,
                );
            }
            Entity::Face3(f) => {
                let (vs, n) = f.vertices();
                let (x, y) = p(vs[0].x(), vs[0].y());
                content.push_str(&format!("{x} {y} m "));
                for q in &vs[1..n] {
                    let (x, y) = p(q.x(), q.y());
                    content.push_str(&format!("{x} {y} l "));
                }
                content.push_str("h f\n");
            }
            Entity::Text(t) => append_text(&mut content, t.content(), t.location(), &p),
            Entity::MText(t) => {
                append_text(&mut content, t.text().content(), t.text().location(), &p)
            }
            Entity::Leader(l) => append_lines(
                &mut content,
                l.vertices().windows(2).map(|v| (v[0], v[1])),
                &p,
            ),
            Entity::Table(t) => {
                append_lines(
                    &mut content,
                    t.grid_lines()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .map(|l| (l.start(), l.end())),
                    &p,
                );
                let mut y = t.anchor().y();
                for (row_height, cells) in t.row_heights().iter().zip(t.cells()) {
                    let mut x = t.anchor().x();
                    for (cell, width) in cells.iter().zip(t.column_widths()) {
                        append_text(
                            &mut content,
                            cell,
                            crate::Point3::new(
                                x + *width * 0.1,
                                y + *row_height * 0.5,
                                t.anchor().z(),
                            )
                            .map_err(|e| e.to_string())?,
                            &p,
                        );
                        x += *width;
                    }
                    y += *row_height;
                }
            }
            Entity::Dimension(d) => {
                for primitive in d.geometry().map_err(|e| e.to_string())? {
                    match primitive {
                        crate::DimensionPrimitive::Line(l) => {
                            append_lines(&mut content, std::iter::once((l.start(), l.end())), &p)
                        }
                        crate::DimensionPrimitive::Arc(a) => append_curve(
                            &mut content,
                            (0..=32)
                                .map(|i| {
                                    a.point_at_fraction(f64::from(i) / 32.0)
                                        .map(|q| (q.x(), q.y()))
                                })
                                .collect::<Result<Vec<_>, _>>()
                                .map_err(|e| e.to_string())?
                                .into_iter(),
                            &p,
                        ),
                        crate::DimensionPrimitive::Text(t) => {
                            append_text(&mut content, t.content(), t.location(), &p)
                        }
                    }
                }
            }
            Entity::Hatch(h) => {
                append_hatch(&mut content, h.boundaries(), &p);
            }
            Entity::XLine(line) => append_construction(
                &mut content,
                line.origin(),
                line.direction(),
                [b[0], b[1], b[2], b[3]],
                false,
                &p,
            ),
            Entity::Ray(ray) => append_construction(
                &mut content,
                ray.origin(),
                ray.direction(),
                [b[0], b[1], b[2], b[3]],
                true,
                &p,
            ),
            Entity::Image(_) => {
                return Err("EXPORTPDF does not support raster IMAGE entities".into());
            }
            Entity::Point(point) => {
                let q = point.position();
                let (x, y) = p(q.x(), q.y());
                content.push_str(&format!(
                    "{} {} m {} {} l {} {} m {} {} l S\n",
                    x - 1.0,
                    y,
                    x + 1.0,
                    y,
                    x,
                    y - 1.0,
                    x,
                    y + 1.0
                ));
            }
            Entity::Insert(_) => {
                return Err("EXPORTPDF unresolved INSERT after block expansion".into());
            }
        }
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.len(),
            content
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, o).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    Ok(out)
}

fn flatten_entities(document: &DxfDocument) -> Result<Vec<Entity>, String> {
    fn visit(
        document: &DxfDocument,
        source: &[Entity],
        depth: usize,
        out: &mut Vec<Entity>,
    ) -> Result<(), String> {
        if depth > 32 {
            return Err("INSERT nesting exceeds 32 levels".into());
        }
        for entity in source {
            if let Entity::Insert(insert) = entity {
                let Some(block) = document.block(insert.block_name()) else {
                    return Err(format!("unresolved INSERT block {}", insert.block_name()));
                };
                let expanded = insert.explode(block).map_err(|e| e.to_string())?;
                visit(document, &expanded, depth + 1, out)?;
            } else {
                out.push(entity.clone());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(document, document.entities(), 0, &mut out)?;
    Ok(out)
}

fn append_curve<I>(out: &mut String, points: I, map: &impl Fn(f64, f64) -> (f64, f64))
where
    I: IntoIterator<Item = (f64, f64)>,
{
    let mut it = points.into_iter();
    let Some((x, y)) = it.next() else {
        return;
    };
    let (x, y) = map(x, y);
    out.push_str(&format!("{x} {y} m "));
    for (x, y) in it {
        let (x, y) = map(x, y);
        out.push_str(&format!("{x} {y} l "));
    }
    out.push_str("S\n");
}
fn append_hatch(
    out: &mut String,
    boundaries: &[Vec<crate::Point3>],
    map: &impl Fn(f64, f64) -> (f64, f64),
) {
    for boundary in boundaries {
        let Some(first) = boundary.first() else {
            continue;
        };
        let (x, y) = map(first.x(), first.y());
        out.push_str(&format!("{x} {y} m "));
        for point in &boundary[1..] {
            let (x, y) = map(point.x(), point.y());
            out.push_str(&format!("{x} {y} l "));
        }
        out.push_str("h ");
    }
    out.push_str("f*\n");
}
fn append_lines<I>(out: &mut String, lines: I, map: &impl Fn(f64, f64) -> (f64, f64))
where
    I: IntoIterator<Item = (crate::Point3, crate::Point3)>,
{
    for (a, b) in lines {
        let (x1, y1) = map(a.x(), a.y());
        let (x2, y2) = map(b.x(), b.y());
        out.push_str(&format!("{x1} {y1} m {x2} {y2} l S\n"));
    }
}
fn append_construction(
    out: &mut String,
    origin: crate::Point3,
    direction: crate::Point3,
    bounds: [f64; 4],
    ray: bool,
    map: &impl Fn(f64, f64) -> (f64, f64),
) {
    let min = match crate::Point3::new(bounds[0], bounds[1], -1.0e9) {
        Ok(p) => p,
        Err(_) => return,
    };
    let max = match crate::Point3::new(bounds[2], bounds[3], 1.0e9) {
        Ok(p) => p,
        Err(_) => return,
    };
    let points = if ray {
        crate::Ray3::new(origin, direction).ok().and_then(|line| {
            let (a, b) = line.clip_to_bounds(min, max).ok().flatten()?;
            Some((line.point_at(a).ok()?, line.point_at(b).ok()?))
        })
    } else {
        crate::XLine3::new(origin, direction).ok().and_then(|line| {
            let (a, b) = line.clip_to_bounds(min, max).ok().flatten()?;
            Some((line.point_at(a).ok()?, line.point_at(b).ok()?))
        })
    };
    if let Some((a, b)) = points {
        append_lines(out, std::iter::once((a, b)), map);
    }
}
fn append_text(
    out: &mut String,
    text: &str,
    point: crate::Point3,
    map: &impl Fn(f64, f64) -> (f64, f64),
) {
    let (x, y) = map(point.x(), point.y());
    let escaped = text
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    out.push_str(&format!("BT /F1 8 Tf {x} {y} Td ({escaped}) Tj ET\n"));
}
