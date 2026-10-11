use super::*;
use crate::{PolylineSegment, PolylineWidth};

#[derive(Default)]
pub(super) struct PlineInput {
    widths: Vec<PolylineWidth>,
    bulges: Vec<f64>,
    next: PolylineWidth,
    input: Option<WidthInput>,
    arc_mode: bool,
    arc_input: Option<ArcInput>,
}

#[derive(Clone, Copy)]
enum ArcInput {
    Direction(Option<Point3>),
    Second(Option<Point3>),
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
    pub(super) fn pline_arc_prompt(&self) -> Option<&'static str> {
        if !self.pline_input.arc_mode || self.points.is_empty() {
            return None;
        }
        Some(match self.pline_input.arc_input {
            Some(ArcInput::Direction(None)) => {
                "PLINE Arc · Specify a point defining the starting tangent direction. Exit cancels this option."
            }
            Some(ArcInput::Direction(Some(_))) => {
                "PLINE Arc · Specify endpoint for the starting tangent direction. Exit cancels this option."
            }
            Some(ArcInput::Second(None)) => {
                "PLINE Arc · Specify second point on the arc. Exit cancels this option."
            }
            Some(ArcInput::Second(Some(_))) => {
                "PLINE Arc · Specify endpoint through the second point. Exit cancels this option."
            }
            None => {
                "PLINE Arc · Tangent endpoint; Direction / Second pt / Line / Width / Halfwidth / Undo / Close, or Enter to finish."
            }
        })
    }

    pub(super) fn pline_arc_pending(&self) -> bool {
        self.pline_input.arc_input.is_some()
    }

    pub(super) fn pline_arc_guide(&self, cursor: Point3) -> Option<Line3> {
        let end = match self.pline_input.arc_input? {
            ArcInput::Direction(Some(point)) => point,
            ArcInput::Direction(None) | ArcInput::Second(None) => cursor,
            ArcInput::Second(Some(_)) => return None,
        };
        Some(Line3::new(*self.points.last()?, end))
    }

    fn pline_bulge_to(&self, end: Point3) -> Result<f64, String> {
        if !self.pline_input.arc_mode {
            return Ok(0.0);
        }
        let start = *self
            .points
            .last()
            .ok_or("Specify the PLINE starting point.")?;
        let segment = match self.pline_input.arc_input {
            Some(ArcInput::Direction(Some(direction))) => PolylineSegment::arc_from_tangent(
                start,
                end,
                (direction.x() - start.x(), direction.y() - start.y()),
            ),
            Some(ArcInput::Second(Some(second))) => {
                PolylineSegment::arc_through(start, second, end)
            }
            Some(_) => {
                return Err("Specify the PLINE arc's direction or second point first.".into());
            }
            None => {
                if self.points.len() < 2 {
                    return Err(
                        "The first arc needs Direction or Second pt to define its shape.".into(),
                    );
                }
                let prior = self.pline_candidate(None, false)?;
                let tangent = prior
                    .segments()
                    .last()
                    .expect("previous segment")
                    .and_then(PolylineSegment::end_tangent_xy)
                    .map_err(|e| {
                        format!("Previous segment has no tangent: {e}. Use Direction or Second pt.")
                    })?;
                PolylineSegment::arc_from_tangent(start, end, tangent)
            }
        }
        .map_err(|e| e.to_string())?;
        Ok(segment.bulge())
    }

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
        let mut bulges = self.pline_input.bulges.clone();
        if let Some(cursor) = cursor {
            *bulges
                .last_mut()
                .ok_or("Specify the PLINE starting point.")? = self.pline_bulge_to(cursor)?;
            vertices.push(cursor);
            widths.push(self.pline_input.next);
            bulges.push(0.0);
        } else if closed {
            if vertices.len() < 2 {
                return Err(crate::GeometryError::NotEnoughVertices.to_string());
            }
            *bulges.last_mut().expect("starting vertex") = self.pline_bulge_to(vertices[0])?;
        }
        let mut polyline =
            Polyline2::with_bulges(vertices, bulges, closed).map_err(|e| e.to_string())?;
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
        if matches!(
            self.pline_input.arc_input,
            Some(ArcInput::Direction(None) | ArcInput::Second(None))
        ) {
            let start = *self.points.last().expect("arc starting point");
            if Line3::new(start, point)
                .length()
                .map_err(|e| e.to_string())?
                == 0.0
            {
                return Err("Choose an arc construction point distinct from its start.".into());
            }
            self.pline_input.arc_input = Some(match self.pline_input.arc_input {
                Some(ArcInput::Direction(_)) => ArcInput::Direction(Some(point)),
                _ => ArcInput::Second(Some(point)),
            });
            self.message(self.prompt(), false);
            return Ok(());
        }
        if !self.points.is_empty() {
            let candidate = self.pline_candidate(Some(point), false)?;
            *self.pline_input.bulges.last_mut().expect("starting vertex") =
                candidate.bulges()[self.points.len() - 1];
            let end = self.pline_input.next.end();
            self.pline_input.next = PolylineWidth::new(end, end).expect("validated width");
            self.pline_input.arc_input = None;
        }
        self.points.push(point);
        self.pline_input.widths.push(self.pline_input.next);
        self.pline_input.bulges.push(0.0);
        Ok(())
    }

    pub(super) fn pline_undo_point(&mut self) {
        if self.pline_input.input.is_some() || self.pline_arc_pending() {
            self.message(
                "Complete or cancel the PLINE construction option before Undo.",
                true,
            );
            return;
        }
        self.points.pop();
        self.pline_input.widths.pop();
        self.pline_input.bulges.pop();
        if let Some(bulge) = self.pline_input.bulges.last_mut() {
            *bulge = 0.0;
        }
        if self.points.is_empty() {
            self.pline_input.arc_mode = false;
        }
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
        if let Some(result) = self.pline_arc_text(&tokens, &verb) {
            return Some(result);
        }
        let factor = match verb.as_str() {
            "W" | "WIDTH" => 1.0,
            "H" | "HALFWIDTH" => 2.0,
            _ => return None,
        };
        Some((|| {
            if self.pline_arc_pending() {
                return Err(
                    "Complete or cancel the PLINE arc construction before setting widths.".into(),
                );
            }
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

    fn pline_arc_text(&mut self, tokens: &[&str], verb: &str) -> Option<Result<(), String>> {
        if matches!(verb, "EXIT" | "X" | "CANCEL") && tokens.len() == 1 && self.pline_arc_pending()
        {
            self.pline_input.arc_input = None;
            self.message(self.prompt(), false);
            return Some(Ok(()));
        }
        let option = match verb {
            "A" if !self.pline_input.arc_mode => "ARC",
            "ARC" => "ARC",
            "LINE" | "L" => "LINE",
            "D" | "DIRECTION" if self.pline_input.arc_mode => "DIRECTION",
            "S" | "SE" | "SECOND" | "SECONDPT" if self.pline_input.arc_mode => "SECOND",
            "A" | "ANGLE" | "CE" | "CENTER" | "R" | "RADIUS" if self.pline_input.arc_mode => {
                return Some(Err("PLINE Angle, Center and Radius construction are not supported yet. Use a tangent endpoint, Direction or Second pt.".into()));
            }
            "LENGTH" | "LEN" => {
                return Some(Err("PLINE Length construction is not supported yet.".into()));
            }
            _ => return None,
        };
        Some((|| {
            if tokens.len() != 1
                && !(option == "SECOND"
                    && tokens.len() == 2
                    && tokens[1].eq_ignore_ascii_case("PT"))
            {
                return Err("Enter the PLINE option, then specify its construction points.".into());
            }
            if option != "LINE" && self.points.is_empty() {
                return Err("Specify the PLINE starting point before choosing Arc.".into());
            }
            self.pline_input.arc_mode = option != "LINE";
            self.pline_input.arc_input = match option {
                "DIRECTION" => Some(ArcInput::Direction(None)),
                "SECOND" => Some(ArcInput::Second(None)),
                _ => None,
            };
            self.message(self.prompt(), false);
            Ok(())
        })())
    }
}
