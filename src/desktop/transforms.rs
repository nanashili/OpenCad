use super::*;

#[derive(Clone, Copy, Default)]
pub(super) struct TransformInput {
    copy: bool,
    reference: Reference,
}

#[derive(Clone, Copy, Default)]
enum Reference {
    #[default]
    None,
    Old,
    New(f64),
}

impl OpenCadApp {
    pub(super) fn transform_prompt(&self) -> &'static str {
        if self.tool == Tool::Mirror {
            return match self.points.len() {
                0 => "MIRROR · Specify first point of the mirror line: x,y[,z]",
                1 => "MIRROR · Specify second point of the mirror line.",
                _ => "MIRROR · Erase source objects? [Yes/No] <No>",
            };
        }
        if self.points.is_empty() {
            return "Specify base point: x,y[,z]";
        }
        match (self.tool, self.transform_input.reference, self.points.len()) {
            (Tool::Rotate, Reference::None, _) => {
                "ROTATE · Angle in degrees, direction point, or [Copy/Reference]."
            }
            (Tool::Scale, Reference::None, _) => {
                "SCALE · Positive scale factor, or [Copy/Reference]."
            }
            (Tool::Rotate, Reference::Old, 1) => {
                "ROTATE · Reference angle in degrees, or first point of its direction."
            }
            (Tool::Scale, Reference::Old, 1) => {
                "SCALE · Positive reference length, or first point of that length."
            }
            (_, Reference::Old, _) => "Specify second reference point.",
            (Tool::Rotate, Reference::New(_), _) => {
                "ROTATE · New absolute angle in degrees, or direction point from the base."
            }
            (_, Reference::New(_), _) => {
                "SCALE · Positive new length, or a point at that distance from the base."
            }
            _ => "Specify transform value.",
        }
    }

    pub(super) fn transform_text(&mut self, input: &str) -> Option<Result<(), String>> {
        if self.tool == Tool::Mirror && self.points.len() == 2 {
            let erase = match input.to_ascii_uppercase().as_str() {
                "Y" | "YES" => true,
                "" | "N" | "NO" => false,
                _ => return None,
            };
            return Some(
                Transform::mirror_xy(self.points[0], self.points[1])
                    .map_err(|error| error.to_string())
                    .and_then(|transform| self.apply_transform(transform, !erase)),
            );
        }
        if !matches!(self.tool, Tool::Rotate | Tool::Scale) || self.points.is_empty() {
            return None;
        }
        match input.to_ascii_uppercase().as_str() {
            "C" | "COPY" => {
                self.transform_input.copy = true;
                self.message(format!("Copy enabled. {}", self.prompt()), false);
                Some(Ok(()))
            }
            "R" | "REFERENCE" => {
                self.transform_input.reference = Reference::Old;
                self.points.truncate(1);
                self.message(self.prompt(), false);
                Some(Ok(()))
            }
            _ => input.parse::<f64>().ok().map(|value| {
                if !value.is_finite() {
                    return Err("Numbers must be finite.".into());
                }
                self.transform_value(value)
            }),
        }
    }

    pub(super) fn transform_point(&mut self, point: Point3) -> Result<(), String> {
        if self.points.is_empty() {
            self.points.push(point);
        } else if self.tool == Tool::Mirror {
            if self.points.len() == 2 {
                return Err("Choose Yes or No to erase or retain the source objects.".into());
            }
            Transform::mirror_xy(self.points[0], point).map_err(|error| error.to_string())?;
            self.points.push(point);
        } else if matches!(self.transform_input.reference, Reference::Old) {
            if self.points.len() == 1 {
                self.points.push(point);
            } else {
                let value = self.transform_measure(self.points[1], point)?;
                self.transform_value(value)?;
                return Ok(());
            }
        } else {
            if self.tool == Tool::Scale && matches!(self.transform_input.reference, Reference::None)
            {
                return Err(
                    "Enter a positive scale factor, or Reference to set lengths with points."
                        .into(),
                );
            }
            let value = self.transform_measure(self.points[0], point)?;
            self.transform_value(value)?;
            return Ok(());
        }
        self.message(self.prompt(), false);
        Ok(())
    }

    fn transform_measure(&self, first: Point3, second: Point3) -> Result<f64, String> {
        let dx = second.x() - first.x();
        let dy = second.y() - first.y();
        if !dx.is_finite() || !dy.is_finite() {
            return Err("The reference measurement exceeds the finite coordinate range.".into());
        }
        if self.tool == Tool::Rotate {
            if dx == 0.0 && dy == 0.0 {
                return Err("An angle needs two different XY points.".into());
            }
            Ok(dy.atan2(dx).to_degrees())
        } else {
            let dz = second.z() - first.z();
            let length = dx.hypot(dy).hypot(dz);
            if !length.is_finite() || length <= 0.0 {
                return Err("The reference length must be finite and positive.".into());
            }
            Ok(length)
        }
    }

    fn transform_value(&mut self, value: f64) -> Result<(), String> {
        if matches!(self.transform_input.reference, Reference::Old) {
            if self.tool == Tool::Scale && value <= 0.0 {
                return Err("The reference length must be positive.".into());
            }
            self.transform_input.reference = Reference::New(value);
            self.points.truncate(1);
            self.message(self.prompt(), false);
            return Ok(());
        }
        let transform = self.transform_for_value(value)?;
        self.apply_transform(transform, self.transform_input.copy)
    }

    fn transform_for_value(&self, value: f64) -> Result<Transform, String> {
        let base = self.points[0];
        if self.tool == Tool::Rotate {
            let angle = match self.transform_input.reference {
                Reference::New(old) => value.rem_euclid(360.0) - old.rem_euclid(360.0),
                _ => value.rem_euclid(360.0),
            };
            Transform::rotate_xy(base, angle.to_radians()).map_err(|error| error.to_string())
        } else {
            let factor = match self.transform_input.reference {
                Reference::New(old) => value / old,
                _ => value,
            };
            Transform::scale(base, factor).map_err(|error| error.to_string())
        }
    }

    pub(super) fn transform_preview(&self, cursor: Point3) -> Option<Transform> {
        let base = *self.points.first()?;
        if self.tool == Tool::Mirror {
            return Transform::mirror_xy(base, self.points.get(1).copied().unwrap_or(cursor)).ok();
        }
        if matches!(self.transform_input.reference, Reference::Old)
            || (self.tool == Tool::Scale
                && matches!(self.transform_input.reference, Reference::None))
        {
            return None;
        }
        self.transform_for_value(self.transform_measure(base, cursor).ok()?)
            .ok()
    }

    pub(super) fn apply_transform(
        &mut self,
        transform: Transform,
        copy: bool,
    ) -> Result<(), String> {
        if self.editor.selection().is_empty() {
            return Err("Select entities before transforming them.".into());
        }
        let changed = self
            .editor
            .transform_selection(transform, copy)
            .map_err(|error| error.to_string())?;
        self.finish();
        self.message(
            if !changed {
                "Selection unchanged."
            } else if copy {
                "Transformed copy created."
            } else {
                "Selection transformed."
            },
            false,
        );
        Ok(())
    }
}
