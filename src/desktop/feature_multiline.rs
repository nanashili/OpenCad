//! Command-facing helpers for MLINE and MLINESTYLE integration.

use crate::{MLine2, MLineJustification, MLineStyle, Point3};

pub fn parse_mline(tokens: &[&str], style: MLineStyle) -> Result<MLine2, String> {
    if tokens.len() < 2 {
        return Err("MLINE requires at least two points.".into());
    }
    let mut closed = false;
    let mut vertices = Vec::new();
    for token in tokens {
        if token.eq_ignore_ascii_case("CLOSE") {
            closed = true;
            continue;
        }
        vertices.push(parse_point(token)?);
    }
    let mut result = MLine2::new(vertices, style).map_err(|e| e.to_string())?;
    result.set_closed(closed).map_err(|e| e.to_string())?;
    Ok(result)
}

pub fn create_mlstyle(name: &str, offsets: &[f64]) -> Result<MLineStyle, String> {
    MLineStyle::new(name, offsets.to_vec()).map_err(|e| e.to_string())
}

pub fn edit_mline(
    mline: &mut MLine2,
    scale: Option<f64>,
    justification: Option<MLineJustification>,
    closed: Option<bool>,
    style: Option<MLineStyle>,
) -> Result<(), String> {
    if let Some(value) = scale {
        mline.set_scale(value).map_err(|e| e.to_string())?;
    }
    if let Some(value) = justification {
        mline.set_justification(value);
    }
    if let Some(value) = closed {
        mline.set_closed(value).map_err(|e| e.to_string())?;
    }
    if let Some(value) = style {
        *mline.style_mut() = value;
    }
    Ok(())
}

fn parse_point(value: &str) -> Result<Point3, String> {
    let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() < 2 || parts.len() > 3 {
        return Err(format!("Invalid point '{value}'. Use x,y[,z]."));
    }
    let x = parts[0]
        .parse::<f64>()
        .map_err(|_| format!("Invalid point '{value}'."))?;
    let y = parts[1]
        .parse::<f64>()
        .map_err(|_| format!("Invalid point '{value}'."))?;
    let z = parts.get(2).map_or(Ok(0.0), |v| {
        v.parse::<f64>()
            .map_err(|_| format!("Invalid point '{value}'."))
    })?;
    Point3::new(x, y, z).map_err(|e| e.to_string())
}
