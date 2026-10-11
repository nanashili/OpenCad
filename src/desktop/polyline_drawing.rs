use super::*;
use crate::PolylineWidth;

#[derive(Default)]
pub(super) struct PlineInput {
    widths: Vec<PolylineWidth>,
    next: PolylineWidth,
    input: Option<WidthInput>,
}

#[derive(Clone, Copy)]
struct WidthInput {
    factor: f64,
    start: Option<f64>,
}

impl PlineInput {
    pub(super) fn new(width: f64) -> Self {
        Self {
            next: PolylineWidth::new(width, width).expect("validated drawing width"),
            ..Self::default()
        }
    }
}

impl OpenCadApp {
    pub(super) fn pline_width_prompt(&self) -> Option<&'static str> {
        let input = self.pline_input.input?;
        Some(match (input.factor == 2.0, input.start.is_some()) {
            (false, false) => {
                "PLINE Width · Starting width; Enter uses the current width. Exit cancels this option."
            }
            (false, true) => {
                "PLINE Width · Ending width; Enter uses the starting width. Exit cancels both values."
            }
            (true, false) => {
                "PLINE Halfwidth · Starting halfwidth; Enter uses the current halfwidth. Exit cancels this option."
            }
            (true, true) => {
                "PLINE Halfwidth · Ending halfwidth; Enter uses the starting halfwidth. Exit cancels both values."
            }
        })
    }

    pub(super) fn pline_candidate(
        &self,
        cursor: Option<Point3>,
        closed: bool,
    ) -> Result<Polyline2, String> {
        let mut vertices = self.points.clone();
        let mut widths = self.pline_input.widths.clone();
        if let Some(cursor) = cursor {
            vertices.push(cursor);
            widths.push(self.pline_input.next);
        }
        let mut polyline = Polyline2::new(vertices, closed).map_err(|e| e.to_string())?;
        polyline.set_widths(widths).map_err(|e| e.to_string())?;
        polyline.extrema().map_err(|e| e.to_string())?;
        Ok(polyline)
    }

    pub(super) fn pline_point(&mut self, point: Point3) -> Result<(), String> {
        if self.pline_input.input.is_some() {
            return Err("Complete or cancel the PLINE width values before adding a vertex.".into());
        }
        if self
            .points
            .first()
            .is_some_and(|first| first.z() != point.z())
        {
            return Err(crate::GeometryError::NonPlanarPolyline.to_string());
        }
        if !self.points.is_empty() {
            self.pline_candidate(Some(point), false)?;
            let end = self.pline_input.next.end();
            self.pline_input.next = PolylineWidth::new(end, end).expect("validated width");
        }
        self.points.push(point);
        self.pline_input.widths.push(self.pline_input.next);
        Ok(())
    }

    pub(super) fn pline_undo_point(&mut self) {
        if self.pline_input.input.is_some() {
            self.message(
                "Complete or cancel the PLINE width values before Undo.",
                true,
            );
            return;
        }
        self.points.pop();
        self.pline_input.widths.pop();
        self.pline_input.next = self.pline_input.widths.last().copied().unwrap_or_else(|| {
            let width = self.editor.pline_width();
            PolylineWidth::new(width, width).expect("validated drawing width")
        });
        self.message(self.prompt(), false);
    }

    fn pline_width_value(&mut self, value: f64) -> Result<(), String> {
        let input = self.pline_input.input.expect("pending width");
        let value = value * input.factor;
        PolylineWidth::new(value, value).map_err(|e| e.to_string())?;
        if let Some(start) = input.start {
            let next = PolylineWidth::new(start, value).map_err(|e| e.to_string())?;
            self.pline_input.next = next;
            *self.pline_input.widths.last_mut().expect("starting vertex") = next;
            self.pline_input.input = None;
        } else {
            self.pline_input.input = Some(WidthInput {
                start: Some(value),
                ..input
            });
        }
        self.message(self.prompt(), false);
        Ok(())
    }

    pub(super) fn pline_text(&mut self, text: &str) -> Option<Result<(), String>> {
        if self.tool != Tool::Polyline {
            return None;
        }
        let tokens: Vec<_> = text.split_whitespace().collect();
        let verb = tokens.first().copied().unwrap_or("").to_ascii_uppercase();
        if let Some(input) = self.pline_input.input {
            if matches!(verb.as_str(), "SAVE" | "SAVEAS" | "PLINEWID") {
                return None;
            }
            if matches!(verb.as_str(), "EXIT" | "X" | "CANCEL") && tokens.len() == 1 {
                self.pline_input.input = None;
                self.message(self.prompt(), false);
                return Some(Ok(()));
            }
            let value = if tokens.is_empty() {
                Ok(input.start.unwrap_or(self.pline_input.next.end()) / input.factor)
            } else if tokens.len() == 1 {
                tokens[0]
                    .parse::<f64>()
                    .map_err(|_| "Enter a finite nonnegative width.".to_owned())
            } else {
                Err("Enter one finite nonnegative width.".into())
            };
            return Some(value.and_then(|value| self.pline_width_value(value)));
        }
        if matches!(verb.as_str(), "U" | "UNDO") && tokens.len() == 1 {
            self.pline_undo_point();
            return Some(Ok(()));
        }
        let factor = match verb.as_str() {
            "W" | "WIDTH" => 1.0,
            "H" | "HALFWIDTH" => 2.0,
            _ => return None,
        };
        Some((|| {
            if self.points.is_empty() {
                return Err("Specify the PLINE starting point before setting widths.".into());
            }
            if tokens.len() > 3 {
                return Err("PLINE Width/Halfwidth accepts a start value and an end value.".into());
            }
            let values = tokens[1..]
                .iter()
                .map(|text| {
                    let value = text
                        .parse::<f64>()
                        .map_err(|_| "Enter a finite nonnegative width.".to_owned())?;
                    PolylineWidth::new(value * factor, value * factor)
                        .map_err(|e| e.to_string())?;
                    Ok(value)
                })
                .collect::<Result<Vec<_>, String>>()?;
            self.pline_input.input = Some(WidthInput {
                factor,
                start: None,
            });
            self.message(self.prompt(), false);
            for value in values {
                self.pline_width_value(value)?;
            }
            Ok(())
        })())
    }
}
