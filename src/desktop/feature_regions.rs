//! Region command parsing and selection-level boolean helpers.

use crate::{Entity, GeometryError, Region2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RegionOperation {
    Region,
    Union,
    Subtract,
    Intersect,
}

pub(super) fn parse_region_command(
    verb: &str,
    tokens: &[&str],
) -> Option<Result<RegionOperation, String>> {
    let operation = match verb.to_ascii_uppercase().as_str() {
        "REGION" => RegionOperation::Region,
        "UNION" => RegionOperation::Union,
        "SUBTRACT" => RegionOperation::Subtract,
        "INTERSECT" => RegionOperation::Intersect,
        _ => return None,
    };
    if !tokens.is_empty() {
        return Some(Err(format!(
            "{} does not take command arguments; select closed planar boundaries first.",
            verb.to_ascii_uppercase()
        )));
    }
    Some(Ok(operation))
}

/// Convert selected planar entities into one or more hatch-backed results.
/// The caller owns the surrounding Editor transaction and replaces the
/// selected entities after this function returns successfully.
pub(super) fn region_operation(
    entities: &[Entity],
    selection: &[usize],
    operation: RegionOperation,
) -> Result<Vec<Entity>, String> {
    if selection.is_empty() {
        return Err("Select closed planar boundaries first.".into());
    }
    let regions = selection
        .iter()
        .map(|index| {
            let entity = entities
                .get(*index)
                .ok_or_else(|| "Selection contains an invalid entity index.".to_owned())?;
            Region2::from_entity(entity).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    match operation {
        RegionOperation::Region => regions
            .iter()
            .map(|region| {
                region
                    .to_hatch()
                    .map(Entity::Hatch)
                    .map_err(|e| e.to_string())
            })
            .collect(),
        RegionOperation::Union => fold_regions(regions, |a, b| a.union(b)),
        RegionOperation::Subtract => {
            let mut iter = regions.into_iter();
            let mut result = iter.next().expect("selection checked");
            for region in iter {
                result = result.subtract(&region).map_err(|e| e.to_string())?;
            }
            if result.is_empty() {
                return Ok(Vec::new());
            }
            Ok(vec![Entity::Hatch(
                result.to_hatch().map_err(|e| e.to_string())?,
            )])
        }
        RegionOperation::Intersect => fold_regions(regions, |a, b| a.intersect(b)),
    }
}

fn fold_regions(
    regions: Vec<Region2>,
    operation: impl Fn(&Region2, &Region2) -> Result<Region2, GeometryError>,
) -> Result<Vec<Entity>, String> {
    let mut iter = regions.into_iter();
    let mut result = iter.next().expect("selection checked");
    for region in iter {
        result = operation(&result, &region).map_err(|e| e.to_string())?;
    }
    if result.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![Entity::Hatch(
        result.to_hatch().map_err(|e| e.to_string())?,
    )])
}
