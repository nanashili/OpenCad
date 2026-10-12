use eframe::egui;

#[derive(Clone, Debug)]
struct FormField {
    label: &'static str,
    value: String,
    hint: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum FeatureFormResult {
    Pending,
    Created(String),
    Cancelled,
}

/// Small, command-aware input form used when a menu command has no arguments.
/// Values are intentionally emitted as the same space-delimited command grammar
/// accepted by the normal command parser; validation remains there so errors do
/// not discard the user's form input.
pub(super) struct FeatureForm {
    verb: String,
    title: String,
    fields: Vec<FormField>,
}

impl FeatureForm {
    pub(super) fn for_command(verb: &str) -> Option<Self> {
        let verb = verb.to_ascii_uppercase();
        let specs: &[(&str, &str, &str)] = match verb.as_str() {
            "POINT" => &[("Point", "0,0,0", "x,y[,z]")],
            "ELLIPSE" => &[
                ("Center", "0,0,0", "x,y[,z]"),
                ("Major axis end", "10,0,0", "x,y[,z]"),
                ("Ratio", "0.5", "positive ratio"),
            ],
            "SPLINE" => &[
                ("Degree", "3", "whole number"),
                (
                    "Control points",
                    "0,0; 10,0; 10,10; 0,10",
                    "point; point; point ...",
                ),
            ],
            "TEXT" | "MTEXT" => &[
                ("Location", "0,0,0", "x,y[,z]"),
                ("Height", "2.5", "positive number"),
                ("Content", "Text", "text content"),
            ],
            "LEADER" | "MLEADER" => &[("Vertices", "0,0;10,5", "point;point;...")],
            "TABLE" => &[
                ("Anchor", "0,0,0", "x,y[,z]"),
                ("Rows", "2", "positive integer"),
                ("Columns", "2", "positive integer"),
                ("Row height", "5", "positive number"),
                ("Column width", "10", "positive number"),
            ],
            "DIMLINEAR" | "DIMALIGNED" => &[
                ("First point", "0,0,0", "x,y[,z]"),
                ("Second point", "10,0,0", "x,y[,z]"),
                ("Offset", "0,5,0", "x,y[,z]"),
            ],
            "DIMRADIUS" | "RADIUS" => &[
                ("Center", "0,0,0", "x,y[,z]"),
                ("Definition point", "5,0,0", "x,y[,z]"),
            ],
            "DIMDIAMETRIC" | "DIAMETRIC" => &[
                ("First point", "0,0,0", "x,y[,z]"),
                ("Second point", "10,0,0", "x,y[,z]"),
            ],
            "DIMANGULAR" | "ANGULAR" => &[
                ("Center", "0,0,0", "x,y[,z]"),
                ("First point", "10,0,0", "x,y[,z]"),
                ("Second point", "0,10,0", "x,y[,z]"),
            ],
            "HATCH" => &[("Boundary points", "0,0;10,0;10,10;0,10", "point;point;...")],
            "BLOCK" => &[
                ("Name", "Block1", "block name"),
                ("Base point", "0,0,0", "x,y[,z]"),
            ],
            "INSERT" => &[
                ("Block name", "Block1", "existing block"),
                ("Insertion point", "0,0,0", "x,y[,z]"),
                ("Scale", "1,1,1", "sx,sy,sz"),
                ("Rotation", "0", "degrees"),
            ],
            "POLYGON" => &[
                ("Sides", "6", "3 or more"),
                ("Center", "0,0,0", "x,y[,z]"),
                ("Radius", "10", "positive number"),
            ],
            "ARRAY" | "ARRAYRECT" => &[
                ("Rows", "2", "positive integer"),
                ("Columns", "2", "positive integer"),
                ("Row spacing", "10", "number"),
                ("Column spacing", "10", "number"),
            ],
            "ARRAYPOLAR" => &[
                ("Items", "6", "positive integer"),
                ("Center", "0,0,0", "x,y[,z]"),
                ("Angle", "360", "degrees"),
            ],
            "BOX" => &[
                ("Minimum corner", "0,0,0", "x,y,z"),
                ("Maximum corner", "10,10,10", "x,y,z"),
            ],
            "XLINE" => &[
                ("Origin", "0,0,0", "x,y[,z]"),
                ("Through", "10,0,0", "x,y[,z]"),
            ],
            "RAY" => &[
                ("Origin", "0,0,0", "x,y[,z]"),
                ("Through", "10,0,0", "x,y[,z]"),
            ],
            "LAYOUT" => &[
                ("Operation", "LIST", "NEW|SET|COPY|RENAME|DELETE|LIST"),
                ("Arguments", "", "layout arguments"),
            ],
            "PAGESETUP" => &[
                ("Width", "210", "paper width"),
                ("Height", "297", "paper height"),
                ("Orientation", "PORTRAIT", "PORTRAIT|LANDSCAPE"),
                ("Scale", "1", "positive number"),
            ],
            "MVIEW" => &[
                ("Minimum point", "0,0,0", "x,y[,z]"),
                ("Maximum point", "100,100,0", "x,y[,z]"),
            ],
            "PLOT" => &[("Output path", "plot.svg", "file path")],
            "EXPORTPDF" => &[("Output path", "plot.pdf", "file path")],
            "IMAGEATTACH" => &[
                ("Path", "image.png", "PNG path"),
                ("Insertion", "0,0,0", "x,y[,z]"),
                ("U vector", "100,0,0", "x,y[,z]"),
                ("V vector", "0,100,0", "x,y[,z]"),
                ("Pixel width", "100", "positive integer"),
                ("Pixel height", "100", "positive integer"),
            ],
            "XATTACH" => &[
                ("DXF path", "reference.dxf", "DXF file path"),
                ("Reference name", "Reference1", "optional block name"),
            ],
            "WEDGE" | "PYRAMID" => &[
                ("Insertion point", "0,0,0", "x,y[,z]"),
                ("Width", "10", "positive number"),
                ("Depth", "10", "positive number"),
                ("Height", "10", "positive number"),
            ],
            "SPHERE" => &[
                ("Center", "0,0,0", "x,y[,z]"),
                ("Radius", "10", "positive number"),
            ],
            "CYLINDER" | "CONE" => &[
                ("Center", "0,0,0", "x,y[,z]"),
                ("Radius", "10", "positive number"),
                ("Height", "20", "positive number"),
            ],
            "TORUS" => &[
                ("Center", "0,0,0", "x,y[,z]"),
                ("Major radius", "10", "positive number"),
                ("Minor radius", "2", "positive number"),
            ],
            "3DFACE" => &[("Vertices", "0,0,0 10,0,0 10,10,0", "three or four points")],
            "EXTRUDE" => &[
                ("Boundary points", "0,0 10,0 10,10", "point point ..."),
                ("Height", "10", "nonzero number"),
            ],
            "REVOLVE" => &[
                ("Profile points", "2,0,0 2,0,10", "point point ..."),
                ("Axis origin", "0,0,0", "x,y[,z]"),
                ("Axis vector", "0,0,1", "x,y[,z]"),
                ("Angle", "360", "degrees"),
            ],
            _ => return None,
        };
        Some(Self {
            title: format!("OpenCad · {verb}"),
            verb,
            fields: specs
                .iter()
                .map(|(label, value, hint)| FormField {
                    label,
                    value: (*value).into(),
                    hint,
                })
                .collect(),
        })
    }

    pub(super) fn show(&mut self, ctx: &egui::Context) -> FeatureFormResult {
        let mut result = FeatureFormResult::Pending;
        egui::Window::new(&self.title)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for field in &mut self.fields {
                    ui.horizontal(|ui| {
                        ui.label(field.label);
                        ui.add(egui::TextEdit::singleline(&mut field.value).desired_width(260.0));
                        ui.small(field.hint);
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked()
                        && self
                            .fields
                            .iter()
                            .all(|field| !field.value.trim().is_empty())
                    {
                        result = FeatureFormResult::Created(self.command_text());
                    }
                    if ui.button("Cancel").clicked() {
                        result = FeatureFormResult::Cancelled;
                    }
                });
            });
        result
    }

    fn command_text(&self) -> String {
        let values: Vec<_> = self
            .fields
            .iter()
            .map(|field| field.value.trim().to_owned())
            .collect();
        let body = match self.verb.as_str() {
            "LEADER" | "MLEADER" | "HATCH" => normalize_points(&values[0]),
            "DIMLINEAR" | "DIMALIGNED" | "DIMRADIUS" | "DIMDIAMETRIC" | "DIMANGULAR" => {
                values.join(";")
            }
            "SPLINE" => format!("{} {}", values[0], normalize_points_spaced(&values[1])),
            "ARRAYPOLAR" => values.join(" "),
            "XATTACH" => values
                .iter()
                .map(|value| encode_command_arg(value))
                .collect::<Vec<_>>()
                .join(" "),
            "IMAGEATTACH" | "PLOT" | "EXPORTPDF" => {
                let mut encoded = values;
                if let Some(path) = encoded.first().cloned() {
                    encoded[0] = encode_command_arg(&path);
                }
                encoded.join(" ")
            }
            "LAYOUT" => {
                let operation = values.first().map(String::as_str).unwrap_or_default();
                let arguments = values.get(1).map(String::as_str).unwrap_or_default();
                let arguments = match operation.to_ascii_uppercase().as_str() {
                    "NEW" | "SET" | "DELETE" => encode_command_arg(arguments),
                    _ => arguments.to_owned(),
                };
                format!("{} {}", operation, arguments)
            }
            _ => values.join(" "),
        };
        format!("{} {}", self.verb, body)
    }
}

/// Encode one form field as a single command argument when it contains spaces
/// or quote characters. Double quotes are doubled so the command tokenizer can
/// recover the original field without interpreting backslashes as escapes.
fn encode_command_arg(value: &str) -> String {
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || value.contains('"')
        || value.contains('\'')
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn point_tokens(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|c: char| c == ';' || c.is_whitespace())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn normalize_points(value: &str) -> String {
    point_tokens(value).collect::<Vec<_>>().join(";")
}
fn normalize_points_spaced(value: &str) -> String {
    point_tokens(value).collect::<Vec<_>>().join(" ")
}
