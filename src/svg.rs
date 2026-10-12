use crate::{Drawing, DxfDocument, Entity, LinetypeError, linetype::PatternPart};
use std::error::Error;
use std::fmt;
use std::fmt::Write;

const PADDING: f64 = 10.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvgError {
    EmptyDrawing,
    BoundsTooLarge,
    Linetype(LinetypeError),
    UnresolvedInsert,
    BlockExpansion(String),
    UnsupportedPdfUnderlay,
    UnresolvedPdfUnderlay,
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDrawing => write!(f, "drawing has no entities"),
            Self::BoundsTooLarge => write!(f, "drawing bounds are too large for SVG coordinates"),
            Self::Linetype(error) => error.fmt(f),
            Self::UnresolvedInsert => {
                write!(f, "INSERT requires a document block definition registry")
            }
            Self::BlockExpansion(error) => write!(f, "cannot evaluate block reference: {error}"),
            Self::UnsupportedPdfUnderlay => {
                write!(f, "SVG export cannot embed PDF underlays")
            }
            Self::UnresolvedPdfUnderlay => {
                write!(f, "PDF underlay page dimensions are unresolved")
            }
        }
    }
}

impl Error for SvgError {}

pub(crate) fn entity_bounds(entities: &[Entity]) -> Result<[f64; 4], SvgError> {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for entity in entities {
        let extent = match entity {
            Entity::Line(line) => [
                line.start().x().min(line.end().x()),
                line.start().y().min(line.end().y()),
                line.start().x().max(line.end().x()),
                line.start().y().max(line.end().y()),
            ],
            Entity::Circle(circle) => [
                circle.center().x() - circle.radius(),
                circle.center().y() - circle.radius(),
                circle.center().x() + circle.radius(),
                circle.center().y() + circle.radius(),
            ],
            Entity::Arc(arc) => {
                let points = arc.extrema().map_err(|_| SvgError::BoundsTooLarge)?;
                let mut extent = [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ];
                for point in points {
                    extent[0] = extent[0].min(point.x());
                    extent[1] = extent[1].min(point.y());
                    extent[2] = extent[2].max(point.x());
                    extent[3] = extent[3].max(point.y());
                }
                extent
            }
            Entity::Polyline(polyline) => polyline
                .extrema()
                .map_err(|_| SvgError::BoundsTooLarge)?
                .iter()
                .fold(
                    [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |extent, point| {
                        [
                            extent[0].min(point.x()),
                            extent[1].min(point.y()),
                            extent[2].max(point.x()),
                            extent[3].max(point.y()),
                        ]
                    },
                ),
            Entity::Polyline3(polyline) => polyline.vertices().iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |extent, point| {
                    [
                        extent[0].min(point.x()),
                        extent[1].min(point.y()),
                        extent[2].max(point.x()),
                        extent[3].max(point.y()),
                    ]
                },
            ),
            Entity::Point(point) => [
                point.position().x(),
                point.position().y(),
                point.position().x(),
                point.position().y(),
            ],
            Entity::XLine(_) | Entity::Ray(_) => continue,
            Entity::Image(image) => {
                let b = image.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::PdfUnderlay(underlay) => {
                let b = underlay.bounds().map_err(|error| {
                    if matches!(error, crate::GeometryError::UnresolvedReference) {
                        SvgError::UnresolvedPdfUnderlay
                    } else {
                        SvgError::BoundsTooLarge
                    }
                })?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::MLine(mline) => mline.bounds().map_err(|_| SvgError::BoundsTooLarge)?,
            Entity::Ellipse(ellipse) => {
                let points = (0..=64)
                    .filter_map(|i| ellipse.point_at_fraction(i as f64 / 64.0).ok())
                    .collect::<Vec<_>>();
                points.iter().fold(
                    [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |b, p| {
                        [
                            b[0].min(p.x()),
                            b[1].min(p.y()),
                            b[2].max(p.x()),
                            b[3].max(p.y()),
                        ]
                    },
                )
            }
            Entity::Spline(spline) => spline
                .tessellate(64)
                .map_err(|_| SvgError::BoundsTooLarge)?
                .iter()
                .fold(
                    [
                        f64::INFINITY,
                        f64::INFINITY,
                        f64::NEG_INFINITY,
                        f64::NEG_INFINITY,
                    ],
                    |b, p| {
                        [
                            b[0].min(p.x()),
                            b[1].min(p.y()),
                            b[2].max(p.x()),
                            b[3].max(p.y()),
                        ]
                    },
                ),
            Entity::Text(text) => {
                let b = text.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::MText(text) => {
                let b = text.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::Leader(leader) => {
                let b = leader.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::MLeader(leader) => {
                let b = leader.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::Table(table) => {
                let b = table.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::Dimension(dimension) => {
                let b = dimension.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::Hatch(hatch) => {
                let b = hatch.bounds().map_err(|_| SvgError::BoundsTooLarge)?;
                [b.min.x(), b.min.y(), b.max.x(), b.max.y()]
            }
            Entity::Insert(insert) => {
                let p = insert.insertion();
                [p.x(), p.y(), p.x(), p.y()]
            }
            Entity::Face3(face) => {
                let (points, count) = face.vertices();
                let points = &points[..count];
                [
                    points.iter().map(|p| p.x()).fold(f64::INFINITY, f64::min),
                    points.iter().map(|p| p.y()).fold(f64::INFINITY, f64::min),
                    points
                        .iter()
                        .map(|p| p.x())
                        .fold(f64::NEG_INFINITY, f64::max),
                    points
                        .iter()
                        .map(|p| p.y())
                        .fold(f64::NEG_INFINITY, f64::max),
                ]
            }
            Entity::Mesh(mesh) => {
                let points = mesh
                    .faces()
                    .iter()
                    .flat_map(|face| {
                        let (vertices, count) = face.vertices();
                        vertices[..count].to_vec()
                    })
                    .collect::<Vec<_>>();
                [
                    points.iter().map(|p| p.x()).fold(f64::INFINITY, f64::min),
                    points.iter().map(|p| p.y()).fold(f64::INFINITY, f64::min),
                    points
                        .iter()
                        .map(|p| p.x())
                        .fold(f64::NEG_INFINITY, f64::max),
                    points
                        .iter()
                        .map(|p| p.y())
                        .fold(f64::NEG_INFINITY, f64::max),
                ]
            }
        };
        if extent.iter().any(|value| !value.is_finite()) {
            return Err(SvgError::BoundsTooLarge);
        }
        bounds[0] = bounds[0].min(extent[0]);
        bounds[1] = bounds[1].min(extent[1]);
        bounds[2] = bounds[2].max(extent[2]);
        bounds[3] = bounds[3].max(extent[3]);
    }
    if entities.is_empty() {
        return Err(SvgError::EmptyDrawing);
    }
    if !(bounds[2] - bounds[0]).is_finite() || !(bounds[3] - bounds[1]).is_finite() {
        return Err(SvgError::BoundsTooLarge);
    }
    Ok(bounds)
}

/// Export supported 3D entities as an XY projection, with positive Y upward.
pub fn render_entities_svg(entities: &[Entity]) -> Result<String, SvgError> {
    render_styled_entities_svg(entities, None, None)
}

/// Export visible document entities with resolved styles on a white background.
pub fn render_document_svg(document: &DxfDocument) -> Result<String, SvgError> {
    let mut entities = Vec::new();
    let mut colors = Vec::new();
    let mut parents = Vec::new();
    for (index, entity) in document.entities().iter().enumerate() {
        let layer = document
            .layer_for_entity(index)
            .expect("entity layer exists");
        if !layer.is_visible() {
            continue;
        }
        let expanded = expand_document_entity(document, entity)?;
        let color = document
            .entity_rgb(index, true)
            .expect("entity style exists");
        for child in expanded {
            entities.push(child);
            colors.push(color);
            parents.push(index);
        }
    }
    let patterns = entities
        .iter()
        .zip(parents.iter())
        .map(|(entity, index)| {
            document
                .linetype_for_entity(*index)
                .expect("effective linetype")
                .parts(
                    entity,
                    document
                        .effective_linetype_scale(*index)
                        .expect("entity style exists"),
                )
                .map_err(SvgError::Linetype)
        })
        .collect::<Result<Vec<_>, _>>()?;
    render_styled_entities_svg(&entities, Some(&colors), Some(&patterns))
}

fn expand_document_entity(
    document: &DxfDocument,
    entity: &Entity,
) -> Result<Vec<Entity>, SvgError> {
    crate::block_expansion::expand_entity(document, entity)
        .map_err(|error| SvgError::BlockExpansion(error.to_string()))
}

fn render_styled_entities_svg(
    entities: &[Entity],
    colors: Option<&[[u8; 3]]>,
    patterns: Option<&[Vec<PatternPart>]>,
) -> Result<String, SvgError> {
    let [min_x, min_y, max_x, max_y] = entity_bounds(entities)?;
    let width = max_x - min_x + 2.0 * PADDING;
    let height = max_y - min_y + 2.0 * PADDING;
    if !width.is_finite() || !height.is_finite() {
        return Err(SvgError::BoundsTooLarge);
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"CAD drawing, XY projection\">\n"
    );
    if colors.is_some() {
        svg.push_str("  <rect width=\"100%\" height=\"100%\" fill=\"white\" />\n");
    }
    svg.push_str("  <g fill=\"none\" stroke=\"black\" stroke-width=\"1\">\n");
    for (index, entity) in entities.iter().enumerate() {
        let color = colors
            .map(|colors| {
                let [r, g, b] = colors[index];
                format!("#{r:02x}{g:02x}{b:02x}")
            })
            .unwrap_or_else(|| "black".into());
        let style = if colors.is_some() {
            format!(" stroke=\"{color}\"")
        } else {
            String::new()
        };
        let solid = [PatternPart::Geometry(entity.clone())];
        for part in patterns.map_or(solid.as_slice(), |patterns| patterns[index].as_slice()) {
            let entity = match part {
                PatternPart::Geometry(entity) => entity,
                PatternPart::Dot(point) => {
                    let cx = point.x() - min_x + PADDING;
                    let cy = max_y - point.y() + PADDING;
                    writeln!(svg, "    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"0.5\" fill=\"{color}\" stroke=\"none\" />").expect("String write");
                    continue;
                }
            };
            match entity {
                Entity::Point(point) => {
                    let cx = point.position().x() - min_x + PADDING;
                    let cy = max_y - point.position().y() + PADDING;
                    writeln!(svg, "    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"1\" fill=\"{color}\" stroke=\"none\" />").expect("String write");
                }
                Entity::XLine(line) => render_construction(
                    &mut svg,
                    line.origin(),
                    line.direction(),
                    min_x,
                    max_y,
                    [min_x, min_y, max_x, max_y],
                    false,
                    style.as_str(),
                ),
                Entity::Ray(ray) => render_construction(
                    &mut svg,
                    ray.origin(),
                    ray.direction(),
                    min_x,
                    max_y,
                    [min_x, min_y, max_x, max_y],
                    true,
                    style.as_str(),
                ),
                Entity::Image(image) => render_image(&mut svg, image, min_x, max_y),
                Entity::PdfUnderlay(_) => return Err(SvgError::UnsupportedPdfUnderlay),
                Entity::MLine(mline) => {
                    for path in mline.explode().map_err(|_| SvgError::BoundsTooLarge)? {
                        for pair in path.windows(2) {
                            render_line(
                                &mut svg,
                                crate::Line3::new(pair[0], pair[1]),
                                min_x,
                                max_y,
                                style.as_str(),
                            );
                        }
                    }
                    for line in mline.cap_lines().map_err(|_| SvgError::BoundsTooLarge)? {
                        render_line(&mut svg, line, min_x, max_y, style.as_str());
                    }
                }
                Entity::Line(line) => {
                    let x1 = line.start().x() - min_x + PADDING;
                    let y1 = max_y - line.start().y() + PADDING;
                    let x2 = line.end().x() - min_x + PADDING;
                    let y2 = max_y - line.end().y() + PADDING;
                    writeln!(
                        svg,
                        "    <line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"{style} />"
                    )
                    .expect("String write");
                }
                Entity::Circle(circle) => {
                    let cx = circle.center().x() - min_x + PADDING;
                    let cy = max_y - circle.center().y() + PADDING;
                    writeln!(
                        svg,
                        "    <circle cx=\"{cx}\" cy=\"{cy}\" r=\"{}\"{style} />",
                        circle.radius()
                    )
                    .expect("String write");
                }
                Entity::Arc(arc) => {
                    let start = arc.start_point().map_err(|_| SvgError::BoundsTooLarge)?;
                    let end = arc.end_point().map_err(|_| SvgError::BoundsTooLarge)?;
                    let x1 = start.x() - min_x + PADDING;
                    let y1 = max_y - start.y() + PADDING;
                    let x2 = end.x() - min_x + PADDING;
                    let y2 = max_y - end.y() + PADDING;
                    let large = u8::from(arc.sweep() > std::f64::consts::PI);
                    writeln!(
                        svg,
                        "    <path d=\"M {x1} {y1} A {} {} 0 {large} 0 {x2} {y2}\"{style} />",
                        arc.radius(),
                        arc.radius(),
                    )
                    .expect("String write");
                }
                Entity::Polyline(polyline) => {
                    if polyline.has_active_width() {
                        for quad in polyline
                            .width_quads_xy()
                            .map_err(|_| SvgError::BoundsTooLarge)?
                        {
                            let mut points = String::new();
                            for point in quad {
                                write!(
                                    points,
                                    "{},{} ",
                                    point.x() - min_x + PADDING,
                                    max_y - point.y() + PADDING
                                )
                                .expect("String write");
                            }
                            writeln!(
                                svg,
                                "    <polygon points=\"{}\" fill=\"{color}\" stroke=\"none\" />",
                                points.trim_end()
                            )
                            .expect("String write");
                        }
                        for (segment, width) in polyline.segments().zip(polyline.widths()) {
                            if width.start() != 0.0 || width.end() != 0.0 {
                                continue;
                            }
                            let segment = segment.map_err(|_| SvgError::BoundsTooLarge)?;
                            let start = segment
                                .point_at_fraction(0.0)
                                .map_err(|_| SvgError::BoundsTooLarge)?;
                            let end = segment
                                .point_at_fraction(1.0)
                                .map_err(|_| SvgError::BoundsTooLarge)?;
                            let x1 = start.x() - min_x + PADDING;
                            let y1 = max_y - start.y() + PADDING;
                            let x2 = end.x() - min_x + PADDING;
                            let y2 = max_y - end.y() + PADDING;
                            match segment {
                                crate::PolylineSegment::Line(_) => writeln!(svg, "    <line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"{style} />"),
                                crate::PolylineSegment::Arc { arc, clockwise } => writeln!(svg, "    <path d=\"M {x1} {y1} A {} {} 0 {} {} {x2} {y2}\"{style} />", arc.radius(), arc.radius(), u8::from(arc.sweep() > std::f64::consts::PI), u8::from(clockwise)),
                            }.expect("String write");
                        }
                        continue;
                    }
                    let count = polyline.vertices().len() - usize::from(!polyline.is_closed());
                    if polyline.bulges()[..count].iter().any(|bulge| *bulge != 0.0) {
                        let first = polyline.vertices()[0];
                        let mut path = format!(
                            "M {} {}",
                            first.x() - min_x + PADDING,
                            max_y - first.y() + PADDING
                        );
                        for (index, segment) in polyline.segments().enumerate() {
                            let end = polyline.vertices()[(index + 1) % polyline.vertices().len()];
                            let x = end.x() - min_x + PADDING;
                            let y = max_y - end.y() + PADDING;
                            match segment.map_err(|_| SvgError::BoundsTooLarge)? {
                                crate::PolylineSegment::Line(_) => write!(path, " L {x} {y}"),
                                crate::PolylineSegment::Arc { arc, clockwise } => write!(
                                    path,
                                    " A {} {} 0 {} {} {x} {y}",
                                    arc.radius(),
                                    arc.radius(),
                                    u8::from(arc.sweep() > std::f64::consts::PI),
                                    u8::from(clockwise)
                                ),
                            }
                            .expect("String write");
                        }
                        if polyline.is_closed() {
                            path.push_str(" Z");
                        }
                        writeln!(svg, "    <path d=\"{path}\"{style} />").expect("String write");
                        continue;
                    }
                    let mut points = String::new();
                    for point in polyline.vertices() {
                        write!(
                            points,
                            "{},{} ",
                            point.x() - min_x + PADDING,
                            max_y - point.y() + PADDING
                        )
                        .expect("String write");
                    }
                    let element = if polyline.is_closed() {
                        "polygon"
                    } else {
                        "polyline"
                    };
                    writeln!(
                        svg,
                        "    <{element} points=\"{}\"{style} />",
                        points.trim_end()
                    )
                    .expect("String write");
                }
                Entity::Polyline3(polyline) => {
                    let mut points = String::new();
                    for point in polyline.vertices() {
                        write!(
                            points,
                            "{},{} ",
                            point.x() - min_x + PADDING,
                            max_y - point.y() + PADDING
                        )
                        .expect("String write");
                    }
                    let element = if polyline.is_closed() {
                        "polygon"
                    } else {
                        "polyline"
                    };
                    writeln!(
                        svg,
                        "    <{element} points=\"{}\"{style} />",
                        points.trim_end()
                    )
                    .expect("String write");
                }
                Entity::Ellipse(ellipse) => {
                    let points = ellipse.tessellate_for_svg();
                    let d = points
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            format!(
                                "{} {} {}",
                                if i == 0 { "M" } else { "L" },
                                p.x() - min_x + PADDING,
                                max_y - p.y() + PADDING
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    writeln!(svg, "    <path d=\"{d}\"{style} />").expect("String write");
                }
                Entity::Spline(spline) => {
                    if let Ok(points) = spline.tessellate(64) {
                        let d = points
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                format!(
                                    "{} {} {}",
                                    if i == 0 { "M" } else { "L" },
                                    p.x() - min_x + PADDING,
                                    max_y - p.y() + PADDING
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        writeln!(svg, "    <path d=\"{d}\"{style} />").expect("String write");
                    }
                }
                Entity::Face3(face) => {
                    let (points, count) = face.vertices();
                    let points = &points[..count];
                    let mut coords = String::new();
                    for p in points {
                        write!(
                            coords,
                            "{},{} ",
                            p.x() - min_x + PADDING,
                            max_y - p.y() + PADDING
                        )
                        .expect("String write");
                    }
                    writeln!(svg, "    <polygon points=\"{}\" fill=\"{color}\" fill-opacity=\"0.2\"{style} />", coords.trim_end()).expect("String write");
                }
                Entity::Mesh(mesh) => {
                    for face in mesh.faces() {
                        let (points, count) = face.vertices();
                        let coords = points[..count]
                            .iter()
                            .map(|p| {
                                format!("{},{}", p.x() - min_x + PADDING, max_y - p.y() + PADDING)
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        writeln!(svg, "    <polygon points=\"{coords}\" fill=\"{color}\" fill-opacity=\"0.2\"{style} />").expect("String write");
                    }
                }
                Entity::Text(text) => {
                    render_text(&mut svg, text, min_x, max_y, color.as_str(), style.as_str())
                }
                Entity::MText(text) => {
                    render_mtext(&mut svg, text, min_x, max_y, color.as_str(), style.as_str())
                }
                Entity::Leader(leader) => {
                    for segment in leader.segments() {
                        render_line(&mut svg, segment, min_x, max_y, style.as_str());
                    }
                    if let Some(label) = leader.annotation()
                        && let Some(point) = leader.vertices().last()
                    {
                        render_label(
                            &mut svg,
                            label,
                            *point,
                            min_x,
                            max_y,
                            2.5,
                            0.0,
                            color.as_str(),
                        );
                    }
                }
                Entity::MLeader(leader) => {
                    render_mleader_geometry(&mut svg, leader, min_x, max_y, style.as_str());
                    match leader.content() {
                        crate::MLeaderContent::MText(text) => render_mtext(
                            &mut svg,
                            text,
                            min_x,
                            max_y,
                            color.as_str(),
                            style.as_str(),
                        ),
                        crate::MLeaderContent::Block(insert) => {
                            let point = insert.insertion();
                            render_label(
                                &mut svg,
                                insert.block_name(),
                                point,
                                min_x,
                                max_y,
                                2.5,
                                0.0,
                                color.as_str(),
                            );
                        }
                    }
                }
                Entity::Table(table) => {
                    let display_cells = table.display_cells();
                    render_table(
                        &mut svg,
                        table,
                        &display_cells,
                        min_x,
                        max_y,
                        color.as_str(),
                        style.as_str(),
                    );
                }
                Entity::Hatch(hatch) => render_hatch(
                    &mut svg,
                    hatch,
                    min_x,
                    max_y,
                    color.as_str(),
                    style.as_str(),
                ),
                Entity::Dimension(dimension) => {
                    for primitive in dimension.geometry().map_err(|_| SvgError::BoundsTooLarge)? {
                        match primitive {
                            crate::DimensionPrimitive::Line(line) => {
                                render_line(&mut svg, line, min_x, max_y, style.as_str())
                            }
                            crate::DimensionPrimitive::Arc(arc) => {
                                let start =
                                    arc.start_point().map_err(|_| SvgError::BoundsTooLarge)?;
                                let end = arc.end_point().map_err(|_| SvgError::BoundsTooLarge)?;
                                let x1 = start.x() - min_x + PADDING;
                                let y1 = max_y - start.y() + PADDING;
                                let x2 = end.x() - min_x + PADDING;
                                let y2 = max_y - end.y() + PADDING;
                                writeln!(svg, "    <path d=\"M {x1} {y1} A {} {} 0 {} 0 {x2} {y2}\"{style} />", arc.radius(),arc.radius(),u8::from(arc.sweep()>std::f64::consts::PI)).expect("String write");
                            }
                            crate::DimensionPrimitive::Text(text) => render_text(
                                &mut svg,
                                &text,
                                min_x,
                                max_y,
                                color.as_str(),
                                style.as_str(),
                            ),
                        }
                    }
                }
                Entity::Insert(_) => return Err(SvgError::UnresolvedInsert),
            }
        }
    }
    svg.push_str("  </g>\n</svg>\n");
    Ok(svg)
}

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn render_mleader_geometry(
    svg: &mut String,
    leader: &crate::MLeader,
    min_x: f64,
    max_y: f64,
    style: &str,
) {
    for branch in leader.branches() {
        for pair in branch.vertices().windows(2) {
            render_line(
                svg,
                crate::Line3::new(pair[0], pair[1]),
                min_x,
                max_y,
                style,
            );
        }
        if let Some(pair) = branch.vertices().windows(2).next() {
            let dx = pair[1].x() - pair[0].x();
            let dy = pair[1].y() - pair[0].y();
            let length = (dx * dx + dy * dy).sqrt();
            if length > f64::EPSILON {
                let ux = dx / length;
                let uy = dy / length;
                let tip = pair[0];
                if let (Ok(left), Ok(right)) = (
                    crate::Point3::new(
                        tip.x() + ux * leader.arrow_size() + uy * leader.arrow_size() * 0.5,
                        tip.y() + uy * leader.arrow_size() - ux * leader.arrow_size() * 0.5,
                        tip.z(),
                    ),
                    crate::Point3::new(
                        tip.x() + ux * leader.arrow_size() - uy * leader.arrow_size() * 0.5,
                        tip.y() + uy * leader.arrow_size() + ux * leader.arrow_size() * 0.5,
                        tip.z(),
                    ),
                ) {
                    render_line(svg, crate::Line3::new(tip, left), min_x, max_y, style);
                    render_line(svg, crate::Line3::new(tip, right), min_x, max_y, style);
                }
            }
        }
        if let Some(pair) = branch.vertices().windows(2).last() {
            let dx = pair[1].x() - pair[0].x();
            let dy = pair[1].y() - pair[0].y();
            let length = (dx * dx + dy * dy).sqrt();
            if length > f64::EPSILON {
                let ux = dx / length;
                let uy = dy / length;
                if let (Ok(start), Ok(end)) = (
                    crate::Point3::new(
                        pair[1].x() + ux * leader.landing_gap(),
                        pair[1].y() + uy * leader.landing_gap(),
                        pair[1].z(),
                    ),
                    crate::Point3::new(
                        pair[1].x() + ux * (leader.landing_gap() + leader.dogleg_length()),
                        pair[1].y() + uy * (leader.landing_gap() + leader.dogleg_length()),
                        pair[1].z(),
                    ),
                ) {
                    render_line(svg, crate::Line3::new(start, end), min_x, max_y, style);
                }
            }
        }
    }
    if leader.text_frame()
        && let crate::MLeaderContent::MText(text) = leader.content()
        && let Ok(bounds) = text.bounds()
    {
        let corners = [
            bounds.min,
            crate::Point3::new(bounds.max.x(), bounds.min.y(), bounds.min.z())
                .unwrap_or(bounds.min),
            bounds.max,
            crate::Point3::new(bounds.min.x(), bounds.max.y(), bounds.max.z())
                .unwrap_or(bounds.max),
        ];
        let points = corners
            .iter()
            .map(|p| format!("{},{}", p.x() - min_x + PADDING, max_y - p.y() + PADDING))
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            svg,
            "    <polygon points=\"{points}\" fill=\"none\"{style} />"
        )
        .expect("String write");
    }
}

fn render_line(svg: &mut String, line: crate::Line3, min_x: f64, max_y: f64, style: &str) {
    let x1 = line.start().x() - min_x + PADDING;
    let y1 = max_y - line.start().y() + PADDING;
    let x2 = line.end().x() - min_x + PADDING;
    let y2 = max_y - line.end().y() + PADDING;
    writeln!(
        svg,
        "    <line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\"{style} />"
    )
    .expect("String write");
}

#[allow(clippy::too_many_arguments)]
fn render_construction(
    svg: &mut String,
    origin: crate::Point3,
    direction: crate::Point3,
    min_x: f64,
    max_y: f64,
    bounds: [f64; 4],
    ray: bool,
    style: &str,
) {
    let min = crate::Point3::new(bounds[0], bounds[1], -1.0e9).expect("finite bounds");
    let max = crate::Point3::new(bounds[2], bounds[3], 1.0e9).expect("finite bounds");
    let clipped = if ray {
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
    if let Some((a, b)) = clipped {
        let line = crate::Line3::new(a, b);
        render_line(svg, line, min_x, max_y, style);
    }
}

fn render_image(svg: &mut String, image: &crate::RasterImage, min_x: f64, max_y: f64) {
    if !image.visible() {
        return;
    }
    let p = image.insertion();
    let u = image.u_vector();
    let v = image.v_vector();
    let x = p.x() - min_x + PADDING;
    let y = max_y - p.y() + PADDING;
    let href = esc(&image.path().to_string_lossy());
    writeln!(svg, "    <image href=\"{href}\" x=\"{x}\" y=\"{y}\" width=\"1\" height=\"1\" opacity=\"{}\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} 0 0)\" />", image.opacity(), u.x(), -u.y(), -v.x(), v.y()).expect("String write");
}

#[allow(clippy::too_many_arguments)]
fn render_label(
    svg: &mut String,
    content: &str,
    p: crate::Point3,
    min_x: f64,
    max_y: f64,
    height: f64,
    rotation: f64,
    color: &str,
) {
    let x = p.x() - min_x + PADDING;
    let y = max_y - p.y() + PADDING;
    writeln!(svg, "    <text x=\"{x}\" y=\"{y}\" fill=\"{color}\" font-size=\"{height}\" transform=\"rotate({} {x} {y})\">{}</text>", -rotation.to_degrees(), esc(content)).expect("String write");
}

fn render_text(
    svg: &mut String,
    text: &crate::Text,
    min_x: f64,
    max_y: f64,
    color: &str,
    _style: &str,
) {
    let (h, v) = text.alignment();
    let anchor = match h {
        crate::HorizontalAlignment::Left => "start",
        crate::HorizontalAlignment::Center => "middle",
        crate::HorizontalAlignment::Right => "end",
    };
    let baseline = match v {
        crate::VerticalAlignment::Bottom => "alphabetic",
        crate::VerticalAlignment::Middle => "middle",
        crate::VerticalAlignment::Top => "hanging",
    };
    let p = text.location();
    let x = p.x() - min_x + PADDING;
    let y = max_y - p.y() + PADDING;
    writeln!(svg, "    <text x=\"{x}\" y=\"{y}\" fill=\"{color}\" font-size=\"{}\" text-anchor=\"{anchor}\" dominant-baseline=\"{baseline}\" transform=\"rotate({} {x} {y}) skewX({}) scale({} 1)\">{}</text>", text.height(), -text.rotation().to_degrees(), -text.oblique().to_degrees(), text.width_factor(), esc(text.content())).expect("String write");
}

fn render_mtext(
    svg: &mut String,
    text: &crate::MText,
    min_x: f64,
    max_y: f64,
    color: &str,
    style: &str,
) {
    render_text(svg, text.text(), min_x, max_y, color, style);
}

fn render_table(
    svg: &mut String,
    table: &crate::Table,
    display_cells: &[Vec<String>],
    min_x: f64,
    max_y: f64,
    color: &str,
    style: &str,
) {
    for line in table.grid_lines().unwrap_or_default() {
        render_line(svg, line, min_x, max_y, style);
    }
    let mut y = table.anchor().y();
    for (r, rh) in table.row_heights().iter().enumerate() {
        let mut x = table.anchor().x();
        for (c, cw) in table.column_widths().iter().enumerate() {
            let p = crate::Point3::new(x + *cw * 0.5, y + *rh * 0.5, table.anchor().z()).unwrap();
            render_label(
                svg,
                display_cells
                    .get(r)
                    .and_then(|row| row.get(c))
                    .map_or("", String::as_str),
                p,
                min_x,
                max_y,
                (*rh * 0.45).max(1.0),
                0.0,
                color,
            );
            x += *cw;
        }
        y += *rh;
    }
}

fn render_hatch(
    svg: &mut String,
    hatch: &crate::Hatch,
    min_x: f64,
    max_y: f64,
    color: &str,
    style: &str,
) {
    match hatch.pattern() {
        crate::HatchPattern::Solid => {
            let mut d = String::new();
            for boundary in hatch.boundaries() {
                if let Some(first) = boundary.first() {
                    write!(
                        d,
                        "M {} {} ",
                        first.x() - min_x + PADDING,
                        max_y - first.y() + PADDING
                    )
                    .expect("String write");
                    for p in &boundary[1..] {
                        write!(
                            d,
                            "L {} {} ",
                            p.x() - min_x + PADDING,
                            max_y - p.y() + PADDING
                        )
                        .expect("String write");
                    }
                    d.push_str("Z ");
                }
            }
            writeln!(
                svg,
                "    <path d=\"{d}\" fill=\"{color}\" fill-rule=\"evenodd\" stroke=\"none\" />"
            )
            .expect("String write");
        }
        crate::HatchPattern::Lines { .. } => {
            for primitive in hatch.primitives().unwrap_or_default() {
                if let crate::HatchPrimitive::Line(line) = primitive {
                    render_line(svg, line, min_x, max_y, style);
                }
            }
        }
    }
}

/// Render a drawing as SVG, mapping Cartesian positive Y upward to SVG Y downward.
pub fn render_svg(drawing: &Drawing) -> Result<String, SvgError> {
    let first = drawing.segments().first().ok_or(SvgError::EmptyDrawing)?;
    let mut min_x = first.start().x();
    let mut max_x = min_x;
    let mut min_y = first.start().y();
    let mut max_y = min_y;

    for segment in drawing.segments() {
        for point in [segment.start(), segment.end()] {
            min_x = min_x.min(point.x());
            max_x = max_x.max(point.x());
            min_y = min_y.min(point.y());
            max_y = max_y.max(point.y());
        }
    }

    let width = (max_x - min_x) + 2.0 * PADDING;
    let height = (max_y - min_y) + 2.0 * PADDING;
    if !width.is_finite() || !height.is_finite() {
        return Err(SvgError::BoundsTooLarge);
    }

    let mut svg = String::new();
    writeln!(svg, "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" role=\"img\" aria-label=\"Line drawing\">").expect("writing to a String cannot fail");
    writeln!(
        svg,
        "  <g fill=\"none\" stroke=\"black\" stroke-width=\"1\">"
    )
    .expect("writing to a String cannot fail");

    for segment in drawing.segments() {
        let x1 = segment.start().x() - min_x + PADDING;
        let y1 = max_y - segment.start().y() + PADDING;
        let x2 = segment.end().x() - min_x + PADDING;
        let y2 = max_y - segment.end().y() + PADDING;
        if [x1, y1, x2, y2].iter().any(|value| !value.is_finite()) {
            return Err(SvgError::BoundsTooLarge);
        }
        writeln!(
            svg,
            "    <line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" />"
        )
        .expect("writing to a String cannot fail");
    }

    svg.push_str("  </g>\n</svg>\n");
    Ok(svg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Point2;

    #[test]
    fn svg_arc_bounds_follow_the_sweep_and_y_flip_keeps_counterclockwise_paths() {
        use crate::{Arc3, Point3};
        use std::f64::consts::FRAC_PI_4;
        let center = Point3::new(0.0, 0.0, 3.0).unwrap();
        let minor = Entity::Arc(Arc3::new(center, 10.0, -FRAC_PI_4, FRAC_PI_4).unwrap());
        let [x1, y1, x2, y2] = entity_bounds(std::slice::from_ref(&minor)).unwrap();
        let diagonal = 50.0_f64.sqrt();
        for (actual, expected) in [(x1, diagonal), (y1, -diagonal), (x2, 10.0), (y2, diagonal)] {
            assert!((actual - expected).abs() < 1e-12);
        }
        assert!(
            render_entities_svg(&[minor])
                .unwrap()
                .contains("A 10 10 0 0 0")
        );
        let major = Entity::Arc(Arc3::new(center, 10.0, FRAC_PI_4, -FRAC_PI_4).unwrap());
        let [x1, y1, x2, y2] = entity_bounds(std::slice::from_ref(&major)).unwrap();
        for (actual, expected) in [(x1, -10.0), (y1, -10.0), (x2, diagonal), (y2, 10.0)] {
            assert!((actual - expected).abs() < 1e-12);
        }
        assert!(
            render_entities_svg(&[major])
                .unwrap()
                .contains("A 10 10 0 1 0")
        );
    }

    #[test]
    fn svg_maps_positive_y_up_and_includes_padding() {
        let mut drawing = Drawing::new();
        drawing.click(Point2::new(0.0, 0.0).unwrap());
        drawing.click(Point2::new(10.0, 20.0).unwrap());

        let svg = render_svg(&drawing).unwrap();
        assert!(svg.contains("viewBox=\"0 0 30 40\""));
        assert!(svg.contains("x1=\"10\" y1=\"30\" x2=\"20\" y2=\"10\""));
    }

    #[test]
    fn svg_rejects_empty_and_overflowing_drawings() {
        assert_eq!(render_svg(&Drawing::new()), Err(SvgError::EmptyDrawing));

        let mut drawing = Drawing::new();
        drawing.click(Point2::new(-f64::MAX, 0.0).unwrap());
        drawing.click(Point2::new(f64::MAX, 0.0).unwrap());
        assert_eq!(render_svg(&drawing), Err(SvgError::BoundsTooLarge));
    }

    #[test]
    fn document_svg_uses_layer_colors_and_only_visible_geometry() {
        let source = include_str!("../tests/fixtures/layers.dxf");
        let mut document = DxfDocument::parse(source).unwrap();
        document.set_layer_color("WALLS", 12).unwrap();
        document.set_layer_color("LOCKED", 8).unwrap();
        let svg = render_document_svg(&document).unwrap();
        assert!(svg.contains("fill=\"white\""));
        assert_eq!(svg.matches("<line ").count(), 3);
        assert!(svg.contains("stroke=\"#a50000\""));
        assert!(svg.contains("stroke=\"#808080\""));
        assert!(svg.contains("stroke=\"#000000\""));
        assert!(!svg.contains("<circle "));
        assert!(!svg.contains("<path "));
        document.set_layer_visible("HIDDEN", true).unwrap();
        document.set_layer_visible("FROZEN", true).unwrap();
        let svg = render_document_svg(&document).unwrap();
        assert!(svg.contains("<circle "));
        assert!(svg.contains("stroke=\"#00ff00\""));
        assert!(svg.contains("<path "));
        assert!(svg.contains("stroke=\"#0000ff\""));
        for name in ["0", "WALLS", "HIDDEN", "FROZEN", "LOCKED"] {
            document.set_layer_visible(name, false).unwrap();
        }
        assert_eq!(render_document_svg(&document), Err(SvgError::EmptyDrawing));
    }

    #[test]
    fn entity_svg_projects_xyz_and_includes_circle_extents() {
        use crate::{Circle3, Line3, Point3};
        let entities = [
            Entity::Line(Line3::new(
                Point3::new(1.0, 2.0, 99.0).unwrap(),
                Point3::new(4.0, 6.0, -99.0).unwrap(),
            )),
            Entity::Circle(Circle3::new(Point3::new(10.0, 20.0, 3.0).unwrap(), 5.0).unwrap()),
        ];
        let svg = render_entities_svg(&entities).unwrap();
        assert!(svg.contains("viewBox=\"0 0 34 43\""));
        assert!(svg.contains("x1=\"10\" y1=\"33\" x2=\"13\" y2=\"29\""));
        assert!(svg.contains("cx=\"19\" cy=\"15\" r=\"5\""));
        assert_eq!(render_entities_svg(&[]), Err(SvgError::EmptyDrawing));
        let overflowing = Entity::Circle(
            Circle3::new(Point3::new(f64::MAX, 0.0, 0.0).unwrap(), f64::MAX).unwrap(),
        );
        assert_eq!(
            render_entities_svg(&[overflowing]),
            Err(SvgError::BoundsTooLarge)
        );
    }
}
