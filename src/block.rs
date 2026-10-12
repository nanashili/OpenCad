//! Reusable block definitions and their placed INSERT instances.
//!
//! Blocks keep their geometry in local coordinates.  An [`Insert`] refers to a
//! definition by name, so definitions remain acyclic even when an application
//! stores them in a drawing-wide library.

use crate::{Arc3, Circle3, Entity, GeometryError, Line3, Point3, PolylineWidth};
use std::f64::consts::TAU;

/// A tag and default value attached to a block definition.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeDefinition {
    tag: String,
    prompt: String,
    default_value: String,
    position: Point3,
}

impl AttributeDefinition {
    pub fn new(
        tag: impl Into<String>,
        prompt: impl Into<String>,
        default_value: impl Into<String>,
        position: Point3,
    ) -> Self {
        Self {
            tag: tag.into(),
            prompt: prompt.into(),
            default_value: default_value.into(),
            position,
        }
    }
    pub fn tag(&self) -> &str {
        &self.tag
    }
    pub fn prompt(&self) -> &str {
        &self.prompt
    }
    pub fn default_value(&self) -> &str {
        &self.default_value
    }
    pub fn position(&self) -> Point3 {
        self.position
    }
}

/// A value supplied for one definition when a block is inserted.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    tag: String,
    value: String,
    position: Option<Point3>,
}

impl Attribute {
    pub fn new(tag: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            value: value.into(),
            position: None,
        }
    }
    pub fn with_position(mut self, position: Point3) -> Self {
        self.position = Some(position);
        self
    }
    pub fn tag(&self) -> &str {
        &self.tag
    }
    pub fn value(&self) -> &str {
        &self.value
    }
    pub fn position(&self) -> Option<Point3> {
        self.position
    }
}

/// A named collection of local entities and optional attribute definitions.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDefinition {
    name: String,
    base: Point3,
    entities: Vec<Entity>,
    attribute_definitions: Vec<AttributeDefinition>,
    xref_path: Option<String>,
    xref_overlay: bool,
}

impl BlockDefinition {
    pub fn new(name: impl Into<String>, base: Point3, entities: Vec<Entity>) -> Self {
        Self {
            name: name.into(),
            base,
            entities,
            attribute_definitions: Vec::new(),
            xref_path: None,
            xref_overlay: false,
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn base(&self) -> Point3 {
        self.base
    }
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }
    pub fn attribute_definitions(&self) -> &[AttributeDefinition] {
        &self.attribute_definitions
    }
    pub fn xref_path(&self) -> Option<&str> {
        self.xref_path.as_deref()
    }
    pub fn xref_overlay(&self) -> bool {
        self.xref_overlay
    }
    pub fn set_xref_source(&mut self, path: Option<impl Into<String>>, overlay: bool) {
        self.xref_path = path.map(Into::into);
        self.xref_overlay = overlay;
    }
    pub fn set_entities(&mut self, entities: Vec<Entity>) {
        self.entities = entities;
    }
    pub fn set_attribute_definitions(&mut self, definitions: Vec<AttributeDefinition>) {
        self.attribute_definitions = definitions;
    }
    pub fn add_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
    }
    pub fn add_attribute_definition(&mut self, definition: AttributeDefinition) {
        self.attribute_definitions.push(definition);
    }
}

/// A placed reference to a block definition.
#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    block_name: String,
    insertion: Point3,
    scale: [f64; 3],
    rotation: f64,
    attributes: Vec<Attribute>,
    rows: Option<usize>,
    columns: Option<usize>,
    row_spacing: f64,
    column_spacing: f64,
}

/// Descriptive alias for APIs that distinguish block inserts from other inserts.
pub type BlockInsert = Insert;

impl Insert {
    pub fn new(block_name: impl Into<String>, insertion: Point3) -> Self {
        Self {
            block_name: block_name.into(),
            insertion,
            scale: [1.0; 3],
            rotation: 0.0,
            attributes: Vec::new(),
            rows: None,
            columns: None,
            row_spacing: 0.0,
            column_spacing: 0.0,
        }
    }
    pub fn block_name(&self) -> &str {
        &self.block_name
    }
    pub fn insertion(&self) -> Point3 {
        self.insertion
    }
    pub fn scale(&self) -> [f64; 3] {
        self.scale
    }
    pub fn rotation(&self) -> f64 {
        self.rotation
    }
    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }
    pub fn rows(&self) -> Option<usize> {
        self.rows
    }
    pub fn columns(&self) -> Option<usize> {
        self.columns
    }
    pub fn row_spacing(&self) -> f64 {
        self.row_spacing
    }
    pub fn column_spacing(&self) -> f64 {
        self.column_spacing
    }

    pub fn set_scale(&mut self, scale: [f64; 3]) -> Result<(), GeometryError> {
        if scale.iter().any(|v| !v.is_finite()) {
            return Err(GeometryError::NonFiniteParameter);
        }
        if scale.iter().any(|v| *v <= 0.0) {
            return Err(GeometryError::NonPositiveScale);
        }
        self.scale = scale;
        Ok(())
    }
    pub fn set_rotation(&mut self, rotation: f64) -> Result<(), GeometryError> {
        if !rotation.is_finite() {
            return Err(GeometryError::NonFiniteAngle);
        }
        self.rotation = rotation.rem_euclid(TAU);
        Ok(())
    }
    pub fn set_attributes(&mut self, attributes: Vec<Attribute>) {
        self.attributes = attributes;
    }
    pub fn set_array(
        &mut self,
        rows: Option<usize>,
        columns: Option<usize>,
        row_spacing: f64,
        column_spacing: f64,
    ) -> Result<(), GeometryError> {
        if rows == Some(0)
            || columns == Some(0)
            || !row_spacing.is_finite()
            || !column_spacing.is_finite()
        {
            return Err(GeometryError::NonFiniteParameter);
        }
        self.rows = rows;
        self.columns = columns;
        self.row_spacing = row_spacing;
        self.column_spacing = column_spacing;
        Ok(())
    }

    /// Explode one definition into transformed entities.  Array offsets are
    /// applied in the definition's local XY axes before the INSERT transform.
    pub fn explode(&self, definition: &BlockDefinition) -> Result<Vec<Entity>, GeometryError> {
        if self.block_name != definition.name {
            return Err(GeometryError::UnsupportedPolylineConversion);
        }
        let rows = self.rows.unwrap_or(1);
        let columns = self.columns.unwrap_or(1);
        let mut result = Vec::with_capacity(definition.entities.len() * rows * columns);
        for row in 0..rows {
            for column in 0..columns {
                let offset = Point3::new(
                    column as f64 * self.column_spacing,
                    row as f64 * self.row_spacing,
                    0.0,
                )?;
                for entity in &definition.entities {
                    result.push(transform_entity(entity, definition.base, self, offset)?);
                }
            }
        }
        Ok(result)
    }
}

fn transform_point(
    point: Point3,
    base: Point3,
    insert: &Insert,
    offset: Point3,
) -> Result<Point3, GeometryError> {
    let x = (point.x() - base.x() + offset.x()) * insert.scale[0];
    let y = (point.y() - base.y() + offset.y()) * insert.scale[1];
    let z = (point.z() - base.z()) * insert.scale[2];
    let (sin, cos) = insert.rotation.sin_cos();
    Point3::new(
        cos.mul_add(x, -sin * y) + insert.insertion.x(),
        sin.mul_add(x, cos * y) + insert.insertion.y(),
        z + insert.insertion.z(),
    )
    .map_err(|_| GeometryError::NonFiniteResult)
}

fn transform_entity(
    entity: &Entity,
    base: Point3,
    insert: &Insert,
    offset: Point3,
) -> Result<Entity, GeometryError> {
    let point = |p| transform_point(p, base, insert, offset);
    match entity {
        Entity::Insert(nested) => {
            let mut result = nested.clone();
            result.insertion = point(nested.insertion)?;
            result.scale = [
                nested.scale[0] * insert.scale[0],
                nested.scale[1] * insert.scale[1],
                nested.scale[2] * insert.scale[2],
            ];
            result.rotation += insert.rotation;
            result.column_spacing *= insert.scale[0];
            result.row_spacing *= insert.scale[1];
            Ok(Entity::Insert(result))
        }
        Entity::Line(line) => Ok(Entity::Line(Line3::new(
            point(line.start())?,
            point(line.end())?,
        ))),
        Entity::Circle(circle) if insert.scale[0] == insert.scale[1] => Ok(Entity::Circle(
            Circle3::new(point(circle.center())?, circle.radius() * insert.scale[0])?,
        )),
        Entity::Arc(arc) if insert.scale[0] == insert.scale[1] => Ok(Entity::Arc(Arc3::new(
            point(arc.center())?,
            arc.radius() * insert.scale[0],
            arc.start_angle() + insert.rotation,
            arc.end_angle() + insert.rotation,
        )?)),
        Entity::Polyline(polyline) => {
            if insert.scale[0] != insert.scale[1]
                && polyline
                    .widths()
                    .iter()
                    .any(|w| w.start() != 0.0 || w.end() != 0.0)
            {
                return Err(GeometryError::UnsupportedPolylineConversion);
            }
            let mut result = polyline.clone();
            result.set_vertices(
                polyline
                    .vertices()
                    .iter()
                    .copied()
                    .map(point)
                    .collect::<Result<_, _>>()?,
            )?;
            if insert.scale[0] == insert.scale[1] {
                result.set_widths(
                    polyline
                        .widths()
                        .iter()
                        .map(|w| {
                            PolylineWidth::new(
                                w.start() * insert.scale[0],
                                w.end() * insert.scale[0],
                            )
                        })
                        .collect::<Result<_, _>>()?,
                )?;
            }
            Ok(Entity::Polyline(result))
        }
        other if insert.scale[0] == insert.scale[1] && insert.scale[1] == insert.scale[2] => {
            let shifted = other.translated(
                offset.x() - base.x(),
                offset.y() - base.y(),
                offset.z() - base.z(),
            )?;
            let scaled = shifted.transformed(crate::Transform::scale(
                Point3::new(0.0, 0.0, 0.0)?,
                insert.scale[0],
            )?)?;
            let rotated = scaled.transformed(crate::Transform::rotate_xy(
                Point3::new(0.0, 0.0, 0.0)?,
                insert.rotation,
            )?)?;
            Ok(rotated.translated(
                insert.insertion.x(),
                insert.insertion.y(),
                insert.insertion.z(),
            )?)
        }
        _ => Err(GeometryError::UnsupportedPolylineConversion),
    }
}
