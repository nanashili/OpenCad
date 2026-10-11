//! ASCII R12 geometry exchange with preservation of existing document records.
//! Geometry uses finite coordinates and named layers. Other table and block
//! contents are retained without interpreting their contents.

use crate::{
    Arc3, Circle3, Entity, EntityStyle, EntityStyleError, Layer, LayerError, Line3, Linetype,
    LinetypeError, Point3, Polyline2,
};
use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Write};
use std::str::SplitInclusive;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DxfErrorKind {
    UnexpectedEnd,
    InvalidGroupCode,
    Expected(&'static str),
    UnsupportedSection(String),
    DuplicateSection(String),
    UnsupportedVersion(String),
    UnsupportedEntity(String),
    UnsupportedProperty(u16),
    MissingField(u16),
    DuplicateField(u16),
    InvalidCoordinate(u16),
    InvalidRadius,
    InvalidHandle(u16),
    DuplicateHandle(String),
    Layer(LayerError),
    InvalidLayerValue(u16),
    Linetype(LinetypeError),
    EntityStyle(EntityStyleError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxfError {
    pub line: usize,
    pub kind: DxfErrorKind,
}

impl fmt::Display for DxfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DXF line {}: ", self.line)?;
        match &self.kind {
            DxfErrorKind::UnexpectedEnd => write!(f, "unexpected end of file"),
            DxfErrorKind::InvalidGroupCode => write!(f, "invalid group code"),
            DxfErrorKind::Expected(expected) => write!(f, "expected {expected}"),
            DxfErrorKind::UnsupportedSection(name) => write!(f, "unsupported section {name}"),
            DxfErrorKind::DuplicateSection(name) => write!(f, "duplicate section {name}"),
            DxfErrorKind::UnsupportedVersion(version) => write!(f, "unsupported version {version}"),
            DxfErrorKind::UnsupportedEntity(name) => write!(f, "unsupported entity {name}"),
            DxfErrorKind::UnsupportedProperty(code) => {
                write!(f, "unsupported property in group {code}")
            }
            DxfErrorKind::MissingField(code) => write!(f, "missing required group {code}"),
            DxfErrorKind::DuplicateField(code) => write!(f, "duplicate group {code}"),
            DxfErrorKind::InvalidCoordinate(code) => {
                write!(f, "group {code} requires a finite coordinate")
            }
            DxfErrorKind::InvalidRadius => {
                write!(f, "group 40 requires a finite, nonnegative radius")
            }
            DxfErrorKind::InvalidHandle(code) => write!(
                f,
                "group {code} requires a handle of 1 to 16 hexadecimal digits"
            ),
            DxfErrorKind::DuplicateHandle(handle) => write!(f, "duplicate handle {handle}"),
            DxfErrorKind::Layer(error) => error.fmt(f),
            DxfErrorKind::InvalidLayerValue(code) => {
                write!(f, "invalid layer value in group {code}")
            }
            DxfErrorKind::Linetype(error) => error.fmt(f),
            DxfErrorKind::EntityStyle(error) => error.fmt(f),
        }
    }
}

impl Error for DxfError {}

#[derive(Clone, Copy)]
struct Group<'a> {
    code: u16,
    value: &'a str,
    line: usize,
    start: usize,
    value_start: usize,
    value_end: usize,
}

struct Groups<'a> {
    lines: SplitInclusive<'a, char>,
    pending: Option<Group<'a>>,
    next_line: usize,
    offset: usize,
}

impl<'a> Groups<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            lines: input.split_inclusive('\n'),
            pending: None,
            next_line: 1,
            offset: 0,
        }
    }

    fn next(&mut self) -> Result<Option<Group<'a>>, DxfError> {
        if let Some(group) = self.pending.take() {
            return Ok(Some(group));
        }
        loop {
            match self.next_pair()? {
                Some(group) if group.code == 999 => continue,
                group => return Ok(group),
            }
        }
    }

    fn next_pair(&mut self) -> Result<Option<Group<'a>>, DxfError> {
        let Some(code_line) = self.lines.next() else {
            return Ok(None);
        };
        let line = self.next_line;
        let start = self.offset;
        self.next_line += 1;
        self.offset += code_line.len();
        let code = code_line
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|code| *code <= 1071)
            .ok_or(DxfError {
                line,
                kind: DxfErrorKind::InvalidGroupCode,
            })?;
        let raw_value = self.lines.next().ok_or(DxfError {
            line: self.next_line,
            kind: DxfErrorKind::UnexpectedEnd,
        })?;
        let value_start = self.offset;
        self.offset += raw_value.len();
        let value = raw_value
            .strip_suffix('\n')
            .map_or(raw_value, |value| value.strip_suffix('\r').unwrap_or(value));
        self.next_line += 1;
        Ok(Some(Group {
            code,
            value,
            line,
            start,
            value_start,
            value_end: value_start + value.len(),
        }))
    }

    fn peek(&mut self) -> Result<Option<Group<'a>>, DxfError> {
        if self.pending.is_none() {
            self.pending = self.next()?;
        }
        Ok(self.pending)
    }

    fn required(&mut self) -> Result<Group<'a>, DxfError> {
        self.next()?.ok_or(DxfError {
            line: self.next_line,
            kind: DxfErrorKind::UnexpectedEnd,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct Coordinate {
    value: f64,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct EntityRecord {
    original: Entity,
    source_line: usize,
    start: usize,
    end: usize,
    fields_end: usize,
    fields: Vec<(u16, Coordinate)>,
    properties: EntityProperties,
    polyline: Option<PolylineRecord>,
}

#[derive(Clone, Debug)]
struct EntityProperties {
    layer: TextField,
    style: EntityStyle,
    color: Option<Coordinate>,
    linetype: Option<TextField>,
    scale: Option<Coordinate>,
}

#[derive(Clone, Debug)]
struct PolylineRecord {
    vertices: Vec<ChildRecord>,
    seqend: ChildRecord,
}

#[derive(Clone, Debug)]
struct ChildRecord {
    start: usize,
    end: usize,
    fields: Vec<(u16, Coordinate)>,
    properties: EntityProperties,
}

#[derive(Clone, Debug)]
struct TextField {
    value: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct LayerRecord {
    original: Layer,
    flags: Coordinate,
    color: Coordinate,
    linetype: TextField,
}

#[derive(Clone, Debug)]
struct SymbolTable {
    start: usize,
    insertion: usize,
    header_end: usize,
    count: Option<Coordinate>,
}

#[derive(Clone, Debug)]
struct LayerData {
    layers: Vec<Layer>,
    records: Vec<Option<LayerRecord>>,
    table: Option<SymbolTable>,
    tables_end: Option<usize>,
    section_insertion: Option<usize>,
}

#[derive(Clone, Debug)]
struct LinetypeData {
    types: Vec<Linetype>,
    imported: Vec<bool>,
    table: Option<SymbolTable>,
    scale: f64,
    scale_field: Option<Coordinate>,
    header_end: Option<usize>,
}

/// An R12 document with editable geometry and layers, retaining source records.
#[derive(Clone, Debug)]
pub struct DxfDocument {
    source: String,
    entities: Vec<Entity>,
    records: Vec<Option<EntityRecord>>,
    removed: Vec<(usize, usize)>,
    insertion: usize,
    entity_layers: Vec<String>,
    entity_styles: Vec<EntityStyle>,
    layer_data: LayerData,
    linetype_data: LinetypeData,
}

impl DxfDocument {
    /// Validate supported sections, entity fields, layer membership and handles.
    pub fn parse(input: &str) -> Result<Self, DxfError> {
        let mut groups = Groups::new(input);
        let mut seen = [false; 4];
        let mut previous_section = 0;
        let mut handles = HashSet::new();
        let mut entities = Vec::new();
        let mut records: Vec<Option<EntityRecord>> = Vec::new();
        let mut insertion = 0;
        loop {
            let marker = groups.required()?;
            if (marker.code, marker.value) == (0, "EOF") {
                if !seen[3] {
                    return Err(DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::Expected("ENTITIES section before EOF"),
                    });
                }
                if let Some(group) = groups.next_pair()? {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Expected("end of file after EOF"),
                    });
                }
                let layer_data = read_layers(input)?;
                let linetype_data = read_linetypes(input)?;
                let entity_layers = records
                    .iter()
                    .map(|record| {
                        let record: &EntityRecord = record.as_ref().expect("imported record");
                        if !layer_data.layers.iter().any(|layer| {
                            layer
                                .name
                                .eq_ignore_ascii_case(&record.properties.layer.value)
                        }) {
                            return Err(DxfError {
                                line: record.source_line,
                                kind: DxfErrorKind::Layer(LayerError::UnknownLayer(
                                    record.properties.layer.value.clone(),
                                )),
                            });
                        }
                        Ok(record.properties.layer.value.clone())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let entity_styles = records
                    .iter()
                    .map(|record| {
                        record
                            .as_ref()
                            .expect("imported record")
                            .properties
                            .style
                            .clone()
                    })
                    .collect();
                return Ok(Self {
                    source: input.to_owned(),
                    entities,
                    records,
                    removed: Vec::new(),
                    insertion,
                    entity_layers,
                    entity_styles,
                    layer_data,
                    linetype_data,
                });
            }
            if (marker.code, marker.value) != (0, "SECTION") {
                return Err(DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::Expected("SECTION or EOF in group 0"),
                });
            }
            let section = groups.required()?;
            if section.code != 2 {
                return Err(DxfError {
                    line: section.line,
                    kind: DxfErrorKind::Expected("section name in group 2"),
                });
            }
            let index = match section.value {
                "HEADER" => 0,
                "TABLES" => 1,
                "BLOCKS" => 2,
                "ENTITIES" => 3,
                name => {
                    return Err(DxfError {
                        line: section.line,
                        kind: DxfErrorKind::UnsupportedSection(name.to_owned()),
                    });
                }
            };
            if seen[index] {
                return Err(DxfError {
                    line: section.line,
                    kind: DxfErrorKind::DuplicateSection(section.value.to_owned()),
                });
            }
            if index < previous_section {
                return Err(DxfError {
                    line: section.line,
                    kind: DxfErrorKind::Expected("HEADER, TABLES, BLOCKS, ENTITIES order"),
                });
            }
            seen[index] = true;
            previous_section = index;
            match section.value {
                "HEADER" => read_header(&mut groups)?,
                "TABLES" | "BLOCKS" => {
                    read_preserved_section(&mut groups, section.value, &mut handles)?
                }
                _ => loop {
                    let group = groups.required()?;
                    match (group.code, group.value) {
                        (0, "ENDSEC") => {
                            insertion = group.start;
                            break;
                        }
                        (0, "LINE" | "CIRCLE" | "ARC") => {
                            let (entity, record) = read_entity(&mut groups, group, &mut handles)?;
                            entities.push(entity);
                            records.push(Some(record));
                        }
                        (0, "POLYLINE") => {
                            let (entity, record) = read_polyline(&mut groups, group, &mut handles)?;
                            entities.push(entity);
                            records.push(Some(record));
                        }
                        (0, name) => {
                            return Err(DxfError {
                                line: group.line,
                                kind: DxfErrorKind::UnsupportedEntity(name.to_owned()),
                            });
                        }
                        _ => {
                            return Err(DxfError {
                                line: group.line,
                                kind: DxfErrorKind::Expected(
                                    "LINE, CIRCLE, ARC, POLYLINE, or ENDSEC in group 0",
                                ),
                            });
                        }
                    }
                },
            }
        }
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layer_data.layers
    }
    pub fn linetypes(&self) -> &[Linetype] {
        &self.linetype_data.types
    }
    pub fn linetype(&self, name: &str) -> Option<&Linetype> {
        self.linetypes()
            .iter()
            .find(|kind| kind.name.eq_ignore_ascii_case(name))
    }
    pub fn linetype_scale(&self) -> f64 {
        self.linetype_data.scale
    }
    pub fn add_linetype(&mut self, kind: Linetype) -> Result<(), LinetypeError> {
        kind.validate()?;
        if self.linetype(&kind.name).is_some() {
            return Err(LinetypeError::DuplicateName(kind.name));
        }
        if self.linetypes().len() >= i16::MAX as usize {
            return Err(LinetypeError::TooManyLinetypes);
        }
        self.linetype_data.types.push(kind);
        self.linetype_data.imported.push(false);
        Ok(())
    }
    pub fn set_layer_linetype(&mut self, layer: &str, name: &str) -> Result<bool, LinetypeError> {
        if name.eq_ignore_ascii_case("BYLAYER") || name.eq_ignore_ascii_case("BYBLOCK") {
            return Err(LinetypeError::UnknownLinetype(name.into()));
        }
        let canonical = self
            .linetype(name)
            .ok_or_else(|| LinetypeError::UnknownLinetype(name.into()))?
            .name
            .clone();
        let layer = self
            .layer_data
            .layers
            .iter_mut()
            .find(|item| item.name.eq_ignore_ascii_case(layer))
            .ok_or_else(|| LinetypeError::UnknownLayer(layer.into()))?;
        if layer.linetype.eq_ignore_ascii_case(&canonical) {
            return Ok(false);
        }
        layer.linetype = canonical;
        Ok(true)
    }
    pub fn set_linetype_scale(&mut self, scale: f64) -> Result<bool, LinetypeError> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(LinetypeError::InvalidScale);
        }
        if scale == self.linetype_data.scale {
            return Ok(false);
        }
        self.linetype_data.scale = scale;
        Ok(true)
    }
    /// Resolve top-level inheritance. Unknown source names display continuously
    /// without rewriting their stored values.
    pub fn linetype_for_entity(&self, index: usize) -> Option<&Linetype> {
        let style = self.entity_style(index)?;
        let layer = self.layer_for_entity(index)?;
        let name = if style.linetype.eq_ignore_ascii_case("BYBLOCK") {
            "CONTINUOUS"
        } else if style.linetype.eq_ignore_ascii_case("BYLAYER") {
            layer.linetype()
        } else {
            &style.linetype
        };
        self.linetype(name).or_else(|| self.linetype("CONTINUOUS"))
    }
    pub fn entity_style(&self, index: usize) -> Option<&EntityStyle> {
        self.entity_styles.get(index)
    }
    /// Resolve color against the entity's layer and drawing background.
    pub fn entity_rgb(&self, index: usize, on_white: bool) -> Option<[u8; 3]> {
        let style = self.entity_style(index)?;
        if style.color == 256 {
            return self
                .layer_for_entity(index)
                .map(|layer| layer.rgb(on_white));
        }
        let index = if (1..=255).contains(&style.color) {
            style.color
        } else {
            7
        };
        Some(crate::color::layer_rgb(index, on_white))
    }
    pub fn effective_linetype_scale(&self, index: usize) -> Option<f64> {
        self.entity_style(index)
            .map(|style| style.scale * self.linetype_scale())
    }
    pub fn set_entity_color(&mut self, index: usize, color: i16) -> Result<bool, EntityStyleError> {
        if !(0..=256).contains(&color) {
            return Err(EntityStyleError::InvalidColor);
        }
        let style = self
            .entity_styles
            .get_mut(index)
            .ok_or(EntityStyleError::UnknownEntity(index))?;
        if style.color == color {
            return Ok(false);
        }
        style.color = color;
        Ok(true)
    }
    pub fn set_entity_linetype(
        &mut self,
        index: usize,
        name: &str,
    ) -> Result<bool, EntityStyleError> {
        let name = if name.eq_ignore_ascii_case("BYLAYER") {
            "BYLAYER".to_owned()
        } else if name.eq_ignore_ascii_case("BYBLOCK") {
            "BYBLOCK".to_owned()
        } else {
            self.linetype(name)
                .ok_or_else(|| EntityStyleError::UnknownLinetype(name.into()))?
                .name()
                .to_owned()
        };
        let style = self
            .entity_styles
            .get_mut(index)
            .ok_or(EntityStyleError::UnknownEntity(index))?;
        if style.linetype.eq_ignore_ascii_case(&name) {
            return Ok(false);
        }
        style.linetype = name;
        Ok(true)
    }
    pub fn set_entity_linetype_scale(
        &mut self,
        index: usize,
        scale: f64,
    ) -> Result<bool, EntityStyleError> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(EntityStyleError::InvalidScale);
        }
        // DXFIN omits nonpositive group 48. Use its documented default, while
        // retaining untouched source bytes. Zero input requests that default.
        let scale = if scale == 0.0 { 1.0 } else { scale };
        let style = self
            .entity_styles
            .get_mut(index)
            .ok_or(EntityStyleError::UnknownEntity(index))?;
        if style.scale == scale {
            return Ok(false);
        }
        style.scale = scale;
        Ok(true)
    }
    pub fn layer(&self, name: &str) -> Option<&Layer> {
        self.layers()
            .iter()
            .find(|layer| layer.name.eq_ignore_ascii_case(name))
    }
    pub fn entity_layer(&self, index: usize) -> Option<&str> {
        self.entity_layers.get(index).map(String::as_str)
    }
    pub fn layer_for_entity(&self, index: usize) -> Option<&Layer> {
        self.entity_layer(index).and_then(|name| self.layer(name))
    }
    pub fn add_layer(&mut self, name: &str) -> Result<(), LayerError> {
        let layer = Layer::new(name)?;
        if self.layer(name).is_some() {
            return Err(LayerError::DuplicateName(name.into()));
        }
        if self.layers().len() >= i16::MAX as usize {
            return Err(LayerError::TooManyLayers);
        }
        self.layer_data.layers.push(layer);
        self.layer_data.records.push(None);
        Ok(())
    }
    pub fn set_entity_layer(&mut self, index: usize, name: &str) -> Result<bool, LayerError> {
        let canonical = self
            .layer(name)
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?
            .name
            .clone();
        let current = self
            .entity_layers
            .get_mut(index)
            .ok_or(LayerError::MissingEntity(index))?;
        if current.eq_ignore_ascii_case(&canonical) {
            return Ok(false);
        }
        *current = canonical;
        Ok(true)
    }
    pub fn set_layer_visible(&mut self, name: &str, visible: bool) -> Result<bool, LayerError> {
        let layer = self
            .layer_data
            .layers
            .iter_mut()
            .find(|layer| layer.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?;
        let old = layer.clone();
        layer.set_visible(visible)?;
        Ok(*layer != old)
    }
    pub fn set_layer_locked(&mut self, name: &str, locked: bool) -> Result<bool, LayerError> {
        let layer = self
            .layer_data
            .layers
            .iter_mut()
            .find(|layer| layer.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?;
        let old = layer.flags;
        layer.set_locked(locked);
        Ok(layer.flags != old)
    }

    /// Change an ordinary ACI color, preserving off/frozen/locked state.
    pub fn set_layer_color(&mut self, name: &str, index: i16) -> Result<bool, LayerError> {
        let layer = self
            .layer_data
            .layers
            .iter_mut()
            .find(|layer| layer.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?;
        let old = layer.color;
        layer.set_color_index(index)?;
        Ok(layer.color != old)
    }

    pub fn lines(&self) -> impl Iterator<Item = Line3> + '_ {
        self.entities.iter().filter_map(|entity| match entity {
            Entity::Line(line) => Some(*line),
            Entity::Circle(_) | Entity::Arc(_) | Entity::Polyline(_) => None,
        })
    }

    /// Replace a LINE by zero-based index, returning its previous geometry.
    /// An absent index returns `None` without changing the document.
    pub fn replace_line(&mut self, index: usize, line: Line3) -> Option<Line3> {
        let entity_index = self
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| matches!(entity, Entity::Line(_)).then_some(index))
            .nth(index)?;
        match self.replace_entity(entity_index, Entity::Line(line))? {
            Entity::Line(old) => Some(old),
            Entity::Circle(_) | Entity::Arc(_) | Entity::Polyline(_) => {
                unreachable!("selected a LINE")
            }
        }
    }

    /// Update an entity without changing its type or identity. Returns `None`
    /// for an absent index or a type mismatch, leaving the document unchanged.
    pub fn replace_entity(&mut self, index: usize, entity: Entity) -> Option<Entity> {
        let old = self.entities.get_mut(index)?;
        if old.name() != entity.name() {
            return None;
        }
        Some(std::mem::replace(old, entity))
    }

    pub fn push(&mut self, entity: Entity) {
        self.push_on_layer(entity, "0").expect("layer 0 exists");
    }

    pub fn push_on_layer(&mut self, entity: Entity, name: &str) -> Result<(), LayerError> {
        self.push_with_style(entity, name, EntityStyle::default())
    }

    pub(crate) fn push_with_style(
        &mut self,
        entity: Entity,
        name: &str,
        style: EntityStyle,
    ) -> Result<(), LayerError> {
        let name = self
            .layer(name)
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?
            .name
            .clone();
        self.entities.push(entity);
        self.records.push(None);
        self.entity_layers.push(name);
        self.entity_styles.push(style);
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Option<Entity> {
        if index >= self.entities.len() {
            return None;
        }
        if let Some(record) = self.records.remove(index) {
            self.removed.push((record.start, record.end));
        }
        self.entity_layers.remove(index);
        self.entity_styles.remove(index);
        Some(self.entities.remove(index))
    }

    /// Preserve original bytes except explicitly edited geometry/layer/style values.
    pub fn to_dxf(&self) -> String {
        let mut changes: Vec<_> = self
            .removed
            .iter()
            .map(|&(start, end)| (start, end, String::new()))
            .collect();
        let mut added = String::new();
        for (((entity, record), layer), style) in self
            .entities
            .iter()
            .zip(&self.records)
            .zip(&self.entity_layers)
            .zip(&self.entity_styles)
        {
            let Some(record) = record else {
                write_entity(&mut added, entity, layer, style);
                continue;
            };
            let original = &record.properties;
            let changed = [
                layer != &original.layer.value,
                style.color != original.style.color,
                style.linetype != original.style.linetype,
                style.scale.to_bits() != original.style.scale.to_bits(),
            ];
            patch_entity_properties(
                &mut changes,
                original,
                layer,
                style,
                changed,
                record.fields_end,
            );
            if let (Entity::Polyline(polyline), Entity::Polyline(old), Some(source)) =
                (entity, &record.original, &record.polyline)
            {
                if polyline.is_closed() != old.is_closed()
                    && !record.fields.iter().any(|(code, _)| *code == 70)
                {
                    changes.push((
                        record.fields_end,
                        record.fields_end,
                        format!("70\n{}\n", u8::from(polyline.is_closed())),
                    ));
                }
                for (index, vertex) in source.vertices.iter().enumerate() {
                    let Some(point) = polyline.vertices().get(index) else {
                        changes.push((vertex.start, vertex.end, String::new()));
                        continue;
                    };
                    patch_entity_properties(
                        &mut changes,
                        &vertex.properties,
                        layer,
                        style,
                        changed,
                        vertex.end,
                    );
                    for (code, field) in &vertex.fields {
                        let value = match code {
                            10 => point.x(),
                            20 => point.y(),
                            30 if polyline.elevation() != old.elevation() => polyline.elevation(),
                            30 => field.value,
                            42 => polyline.bulges()[index],
                            _ => unreachable!("vertex coordinate"),
                        };
                        if value.to_bits() != field.value.to_bits() {
                            changes.push((field.start, field.end, value.to_string()));
                        }
                    }
                    if polyline.bulges()[index] != 0.0
                        && !vertex.fields.iter().any(|(code, _)| *code == 42)
                    {
                        changes.push((
                            vertex.end,
                            vertex.end,
                            format!("42\n{}\n", polyline.bulges()[index]),
                        ));
                    }
                }
                let mut vertices = String::new();
                for (index, point) in polyline
                    .vertices()
                    .iter()
                    .enumerate()
                    .skip(source.vertices.len())
                {
                    write_vertex(
                        &mut vertices,
                        *point,
                        polyline.bulges()[index],
                        layer,
                        style,
                    );
                }
                if !vertices.is_empty() {
                    changes.push((source.seqend.start, source.seqend.start, vertices));
                }
                patch_entity_properties(
                    &mut changes,
                    &source.seqend.properties,
                    layer,
                    style,
                    changed,
                    source.seqend.end,
                );
            }
            for (code, field) in &record.fields {
                let value = entity.coordinate(*code);
                if value.to_bits() != record.original.coordinate(*code).to_bits() {
                    changes.push((field.start, field.end, value.to_string()));
                }
            }
        }
        let mut tables = String::new();
        self.linetype_data
            .write_changes(&mut changes, &mut tables, self.layer_data.table.as_ref());
        self.layer_data.write_changes(&mut changes, &mut tables);
        if !tables.is_empty() {
            if let Some(at) = self.layer_data.tables_end {
                changes.push((at, at, tables));
            } else {
                let at = self
                    .layer_data
                    .section_insertion
                    .expect("ENTITIES section exists");
                changes.push((
                    at,
                    at,
                    format!("0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n"),
                ));
            }
        }
        if !added.is_empty() {
            changes.push((self.insertion, self.insertion, added));
        }
        // Insert fields at an entity boundary before deleting a following record.
        changes.sort_by_key(|change| (change.0, change.1));
        let mut output = String::with_capacity(self.source.len());
        let mut cursor = 0;
        for (start, end, value) in changes {
            output.push_str(&self.source[cursor..start]);
            output.push_str(&value);
            cursor = end;
        }
        output.push_str(&self.source[cursor..]);
        output
    }
}

impl Default for DxfDocument {
    fn default() -> Self {
        Self::parse(&write_dxf(&[])).expect("the minimal writer emits a valid empty drawing")
    }
}

fn layer_field(group: Group<'_>) -> Result<Coordinate, DxfError> {
    let value = group.value.trim().parse::<i16>().map_err(|_| DxfError {
        line: group.line,
        kind: DxfErrorKind::InvalidLayerValue(group.code),
    })?;
    Ok(Coordinate {
        value: f64::from(value),
        start: group.value_start,
        end: group.value_end,
    })
}

fn coordinate(group: Group<'_>) -> Result<Coordinate, DxfError> {
    let value = read_number(group)?;
    Ok(Coordinate {
        value,
        start: group.value_start,
        end: group.value_end,
    })
}

fn record_fields<'a>(groups: &mut Groups<'a>) -> Result<Vec<Group<'a>>, DxfError> {
    let mut fields = Vec::new();
    while groups.peek()?.is_some_and(|group| group.code != 0) {
        fields.push(groups.required()?);
    }
    Ok(fields)
}

fn unique_field<'a>(fields: &[Group<'a>], code: u16) -> Result<Option<Group<'a>>, DxfError> {
    let mut matching = fields.iter().filter(|field| field.code == code);
    let first = matching.next().copied();
    if let Some(duplicate) = matching.next() {
        return Err(DxfError {
            line: duplicate.line,
            kind: DxfErrorKind::DuplicateField(code),
        });
    }
    Ok(first)
}

fn read_linetypes(input: &str) -> Result<LinetypeData, DxfError> {
    let mut data = LinetypeData {
        types: vec![Linetype::continuous()],
        imported: vec![false],
        table: None,
        scale: 1.0,
        scale_field: None,
        header_end: None,
    };
    let mut groups = Groups::new(input);
    let mut in_header = false;
    let mut in_tables = false;
    let mut in_types = false;
    let mut names = HashSet::new();
    while let Some(group) = groups.next()? {
        match (group.code, group.value) {
            (0, "SECTION") => {
                let section = groups.required()?;
                in_header = section.value == "HEADER";
                in_tables = section.value == "TABLES";
            }
            (0, "ENDSEC") => {
                if in_header {
                    data.header_end = Some(group.start);
                }
                in_header = false;
                in_tables = false;
            }
            (9, "$LTSCALE") if in_header => {
                if data.scale_field.is_some() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::DuplicateField(9),
                    });
                }
                let field = groups.required()?;
                if field.code != 40 {
                    return Err(DxfError {
                        line: field.line,
                        kind: DxfErrorKind::Expected("LTSCALE in group 40"),
                    });
                }
                let value = coordinate(field)?;
                if value.value <= 0.0 {
                    return Err(DxfError {
                        line: field.line,
                        kind: DxfErrorKind::Linetype(LinetypeError::InvalidScale),
                    });
                }
                if groups
                    .peek()?
                    .is_some_and(|next| !matches!(next.code, 0 | 9))
                {
                    return Err(DxfError {
                        line: field.line,
                        kind: DxfErrorKind::Expected("one LTSCALE value"),
                    });
                }
                data.scale = value.value;
                data.scale_field = Some(value);
            }
            (0, "TABLE") if in_tables => {
                let fields = record_fields(&mut groups)?;
                in_types = fields
                    .iter()
                    .any(|field| field.code == 2 && field.value == "LTYPE");
                if !in_types {
                    continue;
                }
                if data.table.is_some() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Expected("one LTYPE table"),
                    });
                }
                unique_field(&fields, 2)?;
                let count = unique_field(&fields, 70)?.map(layer_field).transpose()?;
                if count.is_some_and(|count| count.value < 0.0) {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Linetype(LinetypeError::InvalidPattern),
                    });
                }
                let header_end = groups.peek()?.map_or(groups.offset, |field| field.start);
                data.table = Some(SymbolTable {
                    start: group.start,
                    insertion: 0,
                    header_end,
                    count,
                });
            }
            (0, "ENDTAB") if in_tables => {
                if in_types {
                    data.table.as_mut().expect("LTYPE header").insertion = group.start;
                }
                in_types = false;
            }
            (0, "LTYPE") if in_types => {
                let fields = record_fields(&mut groups)?;
                let required = |code| {
                    unique_field(&fields, code)?.ok_or(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::MissingField(code),
                    })
                };
                for field in &fields {
                    if !(matches!(field.code, 2 | 3 | 5 | 70 | 72 | 73 | 40 | 49)
                        || field.code == 74 && field.value.trim() == "0")
                    {
                        return Err(DxfError {
                            line: field.line,
                            kind: DxfErrorKind::UnsupportedProperty(field.code),
                        });
                    }
                }
                let name = required(2)?;
                if name.value.is_empty() {
                    return Err(DxfError {
                        line: name.line,
                        kind: DxfErrorKind::Linetype(LinetypeError::InvalidName),
                    });
                }
                if !names.insert(name.value.to_ascii_lowercase()) {
                    return Err(DxfError {
                        line: name.line,
                        kind: DxfErrorKind::Linetype(LinetypeError::DuplicateName(
                            name.value.into(),
                        )),
                    });
                }
                if required(72)?.value.trim() != "65" {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Expected("simple LTYPE alignment A (65)"),
                    });
                }
                let count = layer_field(required(73)?)?.value;
                if !(0.0..=12.0).contains(&count) {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Linetype(LinetypeError::InvalidPattern),
                    });
                }
                let pattern = fields
                    .iter()
                    .filter(|field| field.code == 49)
                    .map(|field| coordinate(*field).map(|field| field.value))
                    .collect::<Result<Vec<_>, _>>()?;
                if count as usize != pattern.len() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Linetype(LinetypeError::InvalidPattern),
                    });
                }
                let kind = Linetype {
                    name: name.value.into(),
                    description: unique_field(&fields, 3)?
                        .map_or("", |field| field.value)
                        .into(),
                    flags: layer_field(required(70)?)?.value as i16,
                    length: coordinate(required(40)?)?.value,
                    pattern,
                };
                kind.validate().map_err(|error| DxfError {
                    line: group.line,
                    kind: DxfErrorKind::Linetype(error),
                })?;
                if kind.name.eq_ignore_ascii_case("CONTINUOUS") {
                    if !kind.pattern.is_empty() {
                        return Err(DxfError {
                            line: group.line,
                            kind: DxfErrorKind::Expected("CONTINUOUS without pattern elements"),
                        });
                    }
                    data.types[0] = kind;
                    data.imported[0] = true;
                } else {
                    data.types.push(kind);
                    data.imported.push(true);
                }
            }
            _ => {}
        }
    }
    Ok(data)
}

impl SymbolTable {
    fn append(&self, changes: &mut Vec<(usize, usize, String)>, added: String, count: usize) {
        if added.is_empty() {
            return;
        }
        changes.push((self.insertion, self.insertion, added));
        if let Some(field) = self.count {
            changes.push((field.start, field.end, count.to_string()));
        } else {
            changes.push((self.header_end, self.header_end, format!("70\n{count}\n")));
        }
    }
}

impl LinetypeData {
    fn write_changes(
        &self,
        changes: &mut Vec<(usize, usize, String)>,
        tables: &mut String,
        layer_table: Option<&SymbolTable>,
    ) {
        if self
            .types
            .iter()
            .zip(&self.imported)
            .enumerate()
            .any(|(i, (_, imported))| i > 0 && !imported)
        {
            let mut added = String::new();
            for (kind, imported) in self.types.iter().zip(&self.imported) {
                if *imported {
                    continue;
                }
                write!(
                    added,
                    "0\nLTYPE\n2\n{}\n70\n{}\n3\n{}\n72\n65\n73\n{}\n40\n{}\n",
                    kind.name,
                    kind.flags,
                    kind.description,
                    kind.pattern.len(),
                    kind.length
                )
                .expect("String write");
                for value in &kind.pattern {
                    writeln!(added, "49\n{value}").expect("String write");
                }
            }
            if let Some(table) = &self.table {
                table.append(changes, added, self.types.len());
            } else {
                let table = format!(
                    "0\nTABLE\n2\nLTYPE\n70\n{}\n{added}0\nENDTAB\n",
                    self.types.len()
                );
                // R12 resolves a layer's linetype while reading its table.
                if let Some(layer_table) = layer_table {
                    changes.push((layer_table.start, layer_table.start, table));
                } else {
                    tables.push_str(&table);
                }
            }
        }
        if let Some(field) = self.scale_field {
            if self.scale != field.value {
                changes.push((field.start, field.end, self.scale.to_string()));
            }
        } else if self.scale != 1.0 {
            let variable = format!("9\n$LTSCALE\n40\n{}\n", self.scale);
            if let Some(at) = self.header_end {
                changes.push((at, at, variable));
            } else {
                changes.push((
                    0,
                    0,
                    format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n{variable}0\nENDSEC\n"),
                ));
            }
        }
    }
}

// Section framing and handles have already been validated. This second linear
// pass interprets only the layer table, retaining exact value spans for edits.
fn read_layers(input: &str) -> Result<LayerData, DxfError> {
    let mut data = LayerData {
        layers: vec![Layer::new("0").expect("valid default name")],
        records: vec![None],
        table: None,
        tables_end: None,
        section_insertion: None,
    };
    let mut groups = Groups::new(input);
    let mut in_tables = false;
    let mut in_layers = false;
    let mut names = HashSet::new();
    while let Some(group) = groups.next()? {
        match (group.code, group.value) {
            (0, "SECTION") => {
                let section = groups.required()?;
                in_tables = section.value == "TABLES";
                if data.section_insertion.is_none()
                    && matches!(section.value, "BLOCKS" | "ENTITIES")
                {
                    data.section_insertion = Some(group.start);
                }
            }
            (0, "ENDSEC") => {
                if in_tables {
                    data.tables_end = Some(group.start);
                }
                in_tables = false;
            }
            (0, "TABLE") if in_tables => {
                let fields = record_fields(&mut groups)?;
                let name = fields.iter().find(|field| field.code == 2);
                in_layers = name.is_some_and(|field| field.value == "LAYER");
                if !in_layers {
                    continue;
                }
                if data.table.is_some() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::Expected("one LAYER table"),
                    });
                }
                if fields.iter().filter(|field| field.code == 2).count() != 1 {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::DuplicateField(2),
                    });
                }
                let mut count = None;
                for field in fields.iter().filter(|field| field.code == 70) {
                    if count.is_some() {
                        return Err(DxfError {
                            line: field.line,
                            kind: DxfErrorKind::DuplicateField(70),
                        });
                    }
                    let value = layer_field(*field)?;
                    if value.value < 0.0 {
                        return Err(DxfError {
                            line: field.line,
                            kind: DxfErrorKind::InvalidLayerValue(70),
                        });
                    }
                    count = Some(value);
                }
                let header_end = groups.peek()?.map_or(groups.offset, |field| field.start);
                data.table = Some(SymbolTable {
                    start: group.start,
                    insertion: 0,
                    header_end,
                    count,
                });
            }
            (0, "ENDTAB") if in_tables => {
                if in_layers {
                    data.table.as_mut().expect("layer table header").insertion = group.start;
                }
                in_layers = false;
            }
            (0, "LAYER") if in_layers => {
                let fields = record_fields(&mut groups)?;
                let required = |code| -> Result<Group<'_>, DxfError> {
                    let mut found = fields.iter().filter(|field| field.code == code);
                    let value = found.next().copied().ok_or(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::MissingField(code),
                    })?;
                    if let Some(duplicate) = found.next() {
                        return Err(DxfError {
                            line: duplicate.line,
                            kind: DxfErrorKind::DuplicateField(code),
                        });
                    }
                    Ok(value)
                };
                let name = required(2)?;
                let linetype = required(6)?;
                if name.value.is_empty() || linetype.value.is_empty() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::InvalidLayerValue(if name.value.is_empty() {
                            2
                        } else {
                            6
                        }),
                    });
                }
                if !names.insert(name.value.to_ascii_lowercase()) {
                    return Err(DxfError {
                        line: name.line,
                        kind: DxfErrorKind::Layer(LayerError::DuplicateName(name.value.into())),
                    });
                }
                let flags = layer_field(required(70)?)?;
                let color = layer_field(required(62)?)?;
                // Native entmake and R12 export also retain 0 and 256. They are
                // preserved here although the layer color picker uses 1–255.
                if !(-256.0..=256.0).contains(&color.value) {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::InvalidLayerValue(62),
                    });
                }
                let layer = Layer {
                    name: name.value.into(),
                    flags: flags.value as i16,
                    color: color.value as i16,
                    linetype: linetype.value.into(),
                };
                let record = Some(LayerRecord {
                    original: layer.clone(),
                    flags,
                    color,
                    linetype: TextField {
                        value: linetype.value.into(),
                        start: linetype.value_start,
                        end: linetype.value_end,
                    },
                });
                if layer.name == "0" {
                    data.layers[0] = layer;
                    data.records[0] = record;
                } else {
                    data.layers.push(layer);
                    data.records.push(record);
                }
            }
            _ => {}
        }
    }
    if data.table.is_some() && data.records[0].is_none() {
        return Err(DxfError {
            line: 1,
            kind: DxfErrorKind::Expected("layer 0 in the LAYER table"),
        });
    }
    Ok(data)
}

impl LayerData {
    fn write_changes(&self, changes: &mut Vec<(usize, usize, String)>, tables: &mut String) {
        let mut added = String::new();
        for (layer, record) in self.layers.iter().zip(&self.records) {
            if let Some(record) = record {
                if layer.linetype != record.linetype.value {
                    changes.push((
                        record.linetype.start,
                        record.linetype.end,
                        layer.linetype.clone(),
                    ));
                }
                for (value, original, field) in [
                    (layer.flags, record.original.flags, record.flags),
                    (layer.color, record.original.color, record.color),
                ] {
                    if value != original {
                        changes.push((field.start, field.end, value.to_string()));
                    }
                }
            } else {
                write_layer(&mut added, layer);
            }
        }
        if let Some(table) = &self.table {
            table.append(changes, added, self.layers.len());
        } else if self.layers.len() > 1
            || self.layers[0] != Layer::new("0").expect("valid default name")
        {
            let table = format!(
                "0\nTABLE\n2\nLAYER\n70\n{}\n{added}0\nENDTAB\n",
                self.layers.len()
            );
            tables.push_str(&table);
        }
    }
}

fn write_layer(output: &mut String, layer: &Layer) {
    write!(
        output,
        "0\nLAYER\n2\n{}\n70\n{}\n62\n{}\n6\n{}\n",
        layer.name, layer.flags, layer.color, layer.linetype
    )
    .expect("writing to a String cannot fail");
}

/// Extract LINE geometry from a supported R12 document. Use `DxfDocument` to
/// retain its metadata when editing or writing an imported drawing.
pub fn read_dxf(input: &str) -> Result<Vec<Line3>, DxfError> {
    let document = DxfDocument::parse(input)?;
    document
        .entities
        .iter()
        .zip(&document.records)
        .map(|(entity, record)| match entity {
            Entity::Line(line) => Ok(*line),
            _ => Err(DxfError {
                line: record
                    .as_ref()
                    .expect("imported entity has a source record")
                    .source_line,
                kind: DxfErrorKind::UnsupportedEntity(entity.name().to_owned()),
            }),
        })
        .collect()
}

fn read_header(groups: &mut Groups<'_>) -> Result<(), DxfError> {
    let mut version_seen = false;
    loop {
        let group = groups.required()?;
        match (group.code, group.value) {
            (0, "ENDSEC") if version_seen => return Ok(()),
            (9, "$ACADVER") => {
                if version_seen {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::DuplicateField(9),
                    });
                }
                let version = groups.required()?;
                if version.code != 1 || version.value != "AC1009" {
                    return Err(DxfError {
                        line: version.line,
                        kind: DxfErrorKind::UnsupportedVersion(version.value.to_owned()),
                    });
                }
                version_seen = true;
            }
            (9, _) => {
                let value = groups.required()?;
                if value.code == 0 || value.code == 9 {
                    return Err(DxfError {
                        line: value.line,
                        kind: DxfErrorKind::Expected("HEADER variable value"),
                    });
                }
                while groups
                    .peek()?
                    .is_some_and(|group| group.code != 0 && group.code != 9)
                {
                    groups.next()?;
                }
            }
            _ => {
                return Err(DxfError {
                    line: group.line,
                    kind: DxfErrorKind::Expected("HEADER variable in group 9, including $ACADVER"),
                });
            }
        }
    }
}

fn read_preserved_section(
    groups: &mut Groups<'_>,
    section: &str,
    handles: &mut HashSet<u64>,
) -> Result<(), DxfError> {
    let (opening, closing) = if section == "TABLES" {
        ("TABLE", "ENDTAB")
    } else {
        ("BLOCK", "ENDBLK")
    };
    let mut container_open = false;
    let mut record_started = false;
    let mut record_name = "";
    loop {
        let group = groups.required()?;
        if group.code != 0 {
            if !record_started {
                return Err(DxfError {
                    line: group.line,
                    kind: DxfErrorKind::Expected("TABLE or BLOCK record"),
                });
            }
            // In legacy DIMSTYLE records group 5 is an arrow-block name.
            // Their object handle, when present, uses group 105 instead.
            if (group.code == 5 && record_name != "DIMSTYLE") || group.code == 105 {
                read_handle(group, handles)?;
            }
            continue;
        }
        if group.value == "ENDSEC" {
            return if container_open {
                Err(DxfError {
                    line: group.line,
                    kind: DxfErrorKind::Expected(closing),
                })
            } else {
                Ok(())
            };
        }
        record_name = group.value;
        if group.value == opening && !container_open {
            container_open = true;
            record_started = true;
        } else if group.value == closing && container_open {
            container_open = false;
            record_started = section == "BLOCKS";
        } else if container_open
            && !matches!(
                group.value,
                "SECTION" | "EOF" | "TABLE" | "ENDTAB" | "BLOCK" | "ENDBLK"
            )
        {
            record_started = true;
        } else {
            return Err(DxfError {
                line: group.line,
                kind: DxfErrorKind::Expected("matching TABLE/BLOCK framing"),
            });
        }
    }
}

fn read_handle(group: Group<'_>, handles: &mut HashSet<u64>) -> Result<(), DxfError> {
    let text = group.value;
    if text.is_empty() || text.len() > 16 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DxfError {
            line: group.line,
            kind: DxfErrorKind::InvalidHandle(group.code),
        });
    }
    let handle = u64::from_str_radix(text, 16).expect("validated hexadecimal handle fits u64");
    if !handles.insert(handle) {
        return Err(DxfError {
            line: group.line,
            kind: DxfErrorKind::DuplicateHandle(text.to_owned()),
        });
    }
    Ok(())
}

const COMMON_ENTITY_CODES: &[u16] = &[5, 8, 6, 62, 48];

fn read_entity_properties(
    fields: &[Group<'_>],
    line: usize,
    handles: &mut HashSet<u64>,
) -> Result<EntityProperties, DxfError> {
    if let Some(handle) = unique_field(fields, 5)? {
        read_handle(handle, handles)?;
    }
    let layer = unique_field(fields, 8)?.ok_or(DxfError {
        line,
        kind: DxfErrorKind::MissingField(8),
    })?;
    if layer.value.is_empty() {
        return Err(DxfError {
            line: layer.line,
            kind: DxfErrorKind::InvalidLayerValue(8),
        });
    }
    let text = |group: Group<'_>| TextField {
        value: group.value.into(),
        start: group.value_start,
        end: group.value_end,
    };
    let linetype = unique_field(fields, 6)?
        .map(|group| {
            if group.value.is_empty() {
                Err(DxfError {
                    line: group.line,
                    kind: DxfErrorKind::EntityStyle(EntityStyleError::UnknownLinetype(
                        String::new(),
                    )),
                })
            } else {
                Ok(text(group))
            }
        })
        .transpose()?;
    let color = unique_field(fields, 62)?
        .map(|group| {
            let value = group
                .value
                .trim()
                .parse::<i16>()
                .ok()
                .filter(|value| (0..=256).contains(value))
                .ok_or(DxfError {
                    line: group.line,
                    kind: DxfErrorKind::EntityStyle(EntityStyleError::InvalidColor),
                })?;
            Ok(Coordinate {
                value: f64::from(value),
                start: group.value_start,
                end: group.value_end,
            })
        })
        .transpose()?;
    let scale = unique_field(fields, 48)?.map(coordinate).transpose()?;
    Ok(EntityProperties {
        layer: text(layer),
        style: EntityStyle {
            color: color.map_or(256, |field| field.value as i16),
            linetype: linetype
                .as_ref()
                .map_or_else(|| "BYLAYER".into(), |field| field.value.clone()),
            scale: scale
                .filter(|field| field.value > 0.0)
                .map_or(1.0, |field| field.value),
        },
        color,
        linetype,
        scale,
    })
}

fn read_entity(
    groups: &mut Groups<'_>,
    marker: Group<'_>,
    handles: &mut HashSet<u64>,
) -> Result<(Entity, EntityRecord), DxfError> {
    let codes: &[u16] = match marker.value {
        "LINE" => &[10, 20, 30, 11, 21, 31],
        "CIRCLE" => &[10, 20, 30, 40],
        "ARC" => &[10, 20, 30, 40, 50, 51],
        _ => unreachable!("validated entity type"),
    };
    let source_fields = record_fields(groups)?;
    let properties = read_entity_properties(&source_fields, marker.line, handles)?;
    let mut coordinates = vec![None; codes.len()];
    let mut normal = [None; 3];
    for group in source_fields {
        if COMMON_ENTITY_CODES.contains(&group.code) {
            continue;
        }
        if matches!(marker.value, "CIRCLE" | "ARC")
            && let Some(index) = [210, 220, 230].iter().position(|code| *code == group.code)
        {
            if normal[index].is_some() {
                return Err(DxfError {
                    line: group.line,
                    kind: DxfErrorKind::DuplicateField(group.code),
                });
            }
            normal[index] = Some(read_number(group)?);
            continue;
        }
        let index = codes
            .iter()
            .position(|code| *code == group.code)
            .ok_or(DxfError {
                line: group.line,
                kind: DxfErrorKind::UnsupportedProperty(group.code),
            })?;
        if coordinates[index].is_some() {
            return Err(DxfError {
                line: group.line,
                kind: DxfErrorKind::DuplicateField(group.code),
            });
        }
        let value = coordinate(group)?;
        if group.code == 40 && value.value < 0.0 {
            return Err(DxfError {
                line: group.line,
                kind: DxfErrorKind::InvalidRadius,
            });
        }
        coordinates[index] = Some(value);
    }
    let fields = codes
        .iter()
        .copied()
        .zip(coordinates)
        .map(|(code, field)| {
            field.map(|field| (code, field)).ok_or(DxfError {
                line: marker.line,
                kind: DxfErrorKind::MissingField(code),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    validate_xy_normal(normal, marker.line)?;
    let point = |offset: usize| {
        Point3::new(
            fields[offset].1.value,
            fields[offset + 1].1.value,
            fields[offset + 2].1.value,
        )
        .expect("parsed coordinates are finite")
    };
    let entity = match marker.value {
        "LINE" => Entity::Line(Line3::new(point(0), point(3))),
        "CIRCLE" => Entity::Circle(
            Circle3::new(point(0), fields[3].1.value).expect("parsed radius is finite"),
        ),
        "ARC" => Entity::Arc(
            Arc3::new(
                point(0),
                fields[3].1.value,
                fields[4].1.value.to_radians(),
                fields[5].1.value.to_radians(),
            )
            .expect("parsed radius and angles are finite and radius is nonnegative"),
        ),
        _ => unreachable!("validated entity type"),
    };
    let end = groups.peek()?.map_or(groups.offset, |group| group.start);
    Ok((
        entity.clone(),
        EntityRecord {
            original: entity,
            source_line: marker.line,
            start: marker.start,
            end,
            fields_end: end,
            fields,
            properties,
            polyline: None,
        },
    ))
}

fn validate_xy_normal(normal: [Option<f64>; 3], line: usize) -> Result<(), DxfError> {
    if normal.iter().any(Option::is_some) {
        for (index, code) in [210, 220, 230].into_iter().enumerate() {
            let value = normal[index].ok_or(DxfError {
                line,
                kind: DxfErrorKind::MissingField(code),
            })?;
            if value != [0.0, 0.0, 1.0][index] {
                return Err(DxfError {
                    line,
                    kind: DxfErrorKind::UnsupportedProperty(code),
                });
            }
        }
    }
    Ok(())
}

fn validate_polyline_fields(fields: &[Group<'_>], allowed: &[u16]) -> Result<(), DxfError> {
    for field in fields {
        if !COMMON_ENTITY_CODES.contains(&field.code) && !allowed.contains(&field.code) {
            return Err(DxfError {
                line: field.line,
                kind: DxfErrorKind::UnsupportedProperty(field.code),
            });
        }
        unique_field(fields, field.code)?;
    }
    Ok(())
}

fn required_coordinate(
    fields: &[Group<'_>],
    code: u16,
    line: usize,
) -> Result<Coordinate, DxfError> {
    coordinate(unique_field(fields, code)?.ok_or(DxfError {
        line,
        kind: DxfErrorKind::MissingField(code),
    })?)
}

fn require_zero(fields: &[Group<'_>], codes: &[u16]) -> Result<(), DxfError> {
    for code in codes {
        if let Some(field) = unique_field(fields, *code)?
            && read_number(field)? != 0.0
        {
            return Err(DxfError {
                line: field.line,
                kind: DxfErrorKind::UnsupportedProperty(*code),
            });
        }
    }
    Ok(())
}

fn read_polyline(
    groups: &mut Groups<'_>,
    marker: Group<'_>,
    handles: &mut HashSet<u64>,
) -> Result<(Entity, EntityRecord), DxfError> {
    let header = record_fields(groups)?;
    validate_polyline_fields(&header, &[10, 20, 30, 39, 40, 41, 66, 70, 210, 220, 230])?;
    let properties = read_entity_properties(&header, marker.line, handles)?;
    let mut fields = [10, 20, 30]
        .into_iter()
        .map(|code| Ok((code, required_coordinate(&header, code, marker.line)?)))
        .collect::<Result<Vec<_>, DxfError>>()?;
    require_zero(&header, &[10, 20, 39, 40, 41])?;
    let elevation = fields[2].1.value;
    let flags = unique_field(&header, 70)?.map(coordinate).transpose()?;
    if let Some(flags) = flags {
        if flags.value != 0.0 && flags.value != 1.0 {
            return Err(DxfError {
                line: marker.line,
                kind: DxfErrorKind::UnsupportedProperty(70),
            });
        }
        fields.push((70, flags));
    }
    if let Some(follows) = unique_field(&header, 66)? {
        let value = read_number(follows)?;
        if value != 0.0 && value != 1.0 {
            return Err(DxfError {
                line: follows.line,
                kind: DxfErrorKind::UnsupportedProperty(66),
            });
        }
    }
    let mut normal = [None; 3];
    for (index, code) in [210, 220, 230].into_iter().enumerate() {
        normal[index] = unique_field(&header, code)?.map(read_number).transpose()?;
    }
    validate_xy_normal(normal, marker.line)?;
    let fields_end = groups.peek()?.map_or(groups.offset, |group| group.start);
    let mut vertices = Vec::new();
    let mut bulges = Vec::new();
    let mut records = Vec::new();
    let seqend;
    loop {
        let child = groups.required()?;
        if child.code != 0 || !matches!(child.value, "VERTEX" | "SEQEND") {
            return Err(DxfError {
                line: child.line,
                kind: DxfErrorKind::Expected("VERTEX or SEQEND in POLYLINE"),
            });
        }
        let source_fields = record_fields(groups)?;
        let allowed: &[u16] = if child.value == "VERTEX" {
            &[10, 20, 30, 40, 41, 42, 70]
        } else {
            &[]
        };
        validate_polyline_fields(&source_fields, allowed)?;
        let properties = read_entity_properties(&source_fields, child.line, handles)?;
        let mut fields = Vec::new();
        if child.value == "VERTEX" {
            require_zero(&source_fields, &[40, 41, 70])?;
            for code in [10, 20] {
                fields.push((code, required_coordinate(&source_fields, code, child.line)?));
            }
            if let Some(z) = unique_field(&source_fields, 30)? {
                fields.push((30, coordinate(z)?));
            }
            let bulge = unique_field(&source_fields, 42)?
                .map(coordinate)
                .transpose()?;
            bulges.push(bulge.as_ref().map_or(0.0, |field| field.value));
            if let Some(bulge) = bulge {
                fields.push((42, bulge));
            }
            vertices.push(
                Point3::new(fields[0].1.value, fields[1].1.value, elevation)
                    .expect("finite parsed point"),
            );
        }
        let end = groups.peek()?.map_or(groups.offset, |group| group.start);
        let record = ChildRecord {
            start: child.start,
            end,
            fields,
            properties,
        };
        if child.value == "SEQEND" {
            seqend = record;
            break;
        }
        records.push(record);
    }
    let polyline = Polyline2::with_bulges(
        vertices,
        bulges,
        flags.is_some_and(|field| field.value == 1.0),
    )
    .map_err(|_| DxfError {
        line: marker.line,
        kind: DxfErrorKind::Expected("at least two POLYLINE vertices"),
    })?;
    let entity = Entity::Polyline(polyline);
    let end = seqend.end;
    Ok((
        entity.clone(),
        EntityRecord {
            original: entity,
            source_line: marker.line,
            start: marker.start,
            end,
            fields_end,
            fields,
            properties,
            polyline: Some(PolylineRecord {
                vertices: records,
                seqend,
            }),
        },
    ))
}

fn read_number(group: Group<'_>) -> Result<f64, DxfError> {
    group
        .value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or(DxfError {
            line: group.line,
            kind: DxfErrorKind::InvalidCoordinate(group.code),
        })
}

fn write_entity(output: &mut String, entity: &Entity, layer: &str, style: &EntityStyle) {
    write!(output, "0\n{}\n8\n{layer}\n", entity.name()).expect("String write");
    write_style(output, style);
    let codes: &[u16] = match entity {
        Entity::Line(_) => &[10, 20, 30, 11, 21, 31],
        Entity::Circle(_) => &[10, 20, 30, 40],
        Entity::Arc(_) => &[10, 20, 30, 40, 50, 51],
        Entity::Polyline(polyline) => {
            writeln!(
                output,
                "66\n1\n10\n0\n20\n0\n30\n{}\n70\n{}",
                polyline.elevation(),
                u8::from(polyline.is_closed())
            )
            .expect("String write");
            for (index, point) in polyline.vertices().iter().enumerate() {
                write_vertex(output, *point, polyline.bulges()[index], layer, style);
            }
            writeln!(output, "0\nSEQEND\n8\n{layer}").expect("String write");
            write_style(output, style);
            return;
        }
    };
    for code in codes {
        writeln!(output, "{code}\n{}", entity.coordinate(*code)).expect("String write");
    }
}

fn write_vertex(output: &mut String, point: Point3, bulge: f64, layer: &str, style: &EntityStyle) {
    writeln!(output, "0\nVERTEX\n8\n{layer}").expect("String write");
    write_style(output, style);
    writeln!(
        output,
        "10\n{}\n20\n{}\n30\n{}\n70\n0",
        point.x(),
        point.y(),
        point.z()
    )
    .expect("String write");
    if bulge != 0.0 {
        writeln!(output, "42\n{bulge}").expect("String write");
    }
}

fn patch_entity_properties(
    changes: &mut Vec<(usize, usize, String)>,
    source: &EntityProperties,
    layer: &str,
    style: &EntityStyle,
    changed: [bool; 4],
    end: usize,
) {
    if changed[0] {
        changes.push((source.layer.start, source.layer.end, layer.into()));
    }
    let mut fields = String::new();
    if changed[1] {
        patch_style_field(
            changes,
            &mut fields,
            62,
            source.color.as_ref().map(|field| (field.start, field.end)),
            style.color.to_string(),
        );
    }
    if changed[2] {
        patch_style_field(
            changes,
            &mut fields,
            6,
            source
                .linetype
                .as_ref()
                .map(|field| (field.start, field.end)),
            style.linetype.clone(),
        );
    }
    if changed[3] {
        patch_style_field(
            changes,
            &mut fields,
            48,
            source.scale.as_ref().map(|field| (field.start, field.end)),
            style.scale.to_string(),
        );
    }
    if !fields.is_empty() {
        changes.push((end, end, fields));
    }
}

fn write_style(output: &mut String, style: &EntityStyle) {
    if style.color != 256 {
        writeln!(output, "62\n{}", style.color).expect("String write");
    }
    if !style.linetype.eq_ignore_ascii_case("BYLAYER") {
        writeln!(output, "6\n{}", style.linetype).expect("String write");
    }
    if style.scale != 1.0 {
        writeln!(output, "48\n{}", style.scale).expect("String write");
    }
}

fn patch_style_field(
    changes: &mut Vec<(usize, usize, String)>,
    added: &mut String,
    code: u16,
    span: Option<(usize, usize)>,
    value: String,
) {
    if let Some((start, end)) = span {
        changes.push((start, end, value));
    } else {
        writeln!(added, "{code}\n{value}").expect("String write");
    }
}

/// Emit an ASCII R12 DXF with a minimal header and independent 3D LINE entities.
pub fn write_dxf(lines: &[Line3]) -> String {
    let mut output = String::from(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
    );
    for line in lines {
        let start = line.start();
        let end = line.end();
        write!(
            output,
            "0\nLINE\n8\n0\n10\n{}\n20\n{}\n30\n{}\n11\n{}\n21\n{}\n31\n{}\n",
            start.x(),
            start.y(),
            start.z(),
            end.x(),
            end.y(),
            end.z()
        )
        .expect("writing to a String cannot fail");
    }
    output.push_str("0\nENDSEC\n0\nEOF\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXTERNAL: &str = include_str!("../tests/fixtures/reordered-lines.dxf");
    const WRITTEN: &str = include_str!("../tests/fixtures/line3.dxf");
    const CIRCLES: &str = include_str!("../tests/fixtures/circles.dxf");
    const ARCS: &str = include_str!("../tests/fixtures/arcs.dxf");
    const LAYERS: &str = include_str!("../tests/fixtures/layers.dxf");

    #[test]
    fn layer_color_edits_change_only_color_values_and_preserve_layer_state() {
        let source = LAYERS.replace('\n', "\r\n");
        let mut document = DxfDocument::parse(&source).unwrap();
        let geometry = document.entities().to_vec();
        for index in [i16::MIN, -1, 0, 256, i16::MAX] {
            assert_eq!(
                document.set_layer_color("hidden", index),
                Err(LayerError::InvalidColorIndex(index))
            );
            assert_eq!(document.to_dxf(), source);
        }
        assert!(matches!(
            document.set_layer_color("missing", 1),
            Err(LayerError::UnknownLayer(_))
        ));
        assert!(!document.set_layer_color("walls", 1).unwrap());
        document.set_layer_color("hidden", 6).unwrap();
        document.set_layer_color("FROZEN", 12).unwrap();
        document.set_layer_color("LOCKED", 255).unwrap();
        document.set_layer_color("ZERO_COLOR", 1).unwrap();
        document.set_layer_color("BYLAYER_COLOR", 7).unwrap();
        let expected = source
            .replace("62\r\n-3\r\n", "62\r\n-6\r\n")
            .replace("62\r\n5\r\n", "62\r\n12\r\n")
            .replace("62\r\n2\r\n", "62\r\n255\r\n")
            .replace("62\r\n0\r\n", "62\r\n1\r\n")
            .replace("62\r\n256\r\n", "62\r\n7\r\n");
        assert_eq!(document.to_dxf(), expected);
        assert_eq!(document.entities(), geometry);
        assert!(document.layer("HIDDEN").unwrap().is_off());
        assert_eq!(document.layer("FROZEN").unwrap().flags(), 3);
        assert_eq!(document.layer("LOCKED").unwrap().flags(), 68);
        assert_eq!(
            DxfDocument::parse(&expected).unwrap().layers(),
            document.layers()
        );
        document.set_layer_visible("ZERO_COLOR", false).unwrap();
        assert_eq!(document.layer("ZERO_COLOR").unwrap().color_index(), -1);
    }

    #[test]
    fn layers_preserve_source_membership_flags_and_unedited_geometry() {
        let source = LAYERS.replace('\n', "\r\n");
        let mut document = DxfDocument::parse(&source).unwrap();
        assert_eq!(document.to_dxf(), source);
        assert_eq!(document.layers().len(), 7);
        assert_eq!(document.entity_layer(0), Some("walls"));
        assert_eq!(document.layer("Walls").unwrap().flags(), 64);
        assert!(document.layer("hidden").unwrap().is_off());
        assert!(document.layer("FROZEN").unwrap().is_frozen());
        assert!(document.layer("LOCKED").unwrap().is_locked());
        assert_eq!(document.layer("ZERO_COLOR").unwrap().color_index(), 0);
        assert_eq!(document.layer("BYLAYER_COLOR").unwrap().color_index(), 256);
        assert_eq!(
            document.set_layer_visible("ZERO_COLOR", false),
            Err(LayerError::CannotHideColorZero("ZERO_COLOR".into()))
        );
        assert_eq!(document.to_dxf(), source);
        assert!(!document.set_entity_layer(0, "WALLS").unwrap());
        assert_eq!(document.to_dxf(), source);
        document.set_entity_layer(0, "0").unwrap();
        document.set_layer_visible("hidden", true).unwrap();
        document.set_layer_visible("FROZEN", true).unwrap();
        document.set_layer_locked("LOCKED", false).unwrap();
        document.set_layer_visible("BYLAYER_COLOR", false).unwrap();
        let expected = source
            .replace("8\r\nwalls\r\n", "8\r\n0\r\n")
            .replace("62\r\n-3\r\n", "62\r\n3\r\n")
            .replace("70\r\n3\r\n62\r\n5\r\n", "70\r\n2\r\n62\r\n5\r\n")
            .replace("70\r\n68\r\n", "70\r\n64\r\n")
            .replace("62\r\n256\r\n", "62\r\n-256\r\n");
        assert_eq!(document.to_dxf(), expected);
        document.add_layer("Notes").unwrap();
        let output = document.to_dxf();
        let expected = expected
            .replace("2\r\nLAYER\r\n70\r\n7\r\n", "2\r\nLAYER\r\n70\r\n8\r\n")
            .replacen(
                "0\r\nENDTAB\r\n0\r\nENDSEC\r\n",
                "0\nLAYER\n2\nNotes\n70\n0\n62\n7\n6\nCONTINUOUS\n0\r\nENDTAB\r\n0\r\nENDSEC\r\n",
                1,
            );
        assert_eq!(output, expected);
        let reopened = DxfDocument::parse(&output).unwrap();
        assert_eq!(reopened.to_dxf(), output);
        assert_eq!(reopened.entities(), document.entities());
    }

    #[test]
    fn layer_tables_are_inserted_in_minimal_and_existing_table_sections() {
        let bare_blocks =
            "0\nSECTION\n2\nBLOCKS\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
        let with_tables = write_dxf(&[]).replacen(
            "0\nSECTION\n2\nENTITIES\n",
            "0\nSECTION\n2\nTABLES\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n",
            1,
        );
        for source in [write_dxf(&[]), with_tables, bare_blocks.into()] {
            let mut document = DxfDocument::parse(&source).unwrap();
            assert_eq!(document.to_dxf(), source);
            document.add_layer("Detail").unwrap();
            let line = Entity::Line(Line3::new(
                Point3::new(1.0, 2.0, 3.0).unwrap(),
                Point3::new(4.0, 5.0, 6.0).unwrap(),
            ));
            document.push_on_layer(line.clone(), "detail").unwrap();
            let output = document.to_dxf();
            assert_eq!(output.matches("2\nTABLES\n").count(), 1);
            assert!(output.contains("0\nTABLE\n2\nLAYER\n70\n2\n0\nLAYER\n2\n0\n"));
            assert!(
                output.contains("0\nLINE\n8\nDetail\n10\n1\n20\n2\n30\n3\n11\n4\n21\n5\n31\n6\n")
            );
            let reopened = DxfDocument::parse(&output).unwrap();
            assert_eq!(reopened.entity_layer(0), Some("Detail"));
            assert_eq!(reopened.entities(), [line]);
            assert_eq!(reopened.to_dxf(), output);
        }
    }

    #[test]
    fn malformed_layer_values_and_membership_are_rejected() {
        for (source, expected) in [
            (
                LAYERS.replace("8\nwalls\n", "8\nmissing\n"),
                DxfErrorKind::Layer(LayerError::UnknownLayer("missing".into())),
            ),
            (
                LAYERS.replace("2\nHIDDEN\n", "2\nwAlLs\n"),
                DxfErrorKind::Layer(LayerError::DuplicateName("wAlLs".into())),
            ),
            (
                LAYERS.replace("62\n-3\n", "62\n257\n"),
                DxfErrorKind::InvalidLayerValue(62),
            ),
            (
                LAYERS.replace("70\n68\n", "70\n1.5\n"),
                DxfErrorKind::InvalidLayerValue(70),
            ),
            (
                LAYERS.replace("70\n68\n", "70\n68\n70\n4\n"),
                DxfErrorKind::DuplicateField(70),
            ),
            (
                LAYERS.replace("2\nHIDDEN\n70\n0\n", "2\nHIDDEN\n"),
                DxfErrorKind::MissingField(70),
            ),
        ] {
            assert_eq!(DxfDocument::parse(&source).unwrap_err().kind, expected);
        }
        let mut document = DxfDocument::parse(LAYERS).unwrap();
        for name in [
            "",
            "WITH SPACE",
            "bad|name",
            "bad\nname",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            assert_eq!(document.add_layer(name), Err(LayerError::InvalidName));
            assert_eq!(document.to_dxf(), LAYERS);
        }
        assert!(matches!(
            document.add_layer("walls"),
            Err(LayerError::DuplicateName(_))
        ));
        assert_eq!(document.to_dxf(), LAYERS);
    }

    #[test]
    fn arcs_use_dxf_degrees_and_preserve_untouched_field_spelling_when_edited() {
        let source = ARCS.replace('\n', "\r\n");
        let mut document = DxfDocument::parse(&source).unwrap();
        assert_eq!(document.to_dxf(), source);
        let Entity::Arc(mut arc) = document.entities()[0] else {
            panic!("ARC expected")
        };
        assert_eq!(arc.center(), Point3::new(1.0, 2.0, 3.0).unwrap());
        assert!((arc.sweep() - std::f64::consts::PI).abs() < 1e-12);
        arc.set_center(Point3::new(4.0, 6.0, 9.0).unwrap());
        document.replace_entity(0, Entity::Arc(arc)).unwrap();
        let moved = source
            .replacen("10\r\n1e0\r\n", "10\r\n4\r\n", 1)
            .replacen("20\r\n2\r\n", "20\r\n6\r\n", 1)
            .replacen("30\r\n3\r\n", "30\r\n9\r\n", 1);
        assert_eq!(document.to_dxf(), moved);
        arc.set_angles(std::f64::consts::FRAC_PI_2, std::f64::consts::PI)
            .unwrap();
        document.replace_entity(0, Entity::Arc(arc)).unwrap();
        let rotated = moved
            .replacen("50\r\n270.0\r\n", "50\r\n90\r\n", 1)
            .replacen("51\r\n90.00\r\n", "51\r\n180\r\n", 1);
        assert_eq!(document.to_dxf(), rotated);
        assert_eq!(
            DxfDocument::parse(&rotated).unwrap().entities(),
            document.entities()
        );
        document.push(Entity::Arc(
            Arc3::new(
                Point3::new(0.0, 0.0, 7.0).unwrap(),
                2.0,
                0.0,
                std::f64::consts::PI,
            )
            .unwrap(),
        ));
        assert!(
            document
                .to_dxf()
                .contains("0\nARC\n8\n0\n10\n0\n20\n0\n30\n7\n40\n2\n50\n0\n51\n180\n")
        );
    }

    #[test]
    fn circles_preserve_ocs_center_radius_and_default_xy_plane_during_editing() {
        let mut document = DxfDocument::parse(CIRCLES).unwrap();
        assert_eq!(document.to_dxf(), CIRCLES);
        let circle = Circle3::new(Point3::new(1.0, 2.0, 3.0).unwrap(), 5.0).unwrap();
        assert_eq!(document.entities()[0], Entity::Circle(circle));
        let edited = Circle3::new(Point3::new(4.0, 6.0, 9.0).unwrap(), 7.0).unwrap();
        document.replace_entity(0, Entity::Circle(edited)).unwrap();
        let expected = CIRCLES
            .replacen("40\n5\n", "40\n7\n", 1)
            .replacen("30\n3\n", "30\n9\n", 1)
            .replacen("20\n2\n", "20\n6\n", 1)
            .replacen("10\n1\n", "10\n4\n", 1);
        assert_eq!(document.to_dxf(), expected);
        assert_eq!(
            DxfDocument::parse(&expected).unwrap().entities(),
            document.entities()
        );
    }

    #[test]
    fn read_reordered_external_fixture_preserves_3d_and_coincident_endpoints() {
        let lines = read_dxf(&EXTERNAL.replace('\n', "\r\n")).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].start(), Point3::new(1.0, 2.0, 3.0).unwrap());
        assert_eq!(lines[0].end(), Point3::new(4.0, 6.0, 15.0).unwrap());
        let coincident = Point3::new(-7.0, 8.0, 0.5).unwrap();
        assert_eq!((lines[1].start(), lines[1].end()), (coincident, coincident));
    }

    #[test]
    fn writer_matches_an_independent_public_format_fixture() {
        let line = Line3::new(
            Point3::new(1.0, 2.0, 3.0).unwrap(),
            Point3::new(4.0, 6.0, 15.0).unwrap(),
        );
        assert_eq!(write_dxf(&[line]), WRITTEN);
        assert_eq!(read_dxf(WRITTEN).unwrap(), vec![line]);
    }

    #[test]
    fn document_edits_reordered_crlf_fields_without_rewriting_other_values() {
        let source = format!("999\nDrawing note: π\n{EXTERNAL}").replace('\n', "\r\n");
        let mut document = DxfDocument::parse(&source).unwrap();
        let mut line = document.lines().next().unwrap();
        line.set_end(Point3::new(40.0, 60.0, 150.0).unwrap());
        document.replace_line(0, line).unwrap();
        let expected = source
            .replacen("11\r\n4\r\n", "11\r\n40\r\n", 1)
            .replacen("21\r\n6\r\n", "21\r\n60\r\n", 1)
            .replacen("31\r\n15\r\n", "31\r\n150\r\n", 1);
        assert_ne!(expected, source);
        assert_eq!(document.to_dxf(), expected);
        assert_eq!(read_dxf(&expected).unwrap()[0], line);
    }

    #[test]
    fn malformed_and_unsupported_records_return_typed_errors() {
        let cases = [
            (ARCS.replacen("50\n270.0\n", "", 1), DxfErrorKind::MissingField(50)),
            (ARCS.replacen("51\n90.00\n", "", 1), DxfErrorKind::MissingField(51)),
            (ARCS.replacen("50\n270.0\n", "50\nNaN\n", 1), DxfErrorKind::InvalidCoordinate(50)),
            (ARCS.replacen("51\n90.00\n", "51\n90\n51\n180\n", 1), DxfErrorKind::DuplicateField(51)),
            (ARCS.replacen("40\n5.000\n", "40\n-1\n", 1), DxfErrorKind::InvalidRadius),
            (ARCS.replacen("230\n1\n", "230\n-1\n", 1), DxfErrorKind::UnsupportedProperty(230)),
            (CIRCLES.replace("40\n5\n", "40\n-5\n"), DxfErrorKind::InvalidRadius),
            (CIRCLES.replace("40\n5\n", ""), DxfErrorKind::MissingField(40)),
            (CIRCLES.replace("230\n1\n", ""), DxfErrorKind::MissingField(230)),
            (CIRCLES.replace("230\n1\n", "230\n-1\n"), DxfErrorKind::UnsupportedProperty(230)),
            (
                WRITTEN.replace("10\n1\n", "10\nNaN\n"),
                DxfErrorKind::InvalidCoordinate(10),
            ),
            (
                WRITTEN.replace("11\n4\n", ""),
                DxfErrorKind::MissingField(11),
            ),
            (
                WRITTEN.replace("31\n15\n", ""),
                DxfErrorKind::MissingField(31),
            ),
            (
                WRITTEN.replace("10\n1\n", "10\n1\n10\n2\n"),
                DxfErrorKind::DuplicateField(10),
            ),
            (
                WRITTEN.replace("8\n0\n", "8\nother\n"),
                DxfErrorKind::Layer(LayerError::UnknownLayer("other".into())),
            ),
            (
                WRITTEN.replace("LINE", "SPLINE"),
                DxfErrorKind::UnsupportedEntity("SPLINE".into()),
            ),
            (
                WRITTEN.replace("8\n0\n", "8\n0\n39\n1\n"),
                DxfErrorKind::UnsupportedProperty(39),
            ),
            (
                WRITTEN.replace("AC1009", "AC1032"),
                DxfErrorKind::UnsupportedVersion("AC1032".into()),
            ),
            (
                WRITTEN.replace("10\n1\n", "bad\n1\n"),
                DxfErrorKind::InvalidGroupCode,
            ),
            (
                WRITTEN.trim_end().trim_end_matches("EOF").to_owned(),
                DxfErrorKind::UnexpectedEnd,
            ),
            (
                format!("{WRITTEN}0\nLINE\n"),
                DxfErrorKind::Expected("end of file after EOF"),
            ),
            (
                WRITTEN.replace("8\n0\n", "5\nnot-hex\n8\n0\n"),
                DxfErrorKind::InvalidHandle(5),
            ),
            (
                WRITTEN.replace("8\n0\n", "5\nab\n5\ncd\n8\n0\n"),
                DxfErrorKind::DuplicateField(5),
            ),
            (
                EXTERNAL.replace("0\nLINE\n", "0\nLINE\n5\nab\n"),
                DxfErrorKind::DuplicateHandle("ab".into()),
            ),
            (
                WRITTEN.replace("0\nEOF\n", "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n"),
                DxfErrorKind::DuplicateSection("ENTITIES".into()),
            ),
            (
                WRITTEN.replace("0\nSECTION\n2\nENTITIES\n", "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"),
                DxfErrorKind::Expected("ENDTAB"),
            ),
        ];
        for (input, expected) in cases {
            let error = read_dxf(&input).unwrap_err();
            assert_eq!(error.kind, expected, "{input}");
            assert!(error.line > 0);
        }
    }
}
