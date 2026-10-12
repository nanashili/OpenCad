use super::commands::parse_point;
use super::*;

impl OpenCadApp {
    pub(super) fn feature_modify_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "ARRAY" | "ARRAYRECT" => self.modify_array_rect(tokens),
            "ARRAYPOLAR" => self.modify_array_polar(tokens),
            "ALIGN" => self.modify_align(tokens),
            "MATCHPROP" => self.modify_matchprop(tokens),
            "REVERSE" => self.modify_reverse(),
            "STRETCH" => self.modify_stretch(tokens),
            _ => return None,
        };
        Some(result.map(|message| {
            self.finish();
            self.message(message, false);
        }))
    }

    fn modify_array_rect(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() != 4 {
            return Err(
                "ARRAYRECT syntax: ARRAYRECT rows columns row-spacing column-spacing.".into(),
            );
        }
        let rows = whole(tokens[0])?;
        let columns = whole(tokens[1])?;
        let row = number(tokens[2])?;
        let column = number(tokens[3])?;
        if rows == 0 || columns == 0 || rows.checked_mul(columns).is_none_or(|n| n > 512) {
            return Err("ARRAYRECT count must be between 1 and 512.".into());
        }
        let selection = self.editor.selection().to_vec();
        if selection.is_empty() {
            return Err("Select entities before ARRAYRECT.".into());
        }
        let transforms = (0..rows)
            .flat_map(|r| {
                (0..columns)
                    .map(move |c| Transform::translation(c as f64 * column, r as f64 * row, 0.0))
            })
            .collect::<Vec<_>>();
        self.editor
            .copy_selection_many(&transforms)
            .map_err(|e| e.to_string())?;
        Ok(format!(
            "Created {} array copies.",
            rows * columns * selection.len()
        ))
    }

    fn modify_array_polar(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() != 3 {
            return Err("ARRAYPOLAR syntax: ARRAYPOLAR count center total-angle-degrees.".into());
        }
        let count = whole(tokens[0])?;
        if !(1..=512).contains(&count) {
            return Err("ARRAYPOLAR count must be between 1 and 512.".into());
        }
        let center = parse_point(tokens[1], self.drawing_elevation())?;
        let angle = number(tokens[2])?.to_radians();
        let selection = self.editor.selection().len();
        if selection == 0 {
            return Err("Select entities before ARRAYPOLAR.".into());
        }
        let transforms = (1..=count)
            .map(|i| Transform::rotate_xy(center, angle * i as f64 / count as f64))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        self.editor
            .copy_selection_many(&transforms)
            .map_err(|e| e.to_string())?;
        Ok(format!("Created {} array copies.", count * selection))
    }

    fn modify_align(&mut self, tokens: &[&str]) -> Result<String, String> {
        if !(4..=5).contains(&tokens.len()) {
            return Err("ALIGN syntax: ALIGN source1 source2 target1 target2 [scale].".into());
        }
        let a = parse_point(tokens[0], self.drawing_elevation())?;
        let b = parse_point(tokens[1], a.z())?;
        let c = parse_point(tokens[2], self.drawing_elevation())?;
        let d = parse_point(tokens[3], c.z())?;
        let source_len = (b.x() - a.x()).hypot(b.y() - a.y());
        let target_len = (d.x() - c.x()).hypot(d.y() - c.y());
        if source_len == 0.0 || target_len == 0.0 {
            return Err("ALIGN points must define two nonzero XY directions.".into());
        }
        let scale = tokens
            .get(4)
            .map_or(Ok(target_len / source_len), |v| number(v))?;
        if !scale.is_finite() || scale <= 0.0 {
            return Err("ALIGN scale must be positive and finite.".into());
        }
        let rotation = (d.y() - c.y()).atan2(d.x() - c.x()) - (b.y() - a.y()).atan2(b.x() - a.x());
        let transforms = self.editor.selection().to_vec();
        if transforms.is_empty() {
            return Err("Select entities before ALIGN.".into());
        }
        self.editor
            .map_selection_with(false, |entity| {
                let entity = entity.transformed(Transform::scale(a, scale)?)?;
                let entity = entity.transformed(Transform::rotate_xy(a, rotation)?)?;
                entity.translated(c.x() - a.x(), c.y() - a.y(), c.z() - a.z())
            })
            .map_err(|e| e.to_string())?;
        Ok(format!("Aligned {} entities.", transforms.len()))
    }

    fn modify_matchprop(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() < 2 {
            return Err("MATCHPROP syntax: MATCHPROP source-number target-number...".into());
        }
        let source = entity_number(tokens[0])?;
        let targets = tokens[1..]
            .iter()
            .map(|v| entity_number(v))
            .collect::<Result<Vec<_>, _>>()?;
        self.editor
            .match_properties(source, &targets)
            .map_err(|e| e.to_string())?;
        Ok(format!("Matched properties to {} entities.", targets.len()))
    }

    fn modify_reverse(&mut self) -> Result<String, String> {
        if self.editor.selection().is_empty() {
            return Err("Select entities before REVERSE.".into());
        }
        let count = self.editor.selection().len();
        self.editor
            .map_selection_with(false, reverse_entity)
            .map_err(|e| e.to_string())?;
        Ok(format!("Reversed {} entities.", count))
    }

    fn modify_stretch(&mut self, tokens: &[&str]) -> Result<String, String> {
        if tokens.len() != 3 {
            return Err("STRETCH syntax: STRETCH window-min window-max displacement.".into());
        }
        let min = parse_point(tokens[0], self.drawing_elevation())?;
        let max = parse_point(tokens[1], min.z())?;
        let delta = parse_point(tokens[2], 0.0)?;
        if max.x() <= min.x() || max.y() <= min.y() {
            return Err("STRETCH window must have positive width and height.".into());
        }
        let selection = self.editor.selection().to_vec();
        if selection.is_empty() {
            return Err("Select entities before STRETCH.".into());
        }
        self.editor
            .map_selection_with(false, |entity| stretch_entity(entity, min, max, delta))
            .map_err(|e| e.to_string())?;
        Ok(format!("Stretched {} entities.", selection.len()))
    }
}

fn whole(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| "Expected a whole number.".into())
}
fn number(value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|_| "Expected a number.".into())
}
fn entity_number(value: &str) -> Result<usize, String> {
    whole(value)?
        .checked_sub(1)
        .ok_or_else(|| "Entity numbers start at 1.".into())
}
fn reverse_entity(entity: &Entity) -> Result<Entity, crate::GeometryError> {
    match entity {
        Entity::Line(v) => Ok(Entity::Line(Line3::new(v.end(), v.start()))),
        Entity::Polyline(v) => {
            let mut p = v.clone();
            p.reverse();
            Ok(Entity::Polyline(p))
        }
        Entity::Spline(v) => {
            let mut knots = v.knots().to_vec();
            let lo = v.parameter_range().0;
            let hi = v.parameter_range().1;
            for k in &mut knots {
                *k = lo + hi - *k;
            }
            knots.reverse();
            let points = v.control_points().iter().copied().rev().collect();
            let weights = v.weights().iter().copied().rev().collect();
            Ok(Entity::Spline(crate::Spline3::new(
                v.degree(),
                knots,
                points,
                weights,
                v.is_closed(),
            )?))
        }
        Entity::Arc(_) => Err(crate::GeometryError::UnsupportedLengthenAngle),
        _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
    }
}
fn stretch_entity(
    entity: &Entity,
    min: Point3,
    max: Point3,
    d: Point3,
) -> Result<Entity, crate::GeometryError> {
    let inside =
        |p: Point3| p.x() >= min.x() && p.x() <= max.x() && p.y() >= min.y() && p.y() <= max.y();
    match entity {
        Entity::Line(v) => {
            let a = if inside(v.start()) {
                Point3::new(
                    v.start().x() + d.x(),
                    v.start().y() + d.y(),
                    v.start().z() + d.z(),
                )?
            } else {
                v.start()
            };
            let b = if inside(v.end()) {
                Point3::new(
                    v.end().x() + d.x(),
                    v.end().y() + d.y(),
                    v.end().z() + d.z(),
                )?
            } else {
                v.end()
            };
            Ok(Entity::Line(Line3::new(a, b)))
        }
        Entity::Polyline(v) => {
            let mut p = v.clone();
            p.set_vertices(
                v.vertices()
                    .iter()
                    .map(|q| {
                        if inside(*q) {
                            Point3::new(q.x() + d.x(), q.y() + d.y(), q.z() + d.z())
                        } else {
                            Ok(*q)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )?;
            Ok(Entity::Polyline(p))
        }
        Entity::Point(v) => {
            if inside(v.position()) {
                Ok(Entity::Point(crate::PointEntity::new(Point3::new(
                    v.position().x() + d.x(),
                    v.position().y() + d.y(),
                    v.position().z() + d.z(),
                )?)))
            } else {
                Ok(entity.clone())
            }
        }
        Entity::Spline(v) => {
            let points = v
                .control_points()
                .iter()
                .map(|q| {
                    if inside(*q) {
                        Point3::new(q.x() + d.x(), q.y() + d.y(), q.z() + d.z())
                    } else {
                        Ok(*q)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Entity::Spline(crate::Spline3::new(
                v.degree(),
                v.knots().to_vec(),
                points,
                v.weights().to_vec(),
                v.is_closed(),
            )?))
        }
        _ => Ok(entity.clone()),
    }
}
