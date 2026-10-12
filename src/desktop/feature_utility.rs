use super::commands::parse_point;
use super::*;
use std::f64::consts::{PI, TAU};

impl OpenCadApp {
    pub(super) fn feature_utility_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "DIST" => self.utility_dist(tokens),
            "ID" => self.utility_id(tokens),
            "LIST" => self.utility_list(tokens),
            "AREA" => self.utility_area(tokens),
            "DIVIDE" => self.utility_divide(tokens),
            "MEASURE" => self.utility_measure(tokens),
            "POLYGON" => self.utility_polygon(tokens),
            _ => return None,
        };
        Some(result.map(|message| {
            self.finish();
            self.message(message, false);
        }))
    }

    fn utility_dist(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() != 2 {
            return Err("DIST syntax: DIST first-point second-point.".into());
        }
        let a = parse_point(tokens[0], self.drawing_elevation())?;
        let b = parse_point(tokens[1], a.z())?;
        let (dx, dy, dz) = (b.x() - a.x(), b.y() - a.y(), b.z() - a.z());
        Ok(format!(
            "Distance = {:.6}; Delta = ({:.6}, {:.6}, {:.6})",
            dx.hypot(dy).hypot(dz),
            dx,
            dy,
            dz
        ))
    }

    fn utility_id(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() != 1 {
            return Err("ID syntax: ID x,y[,z].".into());
        }
        let p = parse_point(tokens[0], self.drawing_elevation())?;
        Ok(format!("ID X={:.6} Y={:.6} Z={:.6}", p.x(), p.y(), p.z()))
    }

    fn utility_list(&mut self, tokens: &[&str]) -> Result<String, String> {
        let indices = self.utility_indices(tokens)?;
        if indices.is_empty() {
            return Err("LIST requires selected entities or an entity number.".into());
        }
        let mut lines = Vec::with_capacity(indices.len());
        for index in indices {
            let entity = self
                .editor
                .entities()
                .get(index)
                .ok_or("Entity number is outside this drawing.")?;
            let layer = self.editor.entity_layer(index).unwrap_or("?");
            let style = self
                .editor
                .entity_style(index)
                .map(|s| {
                    format!(
                        " color={} linetype={} scale={}",
                        s.color_index(),
                        s.linetype(),
                        s.linetype_scale()
                    )
                })
                .unwrap_or_default();
            let geometry = match entity {
                Entity::Line(line) => format!(
                    "start=({:.3},{:.3},{:.3}) end=({:.3},{:.3},{:.3})",
                    line.start().x(),
                    line.start().y(),
                    line.start().z(),
                    line.end().x(),
                    line.end().y(),
                    line.end().z()
                ),
                Entity::Circle(c) => format!(
                    "center=({:.3},{:.3},{:.3}) radius={:.3}",
                    c.center().x(),
                    c.center().y(),
                    c.center().z(),
                    c.radius()
                ),
                Entity::Arc(a) => format!(
                    "center=({:.3},{:.3},{:.3}) radius={:.3} sweep={:.3}",
                    a.center().x(),
                    a.center().y(),
                    a.center().z(),
                    a.radius(),
                    a.sweep()
                ),
                Entity::Polyline(p) => format!(
                    "vertices={} closed={} length={:.3}",
                    p.vertices().len(),
                    p.is_closed(),
                    p.length().unwrap_or(f64::NAN)
                ),
                Entity::Ellipse(e) => format!(
                    "center=({:.3},{:.3},{:.3}) ratio={:.3} sweep={:.3}",
                    e.center().x(),
                    e.center().y(),
                    e.center().z(),
                    e.ratio(),
                    e.sweep()
                ),
                Entity::Spline(s) => format!(
                    "degree={} control_points={} closed={}",
                    s.degree(),
                    s.control_points().len(),
                    s.is_closed()
                ),
                Entity::Hatch(h) => format!(
                    "boundaries={} pattern={:?}",
                    h.boundaries().len(),
                    h.pattern()
                ),
                _ => "additional entity data unavailable".into(),
            };
            lines.push(format!(
                "{} {} layer={}{} {}",
                index + 1,
                entity.name(),
                layer,
                style,
                geometry
            ));
        }
        Ok(lines.join(" | "))
    }

    fn utility_area(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() >= 3 {
            let points = tokens
                .iter()
                .map(|t| parse_point(t, self.drawing_elevation()))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(format!("Area = {:.6}", polygon_area(&points).abs()));
        }
        let indices = self.utility_indices(tokens)?;
        if indices.len() != 1 {
            return Err("AREA requires a closed selected circle, polyline, ellipse, or explicit polygon points.".into());
        }
        let entity = self
            .editor
            .entities()
            .get(indices[0])
            .ok_or("Entity number is outside this drawing.")?;
        let area = match entity {
            Entity::Circle(c) => PI * c.radius() * c.radius(),
            Entity::Ellipse(e) => PI * e.major_radius() * e.minor_radius() * e.sweep().abs() / TAU,
            Entity::Polyline(p) if p.is_closed() => bulge_area(p),
            Entity::Hatch(h) => h
                .boundaries()
                .iter()
                .map(|boundary| polygon_area(boundary))
                .sum::<f64>(),
            _ => {
                return Err(
                    "AREA supports circles, ellipses, closed polylines, and explicit polygons."
                        .into(),
                );
            }
        };
        Ok(format!("Area = {:.6}", area.abs()))
    }

    fn utility_divide(&mut self, tokens: &[&str]) -> Result<String, String> {
        self.utility_stations(tokens, false)
    }
    fn utility_measure(&mut self, tokens: &[&str]) -> Result<String, String> {
        self.utility_stations(tokens, true)
    }

    fn utility_stations(&mut self, tokens: &[&str], distance_mode: bool) -> Result<String, String> {
        let indices =
            self.utility_indices(tokens.get(..tokens.len().saturating_sub(1)).unwrap_or(&[]))?;
        let value = tokens
            .last()
            .ok_or("Specify a selected curve and a positive spacing/count.")?
            .parse::<f64>()
            .map_err(|_| "The station value must be numeric.".to_owned())?;
        if !value.is_finite() || value <= 0.0 {
            return Err("The station value must be positive and finite.".into());
        }
        if indices.len() != 1 {
            return Err("Select exactly one curve for DIVIDE or MEASURE.".into());
        }
        let entity = self
            .editor
            .entities()
            .get(indices[0])
            .ok_or("Entity number is outside this drawing.")?
            .clone();
        let length = curve_length(&entity)?;
        let count = if distance_mode {
            (length / value).floor() as usize
        } else {
            value.round() as usize
        };
        if !(1..=512).contains(&count) {
            return Err("Station count must produce between 1 and 512 points.".into());
        }
        let points = (1..=count)
            .map(|i| curve_point(&entity, i as f64 / (count + 1) as f64))
            .collect::<Result<Vec<_>, _>>()?;
        self.add_entities(
            points
                .into_iter()
                .map(|p| Entity::Point(crate::PointEntity::new(p)))
                .collect(),
        )?;
        Ok(format!("Created {} station point(s).", count))
    }

    fn utility_polygon(&mut self, tokens: &[&str]) -> Result<String, String> {
        if !(3..=4).contains(&tokens.len()) {
            return Err(
                "POLYGON syntax: POLYGON sides center radius [INSCRIBED|CIRCUMSCRIBED].".into(),
            );
        }
        let sides = tokens[0]
            .parse::<usize>()
            .map_err(|_| "Polygon sides must be a whole number.")?;
        if !(3..=512).contains(&sides) {
            return Err("Polygon sides must be between 3 and 512.".into());
        }
        let center = parse_point(tokens[1], self.drawing_elevation())?;
        let radius = tokens
            .get(2)
            .ok_or("POLYGON requires a radius.")?
            .parse::<f64>()
            .map_err(|_| "Polygon radius must be numeric.")?;
        if !radius.is_finite() || radius <= 0.0 {
            return Err("Polygon radius must be positive and finite.".into());
        }
        let mode = tokens.get(3).copied().unwrap_or("INSCRIBED");
        let radius = if mode.eq_ignore_ascii_case("CIRCUMSCRIBED") {
            radius / (PI / sides as f64).cos()
        } else if mode.eq_ignore_ascii_case("INSCRIBED") {
            radius
        } else {
            return Err("POLYGON mode must be INSCRIBED or CIRCUMSCRIBED.".into());
        };
        let points = (0..sides)
            .map(|i| {
                let a = TAU * i as f64 / sides as f64;
                Point3::new(
                    center.x() + radius * a.cos(),
                    center.y() + radius * a.sin(),
                    center.z(),
                )
                .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let polyline = Polyline2::new(points, true).map_err(|e| e.to_string())?;
        self.add_entities(vec![Entity::Polyline(polyline)])?;
        Ok(format!("Created {}-sided polygon.", sides))
    }

    fn utility_indices(&self, tokens: &[&str]) -> Result<Vec<usize>, String> {
        if tokens.is_empty() {
            return Ok(self.editor.selection().to_vec());
        }
        tokens
            .iter()
            .map(|t| {
                t.parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .map(|n| n - 1)
                    .ok_or_else(|| "Entity numbers must be positive whole numbers.".to_owned())
            })
            .collect()
    }
}

fn curve_length(entity: &Entity) -> Result<f64, String> {
    match entity {
        Entity::Line(v) => v.length().map_err(|e| e.to_string()),
        Entity::Arc(v) => v.length().map_err(|e| e.to_string()),
        Entity::Polyline(v) => v.length().map_err(|e| e.to_string()),
        Entity::Ellipse(v) => v.length().map_err(|e| e.to_string()),
        Entity::Spline(v) => {
            let p = v.tessellate(256).map_err(|e| e.to_string())?;
            Ok(p.windows(2).map(|w| dist(w[0], w[1])).sum())
        }
        _ => Err("Selected entity is not a measurable curve.".into()),
    }
}
fn curve_point(entity: &Entity, f: f64) -> Result<Point3, String> {
    match entity {
        Entity::Line(v) => v.point_at_fraction(f).map_err(|e| e.to_string()),
        Entity::Arc(v) => v.point_at_fraction(f).map_err(|e| e.to_string()),
        Entity::Circle(v) => Point3::new(
            v.center().x() + v.radius() * (TAU * f).cos(),
            v.center().y() + v.radius() * (TAU * f).sin(),
            v.center().z(),
        )
        .map_err(|e| e.to_string()),
        Entity::Polyline(v) => {
            let total = v.length().map_err(|e| e.to_string())?;
            let mut distance = total * f.clamp(0.0, 1.0);
            for segment in v.segments() {
                let segment = segment.map_err(|e| e.to_string())?;
                let length = segment.length().map_err(|e| e.to_string())?;
                if distance <= length || length == 0.0 {
                    return segment
                        .point_at_fraction(if length == 0.0 {
                            0.0
                        } else {
                            distance / length
                        })
                        .map_err(|e| e.to_string());
                }
                distance -= length;
            }
            v.vertices()
                .last()
                .copied()
                .ok_or_else(|| "Polyline has no vertices.".to_owned())
        }
        Entity::Ellipse(v) => v.point_at_fraction(f).map_err(|e| e.to_string()),
        Entity::Spline(v) => {
            let (a, b) = v.parameter_range();
            v.point_at(a + (b - a) * f).map_err(|e| e.to_string())
        }
        _ => Err("Selected entity is not a measurable curve.".into()),
    }
}
fn dist(a: Point3, b: Point3) -> f64 {
    (b.x() - a.x()).hypot(b.y() - a.y()).hypot(b.z() - a.z())
}
fn polygon_area(points: &[Point3]) -> f64 {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = points[(i + 1) % points.len()];
            p.x() * q.y() - q.x() * p.y()
        })
        .sum::<f64>()
        / 2.0
}
fn bulge_area(p: &Polyline2) -> f64 {
    let mut area = polygon_area(p.vertices());
    for i in 0..p.vertices().len() {
        let j = (i + 1) % p.vertices().len();
        let a = p.vertices()[i];
        let b = p.vertices()[j];
        let bulge = p.bulges()[i];
        if bulge != 0.0 {
            let chord = dist(a, b);
            let theta = 4.0 * bulge.atan();
            let r = chord * (1.0 + bulge * bulge) / (4.0 * bulge.abs());
            area += r * r * (theta - theta.sin()) / 2.0;
        }
    }
    area
}
