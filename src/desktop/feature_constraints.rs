use super::*;
use crate::{Anchor, ConstraintKind, LinePair};

impl OpenCadApp {
    pub(super) fn feature_constraints_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "GEOMCONSTRAINT" => self.geom_constraint(tokens),
            "DIMCONSTRAINT" => self.dim_constraint(tokens),
            "CONSTRAINT" => self.constraint_management(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn entity_number(&self, tokens: &[&str], position: usize) -> Result<usize, String> {
        if let Some(value) = tokens.get(position) {
            return value
                .parse::<usize>()
                .ok()
                .and_then(|number| number.checked_sub(1))
                .filter(|index| *index < self.editor.entities().len())
                .ok_or_else(|| "Entity number is outside this drawing.".into());
        }
        self.editor
            .selection()
            .get(position.saturating_sub(1))
            .copied()
            .ok_or_else(|| "Select the required geometry or provide entity numbers.".into())
    }

    fn add_constraint(&mut self, kind: ConstraintKind, value: Option<f64>) -> Result<(), String> {
        let id = self
            .editor
            .add_constraint(kind, value)
            .map_err(|error| error.to_string())?;
        self.editor
            .solve_constraints()
            .map_err(|error| error.to_string())?;
        self.message(format!("Constraint {id} added and solved."), false);
        Ok(())
    }

    fn geom_constraint(&mut self, tokens: &[&str]) -> Result<(), String> {
        let kind = tokens
            .first()
            .ok_or("GEOMCONSTRAINT kind [entity numbers].")?
            .to_ascii_uppercase();
        match kind.as_str() {
            "HORIZONTAL" | "VERTICAL" => {
                let entity = self.entity_number(tokens, 1)?;
                self.add_constraint(
                    if kind == "HORIZONTAL" {
                        ConstraintKind::Horizontal(entity)
                    } else {
                        ConstraintKind::Vertical(entity)
                    },
                    None,
                )
            }
            "COINCIDENT" => {
                let first = self.entity_number(tokens, 1)?;
                let second = self.entity_number(tokens, 2)?;
                self.add_constraint(
                    ConstraintKind::Coincident(Anchor::LineEnd(first), Anchor::LineStart(second)),
                    None,
                )
            }
            "PARALLEL" | "PERPENDICULAR" | "EQUAL" => {
                let first = self.entity_number(tokens, 1)?;
                let second = self.entity_number(tokens, 2)?;
                let pair = LinePair::Lines(first, second);
                self.add_constraint(
                    match kind.as_str() {
                        "PARALLEL" => ConstraintKind::Parallel(pair),
                        "PERPENDICULAR" => ConstraintKind::Perpendicular(pair),
                        _ => ConstraintKind::EqualLength(pair),
                    },
                    None,
                )
            }
            "CONCENTRIC" => {
                let first = self.entity_number(tokens, 1)?;
                let second = self.entity_number(tokens, 2)?;
                self.add_constraint(ConstraintKind::Concentric(first, second), None)
            }
            "TANGENT" => {
                let line = self.entity_number(tokens, 1)?;
                let circle = self.entity_number(tokens, 2)?;
                self.add_constraint(ConstraintKind::Tangent { line, circle }, None)
            }
            "FIX" => {
                let entity = self.entity_number(tokens, 1)?;
                let ids = self
                    .editor
                    .fix_entity_constraint(entity)
                    .map_err(|error| error.to_string())?;
                self.editor
                    .solve_constraints()
                    .map_err(|error| error.to_string())?;
                self.message(
                    format!("Fixed entity with constraints {}.", ids.len()),
                    false,
                );
                Ok(())
            }
            unsupported => Err(format!(
                "Unsupported geometric constraint {unsupported}; use HORIZONTAL, VERTICAL, COINCIDENT, PARALLEL, PERPENDICULAR, CONCENTRIC, EQUAL, TANGENT, or FIX."
            )),
        }
    }

    fn dim_constraint(&mut self, tokens: &[&str]) -> Result<(), String> {
        let kind = tokens
            .first()
            .ok_or("DIMCONSTRAINT kind value [entity numbers].")?
            .to_ascii_uppercase();
        let value = tokens
            .get(1)
            .ok_or("Dimensional constraints require a numeric value.")?
            .parse::<f64>()
            .map_err(|_| "Constraint value must be a finite number.")?;
        if !value.is_finite() {
            return Err("Constraint value must be finite.".into());
        }
        let constraint = match kind.as_str() {
            "LENGTH" | "LINEAR" => ConstraintKind::LineLength(self.entity_number(tokens, 2)?),
            "RADIUS" => ConstraintKind::Radius(self.entity_number(tokens, 2)?),
            "DIAMETER" => ConstraintKind::Diameter(self.entity_number(tokens, 2)?),
            "DISTANCE" => ConstraintKind::Distance {
                a: Anchor::LineStart(self.entity_number(tokens, 2)?),
                b: Anchor::LineEnd(self.entity_number(tokens, 3)?),
            },
            "ANGLE" => ConstraintKind::Angle(LinePair::Lines(
                self.entity_number(tokens, 2)?,
                self.entity_number(tokens, 3)?,
            )),
            unsupported => {
                return Err(format!("Unsupported dimensional constraint {unsupported}."));
            }
        };
        self.add_constraint(constraint, Some(value))
    }

    fn constraint_management(&mut self, tokens: &[&str]) -> Result<(), String> {
        match tokens
            .first()
            .map(|value| value.to_ascii_uppercase())
            .as_deref()
        {
            Some("LIST") | None => {
                let listing = self
                    .editor
                    .constraints()
                    .iter()
                    .map(|record| format!("{}:{:?}", record.id(), record.kind()))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.message(
                    if listing.is_empty() {
                        "No constraints defined.".into()
                    } else {
                        listing
                    },
                    false,
                );
                Ok(())
            }
            Some("DELETE") => {
                let id = tokens
                    .get(1)
                    .ok_or("CONSTRAINT DELETE id.")?
                    .parse::<u64>()
                    .map_err(|_| "Constraint id must be a whole number.")?;
                self.editor
                    .remove_constraint(id)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Constraint {id} deleted."), false);
                Ok(())
            }
            Some("SOLVE") => {
                self.editor
                    .solve_constraints()
                    .map_err(|error| error.to_string())?;
                self.message("Constraints solved.", false);
                Ok(())
            }
            Some(action @ ("ENABLE" | "DISABLE" | "TOGGLE")) => {
                let id = tokens
                    .get(1)
                    .ok_or("CONSTRAINT ENABLE|DISABLE|TOGGLE id.")?
                    .parse::<u64>()
                    .map_err(|_| "Constraint id must be a whole number.")?;
                let enabled = match action {
                    "ENABLE" => true,
                    "DISABLE" => false,
                    _ => !self
                        .editor
                        .constraints()
                        .iter()
                        .find(|record| record.id() == id)
                        .ok_or("Constraint id was not found.")?
                        .enabled(),
                };
                self.editor
                    .set_constraint_enabled(id, enabled)
                    .map_err(|error| error.to_string())?;
                self.message(
                    format!(
                        "Constraint {id} {}.",
                        if enabled { "enabled" } else { "disabled" }
                    ),
                    false,
                );
                Ok(())
            }
            Some(action @ ("VALUE" | "EDIT")) => {
                let id = tokens
                    .get(1)
                    .ok_or("CONSTRAINT VALUE|EDIT id value.")?
                    .parse::<u64>()
                    .map_err(|_| "Constraint id must be a whole number.")?;
                let value = tokens
                    .get(2)
                    .ok_or("Constraint VALUE|EDIT id value.")?
                    .parse::<f64>()
                    .map_err(|_| "Constraint value must be numeric.")?;
                if !value.is_finite() {
                    return Err("Constraint value must be finite.".into());
                }
                self.editor
                    .set_constraint_value(id, value)
                    .map_err(|error| error.to_string())?;
                self.message(format!("Constraint {id} value updated ({action})."), false);
                Ok(())
            }
            Some(action) => Err(format!("Unknown CONSTRAINT action {action}.")),
        }
    }
}
