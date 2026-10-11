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
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDrawing => write!(f, "drawing has no entities"),
            Self::BoundsTooLarge => write!(f, "drawing bounds are too large for SVG coordinates"),
            Self::Linetype(error) => error.fmt(f),
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
    let (entities, colors): (Vec<_>, Vec<_>) = document
        .entities()
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| {
            let layer = document
                .layer_for_entity(index)
                .expect("entity layer exists");
            layer.is_visible().then_some((
                entity.clone(),
                document
                    .entity_rgb(index, true)
                    .expect("entity style exists"),
            ))
        })
        .unzip();
    let patterns = document
        .entities()
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            document
                .layer_for_entity(*index)
                .is_some_and(crate::Layer::is_visible)
        })
        .map(|(index, entity)| {
            document
                .linetype_for_entity(index)
                .expect("effective linetype")
                .parts(
                    entity,
                    document
                        .effective_linetype_scale(index)
                        .expect("entity style exists"),
                )
                .map_err(SvgError::Linetype)
        })
        .collect::<Result<Vec<_>, _>>()?;
    render_styled_entities_svg(&entities, Some(&colors), Some(&patterns))
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
            }
        }
    }
    svg.push_str("  </g>\n</svg>\n");
    Ok(svg)
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
