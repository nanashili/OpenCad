use super::commands::parse_point;
use crate::{Point3, Ray3, XLine3};

/// Parsed construction command payload for the parent dispatcher to persist as
/// an Entity once XLINE/RAY variants are registered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstructionCommand {
    XLine(XLine3),
    Ray(Ray3),
}

pub(super) fn parse_construction_command(
    verb: &str,
    tokens: &[&str],
    elevation: f64,
) -> Option<Result<ConstructionCommand, String>> {
    let result = match verb.to_ascii_uppercase().as_str() {
        "XLINE" => parse_xline(tokens, elevation).map(ConstructionCommand::XLine),
        "RAY" => parse_ray(tokens, elevation).map(ConstructionCommand::Ray),
        _ => return None,
    };
    Some(result)
}

fn parse_xline(tokens: &[&str], elevation: f64) -> Result<XLine3, String> {
    match tokens {
        [origin, through] => XLine3::from_points(
            parse_point(origin, elevation)?,
            parse_point(through, elevation)?,
        )
        .map_err(|e| e.to_string()),
        [mode] if mode.eq_ignore_ascii_case("HORIZONTAL") => XLine3::new(
            Point3::new(0.0, 0.0, elevation).map_err(|e| e.to_string())?,
            Point3::new(1.0, 0.0, 0.0).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string()),
        [mode] if mode.eq_ignore_ascii_case("VERTICAL") => XLine3::new(
            Point3::new(0.0, 0.0, elevation).map_err(|e| e.to_string())?,
            Point3::new(0.0, 1.0, 0.0).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string()),
        _ => Err("XLINE syntax: XLINE origin through, or XLINE HORIZONTAL|VERTICAL.".into()),
    }
}
fn parse_ray(tokens: &[&str], elevation: f64) -> Result<Ray3, String> {
    if tokens.len() != 2 {
        return Err("RAY syntax: RAY origin through.".into());
    }
    Ray3::from_points(
        parse_point(tokens[0], elevation)?,
        parse_point(tokens[1], elevation)?,
    )
    .map_err(|e| e.to_string())
}
