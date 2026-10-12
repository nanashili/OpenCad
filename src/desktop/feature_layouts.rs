use super::commands::{number, parse_point};
use super::*;
use crate::{
    DxfDocument, Entity,
    layout::{Layout, Orientation, PageSetup, PlotTransform, Viewport},
};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const MAX_PLOT_REFERENCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PLOT_IMAGE_PIXELS: u64 = 16_000_000;
const MAX_PDF_LAYOUT_PAGES: usize = 128;

impl OpenCadApp {
    pub(super) fn feature_layout_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        if matches!(
            verb.to_ascii_uppercase().as_str(),
            "PAGESETUP" | "MVIEW" | "VPCLIP"
        ) && self.active_viewport.is_some()
            && let Err(error) = self.leave_model_space()
        {
            return Some(Err(error));
        }
        let result = match verb.to_ascii_uppercase().as_str() {
            "LAYOUT" => self.layout_command(tokens),
            "PAGESETUP" => self.pagesetup_command(tokens),
            "MVIEW" => self.mview_command(tokens),
            "VPCLIP" => self.vpclip_command(tokens),
            "PLOT" => self.plot_command(tokens),
            "EXPORTPDF" => self.plot_pdf_command(tokens),
            _ => return None,
        };
        Some(result.inspect(|_| {
            if matches!(
                verb.to_ascii_uppercase().as_str(),
                "PAGESETUP" | "MVIEW" | "VPCLIP"
            ) {
                self.editor.set_layouts(self.layouts.clone());
            }
        }))
    }
    fn layout_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.is_empty() {
            return Err("LAYOUT syntax: NEW|SET|COPY|RENAME|DELETE|LIST ...".into());
        };
        if matches!(
            t[0].to_ascii_uppercase().as_str(),
            "NEW" | "SET" | "COPY" | "RENAME" | "DELETE"
        ) {
            self.leave_model_space()?;
        }
        match t[0].to_ascii_uppercase().as_str() {
            "NEW" => {
                if t.len() != 2 {
                    return Err("LAYOUT NEW name.".into());
                };
                self.editor
                    .create_layout(
                        t[1],
                        PageSetup::new(210.0, 297.0).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                self.layouts = self.editor.layouts().clone();
                Ok(())
            }
            "SET" => {
                if t.len() != 2 {
                    return Err("LAYOUT SET name.".into());
                };
                self.editor
                    .activate_layout(t[1])
                    .map_err(|e| e.to_string())?;
                self.layouts = self.editor.layouts().clone();
                Ok(())
            }
            "COPY" => {
                if t.len() != 3 {
                    return Err("LAYOUT COPY source target.".into());
                };
                self.editor
                    .copy_layout(t[1], t[2])
                    .map_err(|e| e.to_string())?;
                self.layouts = self.editor.layouts().clone();
                Ok(())
            }
            "RENAME" => {
                if t.len() != 3 {
                    return Err("LAYOUT RENAME old new.".into());
                };
                self.editor
                    .rename_layout(t[1], t[2])
                    .map_err(|e| e.to_string())?;
                self.layouts = self.editor.layouts().clone();
                Ok(())
            }
            "DELETE" => {
                if t.len() != 2 {
                    return Err("LAYOUT DELETE name.".into());
                };
                self.editor.delete_layout(t[1]).map_err(|e| e.to_string())?;
                self.layouts = self.editor.layouts().clone();
                Ok(())
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
        if t[3].eq_ignore_ascii_case("FIT") {
            s.set_fit_to_paper(true);
        } else {
            s.set_scale(number(t[3])?).map_err(|e| e.to_string())?;
        }
        self.layouts.active_mut().set_setup(s);
        Ok(())
    }
    fn mview_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 2 {
            return Err("MVIEW min-point max-point.".into());
        };
        if self.layouts.active().name() == "Model" {
            return Err("MVIEW requires a paper layout.".into());
        }
        let a = parse_point(t[0], self.drawing_elevation())?;
        let b = parse_point(t[1], a.z())?;
        let model_entities = self
            .editor
            .entities()
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                self.editor
                    .document()
                    .entity_space(*index)
                    .is_none_or(|space| space.matches_layout("Model"))
            })
            .map(|(_, entity)| entity.clone())
            .collect::<Vec<_>>();
        let world = if model_entities.is_empty() {
            [0.0, 0.0, 100.0, 100.0]
        } else {
            crate::layout::world_bounds(&model_entities).map_err(|e| e.to_string())?
        };
        let scale = self.layouts.active().setup().scale();
        self.layouts.active_mut().add_viewport(
            Viewport::new(world, [a.x(), a.y(), b.x(), b.y()], scale).map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn vpclip_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() < 2 {
            return Err("VPCLIP index POLYGON point;point;... | ON | OFF | DELETE.".into());
        }
        let index = t[0]
            .parse::<usize>()
            .map_err(|_| "VPCLIP index must be a positive integer.")?;
        let viewport = self
            .layouts
            .active_mut()
            .viewports_mut()
            .get_mut(index.saturating_sub(1))
            .ok_or("VPCLIP viewport index is out of range.")?;
        match t[1].to_ascii_uppercase().as_str() {
            "OFF" => viewport.set_clip_enabled(false).map_err(|e| e.to_string()),
            "ON" => viewport.set_clip_enabled(true).map_err(|e| e.to_string()),
            "DELETE" => {
                viewport.clear_clip_boundary();
                Ok(())
            }
            "POLYGON" => {
                let mut points = Vec::new();
                for token in &t[2..] {
                    for point in token.split(';').filter(|value| !value.trim().is_empty()) {
                        let p = parse_point(point.trim(), 0.0)?;
                        points.push(
                            crate::Point3::new(p.x(), p.y(), 0.0).map_err(|e| e.to_string())?,
                        );
                    }
                }
                viewport
                    .set_clip_boundary(points)
                    .map_err(|e| e.to_string())
            }
            _ => Err("VPCLIP option must be POLYGON, ON, OFF, or DELETE.".into()),
        }
    }
    fn plot_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 1 {
            return Err("PLOT output.svg.".into());
        };
        let layout_name = self.layouts.active().name().to_owned();
        let layouts = self.layouts_for_persistence()?;
        let active_layout = layouts.active();
        let viewports = active_layout.viewports();
        let model_scope_needed = layout_name == "Model" || !viewports.is_empty();
        let mut document = self.editor.document().clone();
        document.set_layouts(layouts.clone());
        let (inner, plot_bounds) = if model_scope_needed {
            render_layout_svg(&document, None, layout_name != "Model")?
        } else {
            empty_layout_svg()
        };
        let paper = if layout_name == "Model" {
            None
        } else {
            Some(render_layout_svg(&document, Some(&layout_name), true)?)
        };
        let setup = active_layout.setup();
        let plotted = render_svg_plot(
            &inner,
            plot_bounds,
            paper.as_ref().map(|(svg, bounds)| (svg.as_str(), *bounds)),
            setup,
            viewports,
        )?;
        fs::write(t[0], plotted).map_err(|e| format!("Cannot write plot: {e}"))
    }
    fn plot_pdf_command(&mut self, t: &[&str]) -> Result<(), String> {
        if t.len() != 1 {
            return Err("EXPORTPDF output.pdf.".into());
        };
        let layouts = self.layouts_for_persistence()?;
        let layout = layouts.active().clone();
        let mut document = self.editor.document().clone();
        document.set_layouts(layouts);
        let bytes =
            vector_pdf(&document, &layout, self.path.as_deref()).map_err(|e| e.to_string())?;
        fs::write(t[0], bytes).map_err(|e| format!("Cannot write PDF: {e}"))
    }
}

fn render_layout_svg(
    document: &DxfDocument,
    layout_name: Option<&str>,
    allow_empty: bool,
) -> Result<(String, Option<[f64; 4]>), String> {
    match crate::svg::render_document_svg_for_layout_with_bounds(document, layout_name) {
        Ok((svg, bounds)) => Ok((svg, Some(bounds))),
        Err(crate::svg::SvgError::EmptyDrawing) if allow_empty => Ok((
            "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".into(),
            None,
        )),
        Err(error) => Err(error.to_string()),
    }
}

fn empty_layout_svg() -> (String, Option<[f64; 4]>) {
    (
        "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".into(),
        None,
    )
}

fn render_svg_plot(
    inner: &str,
    plot_bounds: Option<[f64; 4]>,
    paper: Option<(&str, Option<[f64; 4]>)>,
    setup: PageSetup,
    viewports: &[Viewport],
) -> Result<String, String> {
    let [w, h] = setup.dimensions();
    let body = extract_svg_body(inner);
    let source_bounds = if viewports.is_empty() {
        paper
            .and_then(|(_, bounds)| bounds)
            .or(plot_bounds)
            .unwrap_or([0.0, 0.0, 1.0, 1.0])
    } else {
        let viewport_bounds =
            plot_source_bounds(plot_bounds.unwrap_or([0.0, 0.0, 1.0, 1.0]), viewports);
        paper
            .and_then(|(_, bounds)| bounds)
            .map_or(viewport_bounds, |bounds| {
                union_bounds(viewport_bounds, bounds)
            })
    };
    let plot_transform = setup
        .plot_transform(source_bounds)
        .map_err(|e| e.to_string())?;
    let mut defs = String::new();
    let mut groups = String::new();
    append_svg_page_clip(&mut defs, plot_transform, h);
    if paper.is_none() || !viewports.is_empty() {
        if viewports.is_empty() {
            append_svg_plot_group(
                &mut groups,
                &body,
                plot_transform,
                plot_bounds.unwrap_or(source_bounds),
                None,
                h,
                None,
            );
        } else {
            for (index, viewport) in viewports.iter().enumerate() {
                append_svg_viewport_clips(&mut defs, plot_transform, viewport, index, h);
                append_svg_plot_group(
                    &mut groups,
                    &body,
                    plot_transform,
                    plot_bounds.unwrap_or(source_bounds),
                    Some(viewport),
                    h,
                    Some(index),
                );
            }
        }
    }
    if let Some((paper_inner, Some(paper_bounds))) = paper {
        let paper_body = extract_svg_body(paper_inner);
        let transform = svg_body_transform(plot_transform, paper_bounds, None, h);
        groups.push_str(&format!(
            "<g clip-path=\"url(#plotclip)\"><g transform=\"{transform}\">{paper_body}</g></g>"
        ));
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-label=\"Plotted CAD layout\"><defs>{defs}</defs><rect width=\"100%\" height=\"100%\" fill=\"white\"/>{groups}</svg>\n"
    ))
}

fn extract_svg_body(inner: &str) -> String {
    inner
        .lines()
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with("</svg>"))
        .filter(|line| !line.contains("<rect width=\"100%\" height=\"100%\" fill=\"white\""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_svg_page_clip(defs: &mut String, transform: PlotTransform, page_height: f64) {
    let rect = transform.clip_rect();
    defs.push_str(&format!(
        "<clipPath id=\"plotclip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" /></clipPath>",
        rect[0],
        page_height - rect[3],
        rect[2] - rect[0],
        rect[3] - rect[1]
    ));
}

fn append_svg_viewport_clips(
    defs: &mut String,
    transform: PlotTransform,
    viewport: &Viewport,
    index: usize,
    page_height: f64,
) {
    let rect = transformed_rect(transform, viewport.paper());
    let clip_id = format!("vpclip{index}");
    defs.push_str(&format!(
        "<clipPath id=\"{clip_id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" /></clipPath>",
        rect[0],
        page_height - rect[3],
        rect[2] - rect[0],
        rect[3] - rect[1]
    ));
    if !viewport.clip_enabled() {
        return;
    }
    let Some(points) = viewport.clip_boundary() else {
        return;
    };
    let polygon_id = format!("vppolygon{index}");
    let polygon = points
        .iter()
        .map(|point| {
            let mapped = transform.map([point.x(), point.y()]);
            format!("{},{}", mapped[0], page_height - mapped[1])
        })
        .collect::<Vec<_>>()
        .join(" ");
    defs.push_str(&format!(
        "<clipPath id=\"{polygon_id}\"><polygon points=\"{polygon}\" /></clipPath>"
    ));
}

fn append_svg_plot_group(
    groups: &mut String,
    body: &str,
    transform: PlotTransform,
    bounds: [f64; 4],
    viewport: Option<&Viewport>,
    page_height: f64,
    index: Option<usize>,
) {
    let body_transform = svg_body_transform(transform, bounds, viewport, page_height);
    let has_polygon_clip =
        viewport.is_some_and(|value| value.clip_enabled() && value.clip_boundary().is_some());
    groups.push_str("<g clip-path=\"url(#plotclip)\">");
    if let Some(index) = index {
        groups.push_str(&format!("<g clip-path=\"url(#vpclip{index})\">"));
        if has_polygon_clip {
            groups.push_str(&format!("<g clip-path=\"url(#vppolygon{index})\">"));
        }
    }
    groups.push_str(&format!("<g transform=\"{body_transform}\">{body}</g>"));
    if index.is_some() {
        if has_polygon_clip {
            groups.push_str("</g>");
        }
        groups.push_str("</g>");
    }
    groups.push_str("</g>");
}

fn vector_pdf(
    document: &DxfDocument,
    layout: &Layout,
    document_path: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let mut pdf = PdfWriter::new();
    let page = render_pdf_page(document, layout, document_path, &mut pdf)?;
    pdf.finish_pages(vec![page])
}

pub(super) fn vector_pdf_layouts(
    document: &DxfDocument,
    layouts: &[Layout],
    document_path: Option<&Path>,
) -> Result<Vec<u8>, String> {
    if layouts.is_empty() {
        return Err("EXPORTPDF requires at least one layout.".into());
    }
    if layouts.len() > MAX_PDF_LAYOUT_PAGES {
        return Err("EXPORTPDF exceeds the supported page count.".into());
    }
    let mut pdf = PdfWriter::new();
    let mut pages = Vec::with_capacity(layouts.len());
    for layout in layouts {
        pages.push(render_pdf_page(document, layout, document_path, &mut pdf)?);
    }
    pdf.finish_pages(pages)
}

fn render_pdf_page(
    document: &DxfDocument,
    layout: &Layout,
    document_path: Option<&Path>,
    pdf: &mut PdfWriter,
) -> Result<PdfPage, String> {
    let setup = layout.setup();
    let viewports = layout.viewports();
    let model_prepared = if layout.name() == "Model" || !viewports.is_empty() {
        prepare_pdf_entities(document, document_path, None)?
    } else {
        PreparedPdfEntities {
            entities: Vec::new(),
            bounds: None,
        }
    };
    let paper_prepared = if layout.name() == "Model" {
        None
    } else {
        Some(prepare_pdf_entities(
            document,
            document_path,
            Some(layout.name()),
        )?)
    };
    let model_bounds = model_prepared.bounds;
    let paper_bounds = paper_prepared.as_ref().and_then(|prepared| prepared.bounds);
    let [w, h] = setup.dimensions();
    let source_bounds = if viewports.is_empty() {
        paper_bounds
            .or(model_bounds)
            .unwrap_or([0.0, 0.0, 1.0, 1.0])
    } else {
        let viewport_bounds =
            plot_source_bounds(model_bounds.unwrap_or([0.0, 0.0, 1.0, 1.0]), viewports);
        paper_bounds.map_or(viewport_bounds, |bounds| {
            union_bounds(viewport_bounds, bounds)
        })
    };
    let plot_transform = setup
        .plot_transform(source_bounds)
        .map_err(|error| error.to_string())?;
    let mut content = String::from("1 w 0 0 0 RG\n");
    let mut xobjects = Vec::new();
    let viewport_passes = if layout.name() == "Model" || !viewports.is_empty() {
        viewports.len().max(1)
    } else {
        0
    };
    for viewport_index in 0..viewport_passes {
        let active_viewport = viewports.get(viewport_index);
        let construction_bounds = active_viewport
            .map(Viewport::world)
            .or(model_bounds)
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let mut pass = PdfEntityPass {
            active_viewport,
            plot_transform,
            construction_bounds,
            document_path,
            pdf,
            content: &mut content,
            xobjects: &mut xobjects,
        };
        render_pdf_viewport_pass(&model_prepared.entities, &mut pass)?;
    }
    if let Some(paper) = paper_prepared {
        let mut pass = PdfEntityPass {
            active_viewport: None,
            plot_transform,
            construction_bounds: paper.bounds.unwrap_or(source_bounds),
            document_path,
            pdf,
            content: &mut content,
            xobjects: &mut xobjects,
        };
        render_pdf_viewport_pass(&paper.entities, &mut pass)?;
    }
    let xobject_dict = xobjects
        .iter()
        .map(|resource| format!("/{} {} 0 R", resource.name, resource.object_id))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(PdfPage {
        width: w,
        height: h,
        content,
        xobjects: xobject_dict,
    })
}

struct PdfEntityPass<'a> {
    active_viewport: Option<&'a Viewport>,
    plot_transform: PlotTransform,
    construction_bounds: [f64; 4],
    document_path: Option<&'a Path>,
    pdf: &'a mut PdfWriter,
    content: &'a mut String,
    xobjects: &'a mut Vec<PdfImageResource>,
}

fn render_pdf_viewport_pass(
    entities: &[Entity],
    pass: &mut PdfEntityPass<'_>,
) -> Result<(), String> {
    let [clip_x0, clip_y0, clip_x1, clip_y1] = pass.plot_transform.clip_rect();
    pass.content.push_str(&format!(
        "q {clip_x0} {clip_y0} {} {} re W n\n",
        clip_x1 - clip_x0,
        clip_y1 - clip_y0
    ));
    if let Some(viewport) = pass.active_viewport {
        let [x0, y0, x1, y1] = transformed_rect(pass.plot_transform, viewport.paper());
        pass.content
            .push_str(&format!("q {x0} {y0} {} {} re W n\n", x1 - x0, y1 - y0));
        if let Some(points) = viewport.clip_boundary().filter(|_| viewport.clip_enabled()) {
            let first = points.first().expect("validated viewport clip");
            let first = pass.plot_transform.map([first.x(), first.y()]);
            pass.content
                .push_str(&format!("{} {} m ", first[0], first[1]));
            for point in &points[1..] {
                let mapped = pass.plot_transform.map([point.x(), point.y()]);
                pass.content
                    .push_str(&format!("{} {} l ", mapped[0], mapped[1]));
            }
            pass.content.push_str("h W n\n");
        }
    }
    render_pdf_entity_pass(entities, pass)?;
    if pass.active_viewport.is_some() {
        pass.content.push_str("Q\n");
    }
    pass.content.push_str("Q\n");
    Ok(())
}

fn render_pdf_entity_pass(entities: &[Entity], pass: &mut PdfEntityPass<'_>) -> Result<(), String> {
    let active_viewport = pass.active_viewport;
    let plot_transform = pass.plot_transform;
    let construction_bounds = pass.construction_bounds;
    let document_path = pass.document_path;
    let pdf = &mut *pass.pdf;
    let content = &mut *pass.content;
    let xobjects = &mut *pass.xobjects;
    for e in entities {
        let p = |x: f64, y: f64| {
            if let Some(v) = active_viewport {
                let q = v.map([x, y]);
                let mapped = plot_transform.map(q);
                (mapped[0], mapped[1])
            } else {
                let mapped = plot_transform.map([x, y]);
                (mapped[0], mapped[1])
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
            Entity::Polyline3(pl) => {
                for segment in pl.segments() {
                    let a = segment.start();
                    let z = segment.end();
                    let (x1, y1) = p(a.x(), a.y());
                    let (x2, y2) = p(z.x(), z.y());
                    content.push_str(&format!("{x1} {y1} m {x2} {y2} l S\n"));
                }
            }
            Entity::Circle(c) => append_curve(
                content,
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
                content,
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
                content,
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
                    content,
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
            Entity::Mesh(mesh) => {
                for face in mesh.faces() {
                    let (vs, n) = face.vertices();
                    let (x, y) = p(vs[0].x(), vs[0].y());
                    content.push_str(&format!("{x} {y} m "));
                    for q in &vs[1..n] {
                        let (x, y) = p(q.x(), q.y());
                        content.push_str(&format!("{x} {y} l "));
                    }
                    content.push_str("h f\n");
                }
            }
            Entity::Text(t) => append_text(content, t.content(), t.location(), &p),
            Entity::MText(t) => append_text(content, t.text().content(), t.text().location(), &p),
            Entity::Leader(l) => {
                append_lines(content, l.vertices().windows(2).map(|v| (v[0], v[1])), &p)
            }
            Entity::MLeader(l) => {
                append_lines(
                    content,
                    l.geometry()
                        .unwrap_or_else(|_| l.segments())
                        .into_iter()
                        .map(|line| (line.start(), line.end())),
                    &p,
                );
                match l.content() {
                    crate::MLeaderContent::MText(text) => {
                        append_text(content, text.text().content(), text.text().location(), &p)
                    }
                    crate::MLeaderContent::Block(insert) => {
                        append_text(content, insert.block_name(), insert.insertion(), &p)
                    }
                }
            }
            Entity::Table(t) => {
                let display_cells = t.display_cells();
                append_lines(
                    content,
                    t.grid_lines()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .map(|l| (l.start(), l.end())),
                    &p,
                );
                let mut y = t.anchor().y();
                for (row_height, cells) in t.row_heights().iter().zip(&display_cells) {
                    let mut x = t.anchor().x();
                    for (cell, width) in cells.iter().zip(t.column_widths()) {
                        append_text(
                            content,
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
                            append_lines(content, std::iter::once((l.start(), l.end())), &p)
                        }
                        crate::DimensionPrimitive::Arc(a) => append_curve(
                            content,
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
                            append_text(content, t.content(), t.location(), &p)
                        }
                    }
                }
            }
            Entity::Hatch(h) => {
                append_hatch(content, h.boundaries(), &p);
            }
            Entity::XLine(line) => append_construction(
                content,
                line.origin(),
                line.direction(),
                construction_bounds,
                false,
                &p,
            ),
            Entity::Ray(ray) => append_construction(
                content,
                ray.origin(),
                ray.direction(),
                construction_bounds,
                true,
                &p,
            ),
            Entity::Image(image) => {
                if !image.visible() {
                    continue;
                }
                append_raster_plot_image(pdf, content, xobjects, image, document_path, &p)?;
            }
            Entity::PdfUnderlay(underlay) => {
                append_pdf_underlay(pdf, content, xobjects, underlay, document_path, &p)?;
            }
            Entity::MLine(mline) => {
                for path in mline.explode().map_err(|e| e.to_string())? {
                    append_lines(content, path.windows(2).map(|pair| (pair[0], pair[1])), &p);
                }
                append_lines(
                    content,
                    mline
                        .cap_lines()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .map(|line| (line.start(), line.end())),
                    &p,
                );
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
    Ok(())
}

fn append_pdf_underlay(
    pdf: &mut PdfWriter,
    content: &mut String,
    xobjects: &mut Vec<PdfImageResource>,
    underlay: &crate::PdfUnderlay,
    document_path: Option<&Path>,
    map: &impl Fn(f64, f64) -> (f64, f64),
) -> Result<(), String> {
    if !underlay.display_enabled() {
        return Ok(());
    }
    let path = resolve_reference_path(underlay.path(), document_path);
    let data = read_bounded(&path)?;
    let mut placement = underlay.clone();
    if !placement.page_size_known() {
        let info = crate::pdf_info(&data)
            .map_err(|error| format!("Cannot resolve PDF underlay {}: {error}", path.display()))?;
        let page = placement
            .page()
            .checked_sub(1)
            .and_then(|index| info.pages.get(index as usize))
            .ok_or_else(|| {
                format!(
                    "PDF underlay page {} is unavailable in {}",
                    placement.page(),
                    path.display()
                )
            })?;
        placement
            .set_page_size([f64::from(page.width_points), f64::from(page.height_points)])
            .map_err(|error| format!("Invalid PDF underlay dimensions: {error}"))?;
    }
    let mut preview = crate::render_pdf_page(&data, placement.page() as usize)
        .map_err(|error| format!("Cannot render PDF underlay {}: {error}", path.display()))?;
    crate::pdf_render::adjust_preview(
        &mut preview,
        placement.fade(),
        placement.contrast(),
        placement.monochrome(),
    )
    .map_err(|error| format!("Cannot adjust PDF underlay pixels: {error}"))?;
    let resource = pdf.add_rgba_image(preview.width, preview.height, &preview.pixels)?;
    xobjects.push(resource.clone());
    let [page_width, page_height] = placement
        .resolved_page_size()
        .map_err(|error| format!("PDF underlay dimensions unavailable: {error}"))?;
    let [scale_x, scale_y] = placement.scale();
    let (sin, cos) = placement.rotation().sin_cos();
    let insertion = placement.insertion();
    let full_u = crate::Point3::new(cos * page_width * scale_x, sin * page_width * scale_x, 0.0)
        .map_err(|error| format!("Invalid PDF underlay placement: {error}"))?;
    let full_v = crate::Point3::new(
        -sin * page_height * scale_y,
        cos * page_height * scale_y,
        0.0,
    )
    .map_err(|error| format!("Invalid PDF underlay placement: {error}"))?;
    let mut raster = crate::RasterImage::new(
        path,
        insertion,
        full_u,
        full_v,
        preview.width,
        preview.height,
    )
    .map_err(|error| format!("Invalid PDF underlay image geometry: {error}"))?;
    if placement.clip_enabled() {
        let polygon = placement
            .clipped_polygon()
            .map_err(|error| format!("Invalid PDF underlay clip: {error}"))?;
        raster
            .set_clip_boundary(Some(polygon))
            .map_err(|error| format!("Invalid PDF underlay clip: {error}"))?;
    }
    append_underlay_image(content, &raster, resource, map)?;
    if placement.frame_mode() == 1 {
        let frame = placement
            .clipped_polygon()
            .map_err(|error| format!("Invalid PDF underlay frame: {error}"))?;
        let mut edges = frame
            .windows(2)
            .map(|pair| (pair[0], pair[1]))
            .collect::<Vec<_>>();
        if let (Some(first), Some(last)) = (frame.first(), frame.last()) {
            edges.push((*last, *first));
        }
        append_lines(content, edges, map);
    }
    Ok(())
}

fn append_raster_plot_image(
    pdf: &mut PdfWriter,
    content: &mut String,
    xobjects: &mut Vec<PdfImageResource>,
    image: &crate::RasterImage,
    document_path: Option<&Path>,
    map: &impl Fn(f64, f64) -> (f64, f64),
) -> Result<(), String> {
    let path = resolve_reference_path(image.path(), document_path);
    let (width, height, pixels) = load_png_pixels(&path)?;
    let pixels = with_opacity(pixels, image.opacity())?;
    let resource = pdf.add_rgba_image(width, height, &pixels)?;
    xobjects.push(resource.clone());
    append_raster_image(content, image, resource, map)
}

struct PreparedPdfEntities {
    entities: Vec<Entity>,
    bounds: Option<[f64; 4]>,
}

fn plot_source_bounds(entity_bounds: [f64; 4], viewports: &[Viewport]) -> [f64; 4] {
    let Some(first) = viewports.first() else {
        return entity_bounds;
    };
    viewports
        .iter()
        .skip(1)
        .fold(first.paper(), |bounds, viewport| {
            let paper = viewport.paper();
            [
                bounds[0].min(paper[0]),
                bounds[1].min(paper[1]),
                bounds[2].max(paper[2]),
                bounds[3].max(paper[3]),
            ]
        })
}

fn union_bounds(left: [f64; 4], right: [f64; 4]) -> [f64; 4] {
    [
        left[0].min(right[0]),
        left[1].min(right[1]),
        left[2].max(right[2]),
        left[3].max(right[3]),
    ]
}

fn transformed_rect(transform: PlotTransform, rect: [f64; 4]) -> [f64; 4] {
    let a = transform.map([rect[0], rect[1]]);
    let b = transform.map([rect[2], rect[3]]);
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[0].max(b[0]),
        a[1].max(b[1]),
    ]
}

fn svg_body_transform(
    plot_transform: PlotTransform,
    bounds: [f64; 4],
    viewport: Option<&Viewport>,
    page_height: f64,
) -> String {
    let (scale, translation) = if let Some(viewport) = viewport {
        let world = viewport.world();
        let paper = viewport.paper();
        let scale = plot_transform.scale() * viewport.scale();
        let translation = [
            plot_transform.translation()[0]
                + plot_transform.scale()
                    * (paper[0] + viewport.scale() * (bounds[0] - 10.0 - world[0])),
            page_height
                - plot_transform.translation()[1]
                - plot_transform.scale()
                    * (paper[1] + viewport.scale() * (bounds[3] + 10.0 - world[1])),
        ];
        (scale, translation)
    } else {
        let scale = plot_transform.scale();
        let translation = [
            plot_transform.translation()[0] + scale * (bounds[0] - 10.0),
            page_height - plot_transform.translation()[1] - scale * (bounds[3] + 10.0),
        ];
        (scale, translation)
    };
    format!(
        "translate({} {}) scale({scale} {scale})",
        translation[0], translation[1]
    )
}

fn prepare_pdf_entities(
    document: &DxfDocument,
    document_path: Option<&Path>,
    layout_name: Option<&str>,
) -> Result<PreparedPdfEntities, String> {
    let mut entities = flatten_entities(document, layout_name)?;
    entities.retain(|entity| match entity {
        Entity::Image(image) => image.visible(),
        Entity::PdfUnderlay(underlay) => underlay.display_enabled(),
        _ => true,
    });
    resolve_underlay_sizes(&mut entities, document_path)?;
    let bounds = if entities.is_empty() {
        None
    } else {
        Some(crate::layout::world_bounds(&entities).map_err(|e| e.to_string())?)
    };
    Ok(PreparedPdfEntities { entities, bounds })
}

fn flatten_entities(
    document: &DxfDocument,
    layout_name: Option<&str>,
) -> Result<Vec<Entity>, String> {
    let mut entities = Vec::new();
    for (index, entity) in document.entities().iter().enumerate() {
        let in_scope = document
            .entity_space(index)
            .is_none_or(|space| match layout_name {
                Some(name) => space.matches_layout(name),
                None => space.matches_layout("Model"),
            });
        let visible = document
            .layer_for_entity(index)
            .is_none_or(|layer| layer.is_visible());
        if !in_scope || !visible {
            continue;
        }
        let expanded = crate::block_expansion::expand_entity(document, entity)
            .map_err(|error| error.to_string())?;
        entities.extend(expanded);
    }
    Ok(entities)
}

fn resolve_underlay_sizes(
    entities: &mut [Entity],
    document_path: Option<&Path>,
) -> Result<(), String> {
    for entity in entities {
        let Entity::PdfUnderlay(underlay) = entity else {
            continue;
        };
        if underlay.page_size_known() {
            continue;
        }
        let path = resolve_reference_path(underlay.path(), document_path);
        let data = read_bounded(&path)?;
        let info = crate::pdf_info(&data)
            .map_err(|error| format!("Cannot resolve PDF underlay {}: {error}", path.display()))?;
        let page = underlay
            .page()
            .checked_sub(1)
            .and_then(|index| info.pages.get(index as usize))
            .ok_or_else(|| {
                format!(
                    "PDF underlay page {} is unavailable in {}",
                    underlay.page(),
                    path.display()
                )
            })?;
        underlay
            .set_page_size([f64::from(page.width_points), f64::from(page.height_points)])
            .map_err(|error| format!("Invalid PDF underlay dimensions: {error}"))?;
    }
    Ok(())
}

#[derive(Clone)]
struct PdfImageResource {
    name: String,
    object_id: usize,
}

struct PdfObject {
    dictionary: String,
    stream: Option<Vec<u8>>,
}

struct PdfPage {
    width: f64,
    height: f64,
    content: String,
    xobjects: String,
}

struct PdfWriter {
    objects: Vec<Option<PdfObject>>,
    next_image: usize,
}

impl PdfWriter {
    fn new() -> Self {
        Self {
            objects: (0..3).map(|_| None).collect(),
            next_image: 1,
        }
    }

    fn add_object(&mut self, object: PdfObject) -> usize {
        self.objects.push(Some(object));
        self.objects.len()
    }

    fn add_rgba_image(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<PdfImageResource, String> {
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .filter(|pixels| *pixels > 0 && *pixels <= MAX_PLOT_IMAGE_PIXELS)
            .ok_or("Plot image dimensions exceed the supported limit.")?;
        let byte_count = usize::try_from(pixels)
            .ok()
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Plot image dimensions exceed the supported limit.")?;
        if rgba.len() != byte_count {
            return Err("Plot image pixel data has an invalid length.".into());
        }
        let rgb_byte_count = usize::try_from(pixels)
            .ok()
            .and_then(|pixels| pixels.checked_mul(3))
            .ok_or("Plot image dimensions exceed the supported limit.")?;
        let mut rgb = Vec::with_capacity(rgb_byte_count);
        let mut alpha = Vec::with_capacity(pixels as usize);
        let mut has_alpha = false;
        for pixel in rgba.chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
            alpha.push(pixel[3]);
            has_alpha |= pixel[3] != 255;
        }
        let soft_mask = if has_alpha {
            Some(self.add_object(PdfObject {
                dictionary: format!(
                    "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>",
                    alpha.len()
                ),
                stream: Some(alpha),
            }))
        } else {
            None
        };
        let mut dictionary = format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {}",
            rgb.len()
        );
        if let Some(mask) = soft_mask {
            dictionary.push_str(&format!(" /SMask {mask} 0 R"));
        }
        dictionary.push_str(" >>");
        let object_id = self.add_object(PdfObject {
            dictionary,
            stream: Some(rgb),
        });
        let name = format!("Im{}", self.next_image);
        self.next_image += 1;
        Ok(PdfImageResource { name, object_id })
    }

    fn finish_pages(mut self, pages: Vec<PdfPage>) -> Result<Vec<u8>, String> {
        self.assemble_page_objects(pages);
        self.serialize()
    }

    fn assemble_page_objects(&mut self, pages: Vec<PdfPage>) {
        self.objects[0] = Some(PdfObject::plain("<< /Type /Catalog /Pages 2 0 R >>"));
        self.objects[2] = Some(PdfObject::plain(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        ));
        let mut kids = Vec::with_capacity(pages.len());
        for page in pages {
            let content_id = self.add_object(PdfObject::stream(page.content.into_bytes()));
            let page_id = self.add_object(PdfObject::plain(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> /XObject << {} >> >> /Contents {} 0 R >>",
                page.width, page.height, page.xobjects, content_id
            )));
            kids.push(format!("{page_id} 0 R"));
        }
        self.objects[1] = Some(PdfObject::plain(format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            kids.len()
        )));
    }

    fn serialize(self) -> Result<Vec<u8>, String> {
        let mut output = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len() + 1);
        offsets.push(0usize);
        for (index, object) in self.objects.into_iter().enumerate() {
            let object = object.ok_or("PDF object writer left an unresolved object.")?;
            offsets.push(output.len());
            output.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            output.extend_from_slice(object.dictionary.as_bytes());
            if let Some(stream) = object.stream {
                output.extend_from_slice(b"\nstream\n");
                output.extend_from_slice(&stream);
                output.extend_from_slice(b"\nendstream");
            }
            output.extend_from_slice(b"\nendobj\n");
        }
        let xref_offset = output.len();
        output.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
        for offset in offsets.iter().skip(1) {
            if *offset > 9_999_999_999 {
                return Err("PDF output exceeds the supported xref size.".into());
            }
            output.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        output.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        Ok(output)
    }
}

impl PdfObject {
    fn plain(dictionary: impl Into<String>) -> Self {
        Self {
            dictionary: dictionary.into(),
            stream: None,
        }
    }

    fn stream(bytes: Vec<u8>) -> Self {
        Self {
            dictionary: format!("<< /Length {} >>", bytes.len()),
            stream: Some(bytes),
        }
    }
}

fn resolve_reference_path(path: &Path, document_path: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        return path.to_owned();
    }
    document_path
        .and_then(Path::parent)
        .map_or_else(|| path.to_owned(), |parent| parent.join(path))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("Cannot read plot reference {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(MAX_PLOT_REFERENCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read plot reference {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_PLOT_REFERENCE_BYTES {
        return Err(format!(
            "Plot reference {} exceeds the supported file size.",
            path.display()
        ));
    }
    Ok(bytes)
}

fn load_png_pixels(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = read_bounded(path)?;
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_PLOT_IMAGE_PIXELS as u32);
    limits.max_image_height = Some(MAX_PLOT_IMAGE_PIXELS as u32);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|error| {
        format!(
            "Cannot decode PNG plot reference {}: {error}",
            path.display()
        )
    })?;
    let width = decoded.width();
    let height = decoded.height();
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .filter(|pixels| *pixels > 0 && *pixels <= MAX_PLOT_IMAGE_PIXELS)
        .ok_or_else(|| {
            format!(
                "PNG plot reference {} exceeds supported dimensions.",
                path.display()
            )
        })?;
    let rgba = decoded.to_rgba8().into_raw();
    let expected = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            format!(
                "PNG plot reference {} exceeds supported dimensions.",
                path.display()
            )
        })?;
    if rgba.len() != expected {
        return Err(format!(
            "PNG plot reference {} has invalid pixel data.",
            path.display()
        ));
    }
    Ok((width, height, rgba))
}

fn with_opacity(mut pixels: Vec<u8>, opacity: f64) -> Result<Vec<u8>, String> {
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err("Raster image opacity is invalid.".into());
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = (f64::from(pixel[3]) * opacity).round() as u8;
    }
    Ok(pixels)
}

fn append_raster_image(
    content: &mut String,
    image: &crate::RasterImage,
    resource: PdfImageResource,
    map: &impl Fn(f64, f64) -> (f64, f64),
) -> Result<(), String> {
    append_clipped_image(content, image, resource, map)
}

fn append_underlay_image(
    content: &mut String,
    image: &crate::RasterImage,
    resource: PdfImageResource,
    map: &impl Fn(f64, f64) -> (f64, f64),
) -> Result<(), String> {
    append_clipped_image(content, image, resource, map)
}

fn append_clipped_image(
    content: &mut String,
    image: &crate::RasterImage,
    resource: PdfImageResource,
    map: &impl Fn(f64, f64) -> (f64, f64),
) -> Result<(), String> {
    let triangles = crate::raster_geometry::clipped_triangles(
        image.insertion(),
        image.u_vector(),
        image.v_vector(),
        image.clip_boundary(),
    )
    .map_err(|error| format!("Cannot clip raster plot image: {error}"))?;
    let insertion = image.insertion();
    let u_end = add_point(insertion, image.u_vector())?;
    let v_end = add_point(insertion, image.v_vector())?;
    let origin = map(insertion.x(), insertion.y());
    let u = map(u_end.x(), u_end.y());
    let v = map(v_end.x(), v_end.y());
    let a = u.0 - origin.0;
    let b = u.1 - origin.1;
    let c = v.0 - origin.0;
    let d = v.1 - origin.1;
    let (c, d, e, f) = (c, d, origin.0, origin.1);
    for triangle in triangles {
        let points = triangle
            .positions
            .iter()
            .map(|point| map(point.x(), point.y()))
            .collect::<Vec<_>>();
        content.push_str("q ");
        content.push_str(&format!("{} {} m ", points[0].0, points[0].1));
        content.push_str(&format!(
            "{} {} l {} {} l h W n ",
            points[1].0, points[1].1, points[2].0, points[2].1
        ));
        content.push_str(&format!(
            "{a} {b} {c} {d} {e} {f} cm /{} Do Q\n",
            resource.name
        ));
    }
    Ok(())
}

fn add_point(point: crate::Point3, vector: crate::Point3) -> Result<crate::Point3, String> {
    crate::Point3::new(
        point.x() + vector.x(),
        point.y() + vector.y(),
        point.z() + vector.z(),
    )
    .map_err(|error| format!("Invalid raster image placement: {error}"))
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
