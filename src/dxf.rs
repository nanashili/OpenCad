//! ASCII R12 geometry exchange with preservation of existing document records.
//! Geometry uses finite coordinates and named layers. Other table and block
//! contents are retained without interpreting their contents.

use crate::group::EntityGroup;
use crate::{Anchor, ConstraintError, ConstraintKind, ConstraintRecord, ConstraintSet, LinePair};
use crate::{
    Arc3, BlockDefinition, Circle3, CornerParameter, Dimension, DimensionKind, DimensionStyle,
    Entity, EntityStyle, EntityStyleError, Face3, GeometryError, Hatch, HatchPattern, Insert,
    Layer, LayerError, Leader, Line3, Linetype, LinetypeError, MLeader, MLeaderBranch,
    MLeaderContent, MLeaderStyle, MLine2, MLineElement, MLineJustification, MLineStyle, MText,
    Mesh3, Point3, Polyline2, Polyline3, PolylineWidth, RasterImage, Ray3, Spline3, Table, Text,
    TextStyle, XLine3,
};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Write};
use std::hash::{Hash, Hasher};
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
    Corner(GeometryError),
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
            DxfErrorKind::Corner(error) => error.fmt(f),
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

#[derive(Clone, Debug, Default)]
struct DimensionData {
    values: [f64; 4],
    fields: [Option<Coordinate>; 4],
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
    dimension_data: DimensionData,
    block_definitions: Vec<BlockDefinition>,
    imported_block_count: usize,
    block_replacements: Vec<BlockDefinition>,
    layouts: crate::LayoutCollection,
    groups: Vec<EntityGroup>,
    reordered: bool,
    constraints: ConstraintSet,
    mline_styles: Vec<MLineStyle>,
    text_styles: Vec<TextStyle>,
    dimension_styles: Vec<DimensionStyle>,
    mleader_styles: Vec<MLeaderStyle>,
    mleader_style_handles: HashMap<u64, String>,
    current_text_style: String,
    current_dimension_style: String,
    current_mleader_style: String,
}

impl DxfDocument {
    /// Validate supported sections, entity fields, layer membership and handles.
    pub fn parse(input: &str) -> Result<Self, DxfError> {
        let mut groups = Groups::new(input);
        let mline_styles = read_mline_styles(input);
        let text_styles = read_text_styles(input);
        let dimension_styles = read_dimension_styles(input);
        let (mleader_styles, mleader_style_handles) = read_mleader_styles(input);
        let current_styles = read_current_style_names(input);
        let block_definitions = read_block_definitions(input)?;
        let mut seen = [false; 5];
        let mut previous_section = 0;
        let mut handles = HashSet::new();
        let clip_handles = viewport_clip_handles(input);
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
                let dimension_data = read_dimensions(input)?;
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
                let image_paths = read_image_def_paths(input);
                for entity in &mut entities {
                    if let Entity::Image(image) = entity {
                        let key = image.path().to_string_lossy().to_string();
                        if let Some(path) = image_paths.get(&key) {
                            image.set_path(path.clone());
                        }
                    }
                }
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
                    dimension_data,
                    imported_block_count: block_definitions.len(),
                    block_definitions,
                    block_replacements: Vec::new(),
                    layouts: read_layouts(input),
                    groups: read_groups(input),
                    reordered: false,
                    constraints: read_constraints(input, &entity_handles(input)),
                    mline_styles,
                    text_styles,
                    dimension_styles,
                    mleader_styles,
                    mleader_style_handles,
                    current_text_style: current_styles.0,
                    current_dimension_style: current_styles.1,
                    current_mleader_style: current_styles.2,
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
                "OBJECTS" => 4,
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
                "TABLES" | "BLOCKS" | "OBJECTS" => {
                    read_preserved_section(&mut groups, section.value, &mut handles)?
                }
                _ => loop {
                    let group = groups.required()?;
                    match (group.code, group.value) {
                        (0, "ENDSEC") => {
                            insertion = group.start;
                            break;
                        }
                        (
                            0,
                            "POINT" | "LINE" | "XLINE" | "RAY" | "CIRCLE" | "ARC" | "ELLIPSE"
                            | "3DFACE",
                        ) => {
                            let (entity, record) = read_entity(&mut groups, group, &mut handles)?;
                            entities.push(entity);
                            records.push(Some(record));
                        }
                        (
                            0,
                            "SPLINE" | "TEXT" | "MTEXT" | "LEADER" | "MULTILEADER" | "TABLE"
                            | "ACAD_TABLE" | "DIMENSION" | "HATCH" | "IMAGE" | "MLINE" | "MESH",
                        ) => {
                            let (entity, record) = read_extended_entity(
                                &mut groups,
                                group,
                                &mut handles,
                                &mline_styles,
                                &mleader_styles,
                                &mleader_style_handles,
                                &dimension_styles,
                                &block_definitions,
                            )?;
                            entities.push(entity);
                            records.push(Some(record));
                        }
                        (0, "INSERT") => {
                            let (entity, record) =
                                read_insert_entity(&mut groups, group, &mut handles)?;
                            entities.push(entity);
                            records.push(Some(record));
                        }
                        (0, "VIEWPORT") => {
                            // Viewports are represented by the typed layout model.
                            // Leave their source bytes untouched and import them below.
                            let _ = record_fields(&mut groups)?;
                        }
                        (0, "LWPOLYLINE") => {
                            let fields = record_fields(&mut groups)?;
                            let is_clip = fields
                                .iter()
                                .find(|field| field.code == 5)
                                .is_some_and(|field| clip_handles.contains(field.value));
                            if is_clip {
                                if let Some(handle) = fields.iter().find(|field| field.code == 5) {
                                    read_handle(*handle, &mut handles)?;
                                }
                                continue;
                            }
                            let (entity, record) = read_lwpolyline_fields(
                                fields,
                                group,
                                groups.peek()?.map_or(groups.offset, |next| next.start),
                                &mut handles,
                            )?;
                            entities.push(entity);
                            records.push(Some(record));
                        }
                        (0, "POLYLINE") => {
                            let (entity, record) = read_polyline(&mut groups, group, &mut handles)?;
                            if !record_handle_at(input, record.start)
                                .is_some_and(|handle| clip_handles.contains(&handle))
                            {
                                entities.push(entity);
                                records.push(Some(record));
                            }
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
                                    "supported entity or ENDSEC in group 0",
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
    pub fn groups(&self) -> &[EntityGroup] {
        &self.groups
    }
    pub fn constraints(&self) -> &[ConstraintRecord] {
        self.constraints.records()
    }
    pub fn constraint_set(&self) -> &ConstraintSet {
        &self.constraints
    }
    pub fn set_constraints(&mut self, constraints: ConstraintSet) {
        self.constraints = constraints;
    }
    pub fn add_constraint(
        &mut self,
        kind: ConstraintKind,
        value: Option<f64>,
    ) -> Result<u64, ConstraintError> {
        self.constraints.add(kind, value)
    }
    pub fn add_fixed_constraint(
        &mut self,
        anchor: Anchor,
        point: Point3,
    ) -> Result<u64, ConstraintError> {
        self.constraints.add_fixed(anchor, point)
    }
    pub fn solve_constraints(&mut self) -> Result<bool, ConstraintError> {
        let mut candidate = self.entities.clone();
        let changed = self.constraints.solve_entities(&mut candidate)?;
        if changed {
            self.entities = candidate;
        }
        Ok(changed)
    }
    pub fn set_constraint_enabled(
        &mut self,
        id: u64,
        enabled: bool,
    ) -> Result<bool, ConstraintError> {
        self.constraints.set_enabled(id, enabled)
    }
    pub fn set_constraint_value(&mut self, id: u64, value: f64) -> Result<bool, ConstraintError> {
        self.constraints.set_value(id, value)
    }
    pub fn remove_constraint(&mut self, id: u64) -> Result<ConstraintRecord, ConstraintError> {
        self.constraints.remove(id)
    }
    pub fn mline_styles(&self) -> &[MLineStyle] {
        &self.mline_styles
    }
    pub fn set_mline_styles(&mut self, styles: Vec<MLineStyle>) {
        self.mline_styles = styles;
    }
    pub fn mline_style(&self, name: &str) -> Option<&MLineStyle> {
        self.mline_styles
            .iter()
            .find(|style| style.name().eq_ignore_ascii_case(name))
    }
    pub fn current_mline_style(&self) -> Option<&MLineStyle> {
        self.mline_styles.first()
    }
    pub fn text_styles(&self) -> &[TextStyle] {
        &self.text_styles
    }
    pub fn set_text_styles(&mut self, styles: Vec<TextStyle>) -> Result<(), DxfError> {
        validate_style_names(styles.iter().map(TextStyle::name))?;
        if !styles
            .iter()
            .any(|style| style.name().eq_ignore_ascii_case(&self.current_text_style))
        {
            return Err(group_error(2));
        }
        self.text_styles = styles;
        Ok(())
    }
    pub fn text_style(&self, name: &str) -> Option<&TextStyle> {
        self.text_styles
            .iter()
            .find(|style| style.name().eq_ignore_ascii_case(name))
    }
    pub fn current_text_style(&self) -> &str {
        &self.current_text_style
    }
    pub fn set_current_text_style(&mut self, name: impl Into<String>) -> Result<(), DxfError> {
        let name = name.into();
        if self.text_style(&name).is_none() {
            return Err(group_error(2));
        }
        self.current_text_style = name;
        Ok(())
    }
    pub fn dimension_styles(&self) -> &[DimensionStyle] {
        &self.dimension_styles
    }
    pub fn set_dimension_styles(&mut self, styles: Vec<DimensionStyle>) -> Result<(), DxfError> {
        validate_style_names(styles.iter().map(DimensionStyle::name))?;
        if !styles.iter().any(|style| {
            style
                .name()
                .eq_ignore_ascii_case(&self.current_dimension_style)
        }) {
            return Err(group_error(2));
        }
        self.dimension_styles = styles;
        Ok(())
    }
    pub fn dimension_style(&self, name: &str) -> Option<&DimensionStyle> {
        self.dimension_styles
            .iter()
            .find(|style| style.name().eq_ignore_ascii_case(name))
    }
    pub fn current_dimension_style(&self) -> &str {
        &self.current_dimension_style
    }
    pub fn set_current_dimension_style(&mut self, name: impl Into<String>) -> Result<(), DxfError> {
        let name = name.into();
        if self.dimension_style(&name).is_none() {
            return Err(group_error(2));
        }
        self.current_dimension_style = name;
        Ok(())
    }
    pub fn mleader_styles(&self) -> &[MLeaderStyle] {
        &self.mleader_styles
    }
    pub fn set_mleader_styles(&mut self, styles: Vec<MLeaderStyle>) -> Result<(), DxfError> {
        validate_style_names(styles.iter().map(MLeaderStyle::name))?;
        if !styles.iter().any(|style| {
            style
                .name()
                .eq_ignore_ascii_case(&self.current_mleader_style)
        }) {
            return Err(group_error(2));
        }
        self.mleader_styles = styles;
        self.mleader_style_handles.clear();
        Ok(())
    }
    pub fn mleader_style(&self, name: &str) -> Option<&MLeaderStyle> {
        self.mleader_styles
            .iter()
            .find(|style| style.name().eq_ignore_ascii_case(name))
    }
    pub fn current_mleader_style(&self) -> &str {
        &self.current_mleader_style
    }
    pub fn set_current_mleader_style(&mut self, name: impl Into<String>) -> Result<(), DxfError> {
        let name = name.into();
        if self.mleader_style(&name).is_none() {
            return Err(group_error(2));
        }
        self.current_mleader_style = name;
        Ok(())
    }
    pub fn create_group(
        &mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        selectable: bool,
        members: &[usize],
    ) -> Result<usize, DxfError> {
        let name = name.into();
        let mut members = members.to_vec();
        members.sort_unstable();
        members.dedup();
        if members.iter().any(|index| *index >= self.entities.len()) {
            return Err(group_error(0));
        }
        if self.groups.iter().any(|group| group.name() == name) {
            return Err(group_error(2));
        }
        let group =
            EntityGroup::new(name, description, selectable, members).map_err(|_| group_error(2))?;
        self.groups.push(group);
        Ok(self.groups.len() - 1)
    }
    pub fn rename_group(&mut self, index: usize, name: impl Into<String>) -> Result<(), DxfError> {
        let name = name.into();
        if name.trim().is_empty()
            || self
                .groups
                .iter()
                .enumerate()
                .any(|(i, group)| i != index && group.name() == name)
        {
            return Err(group_error(2));
        }
        let group = self.groups.get_mut(index).ok_or_else(|| group_error(2))?;
        group.set_name(name);
        Ok(())
    }
    pub fn remove_group(&mut self, index: usize) -> Result<EntityGroup, DxfError> {
        (index < self.groups.len())
            .then(|| self.groups.remove(index))
            .ok_or_else(|| group_error(2))
    }
    pub fn set_group_membership(
        &mut self,
        index: usize,
        members: &[usize],
    ) -> Result<(), DxfError> {
        let mut members = members.to_vec();
        members.sort_unstable();
        members.dedup();
        if members.iter().any(|member| *member >= self.entities.len()) {
            return Err(group_error(340));
        }
        self.groups
            .get_mut(index)
            .ok_or_else(|| group_error(340))?
            .set_members(members);
        Ok(())
    }
    pub fn reorder_entities(&mut self, order: &[usize]) -> Result<bool, DxfError> {
        if order.len() != self.entities.len() {
            return Err(group_error(0));
        }
        let mut seen = vec![false; order.len()];
        if order
            .iter()
            .any(|index| *index >= order.len() || std::mem::replace(&mut seen[*index], true))
        {
            return Err(group_error(0));
        }
        if order.iter().enumerate().all(|(new, old)| new == *old) {
            return Ok(false);
        }
        let old_groups = self.groups.clone();
        let entities = order
            .iter()
            .map(|index| self.entities[*index].clone())
            .collect();
        let records = order
            .iter()
            .map(|index| self.records[*index].clone())
            .collect();
        let layers = order
            .iter()
            .map(|index| self.entity_layers[*index].clone())
            .collect();
        let styles = order
            .iter()
            .map(|index| self.entity_styles[*index].clone())
            .collect();
        let mut inverse = vec![0; order.len()];
        for (new, old) in order.iter().copied().enumerate() {
            inverse[old] = new;
        }
        self.entities = entities;
        self.records = records;
        self.entity_layers = layers;
        self.entity_styles = styles;
        self.groups = old_groups
            .into_iter()
            .map(|mut group| {
                group.set_members(
                    group
                        .members()
                        .iter()
                        .map(|member| inverse[*member])
                        .collect(),
                );
                group
            })
            .collect();
        self.constraints = remap_constraints(
            &self.constraints,
            &inverse.iter().map(|i| Some(*i)).collect::<Vec<_>>(),
        );
        self.reordered = true;
        Ok(true)
    }
    pub fn layouts(&self) -> &crate::LayoutCollection {
        &self.layouts
    }
    pub fn layouts_mut(&mut self) -> &mut crate::LayoutCollection {
        &mut self.layouts
    }
    pub fn set_layouts(&mut self, layouts: crate::LayoutCollection) {
        self.layouts = layouts;
    }

    /// Definitions registered for INSERT expansion. Existing opaque BLOCKS records remain
    /// source-preserved; definitions added through this API are emitted as standard BLOCK/ENDBLK
    /// records when the document is written.
    pub fn block_definitions(&self) -> &[BlockDefinition] {
        &self.block_definitions
    }
    pub fn block(&self, name: &str) -> Option<&BlockDefinition> {
        self.block_definitions
            .iter()
            .find(|block| block.name().eq_ignore_ascii_case(name))
    }
    pub fn add_block_definition(&mut self, block: BlockDefinition) -> Result<(), DxfError> {
        if block.name().is_empty() || self.block(block.name()).is_some() {
            return Err(DxfError {
                line: 0,
                kind: DxfErrorKind::UnsupportedProperty(2),
            });
        }
        self.block_definitions.push(block);
        Ok(())
    }

    /// Replace a named block definition while retaining its registry position.
    /// Writers can use the returned registry to emit the changed definition in
    /// place of an imported BLOCK record.
    pub fn replace_block_definition(&mut self, block: BlockDefinition) -> Result<(), DxfError> {
        let Some(existing) = self
            .block_definitions
            .iter_mut()
            .find(|item| item.name().eq_ignore_ascii_case(block.name()))
        else {
            return Err(DxfError {
                line: 0,
                kind: DxfErrorKind::UnsupportedProperty(2),
            });
        };
        *existing = block.clone();
        if let Some(replacement) = self
            .block_replacements
            .iter_mut()
            .find(|item| item.name().eq_ignore_ascii_case(block.name()))
        {
            *replacement = block;
        } else if self
            .block_definitions
            .iter()
            .position(|item| item.name().eq_ignore_ascii_case(block.name()))
            .is_some_and(|index| index < self.imported_block_count)
        {
            self.block_replacements.push(block);
        }
        Ok(())
    }
    pub fn remove_block_definition(&mut self, name: &str) -> Option<BlockDefinition> {
        let index = self
            .block_definitions
            .iter()
            .position(|block| block.name().eq_ignore_ascii_case(name))?;
        Some(self.block_definitions.remove(index))
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

    pub fn corner_value(&self, parameter: CornerParameter) -> f64 {
        self.dimension_data.values[parameter.index()]
    }

    pub fn pline_width(&self) -> f64 {
        self.dimension_data.values[3]
    }

    pub fn set_pline_width(&mut self, value: f64) -> Result<bool, GeometryError> {
        PolylineWidth::new(value, value)?;
        let current = &mut self.dimension_data.values[3];
        if *current == value {
            return Ok(false);
        }
        *current = value;
        Ok(true)
    }

    pub fn set_corner_value(
        &mut self,
        parameter: CornerParameter,
        value: f64,
    ) -> Result<bool, GeometryError> {
        let value = crate::corner::dimension(value)?;
        let current = &mut self.dimension_data.values[parameter.index()];
        if *current == value {
            return Ok(false);
        }
        *current = value;
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
    pub fn set_layer_frozen(&mut self, name: &str, frozen: bool) -> Result<bool, LayerError> {
        let layer = self
            .layer_data
            .layers
            .iter_mut()
            .find(|layer| layer.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?;
        let old = layer.flags;
        if frozen {
            layer.flags |= 1;
        } else {
            layer.flags &= !1;
        }
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
            _ => None,
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
            _ => {
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
        let removed = self.entities.remove(index);
        for group in &mut self.groups {
            group.set_members(
                group
                    .members()
                    .iter()
                    .filter_map(|member| {
                        if *member == index {
                            None
                        } else if *member > index {
                            Some(*member - 1)
                        } else {
                            Some(*member)
                        }
                    })
                    .collect(),
            );
        }
        let mut mapping = vec![None; self.entities.len() + 1];
        for (old, mapped) in mapping.iter_mut().enumerate() {
            *mapped = if old == index {
                None
            } else if old > index {
                Some(old - 1)
            } else {
                Some(old)
            };
        }
        self.constraints = remap_constraints(&self.constraints, &mapping);
        Some(removed)
    }

    /// Preserve original bytes except explicitly edited geometry/layer/style values.
    pub fn to_dxf(&self) -> String {
        let mut changes: Vec<_> = self
            .removed
            .iter()
            .map(|&(start, end)| (start, end, String::new()))
            .collect();
        let mut added = String::new();
        let mut object_handle = next_handle(&self.source);
        let mut generated_handles = HashMap::new();
        for (entity_index, (((entity, record), layer), style)) in self
            .entities
            .iter()
            .zip(&self.records)
            .zip(&self.entity_layers)
            .zip(&self.entity_styles)
            .enumerate()
        {
            let Some(record) = record else {
                let mut serialized = String::new();
                write_entity(&mut serialized, entity, layer, style);
                let handle = format!("{:X}", object_handle);
                insert_entity_handle(&mut serialized, &handle);
                added.push_str(&serialized);
                generated_handles.insert(entity_index, handle);
                object_handle = object_handle.saturating_add(1);
                continue;
            };
            if entity != &record.original
                && matches!(
                    entity,
                    Entity::Face3(_)
                        | Entity::Text(_)
                        | Entity::MText(_)
                        | Entity::Leader(_)
                        | Entity::MLeader(_)
                        | Entity::Table(_)
                        | Entity::Dimension(_)
                        | Entity::Hatch(_)
                        | Entity::Insert(_)
                        | Entity::Spline(_)
                        | Entity::XLine(_)
                        | Entity::Ray(_)
                        | Entity::Image(_)
                        | Entity::Polyline3(_)
                        | Entity::Polyline(_)
                )
            {
                let mut replacement = String::new();
                write_entity(&mut replacement, entity, layer, style);
                if let Some(handle) = record_handle_at(&self.source, record.start) {
                    insert_entity_handle(&mut replacement, &handle);
                }
                changes.push((record.start, record.end, replacement));
                continue;
            }
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
                let default_width = |code| {
                    record
                        .fields
                        .iter()
                        .find(|(field_code, _)| *field_code == code)
                        .map_or(0.0, |(_, field)| field.value)
                };
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
                            40 => polyline.widths()[index].start(),
                            41 => polyline.widths()[index].end(),
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
                    for (code, value) in [
                        (40, polyline.widths()[index].start()),
                        (41, polyline.widths()[index].end()),
                    ] {
                        if value != default_width(code)
                            && !vertex
                                .fields
                                .iter()
                                .any(|(field_code, _)| *field_code == code)
                        {
                            changes.push((vertex.end, vertex.end, format!("{code}\n{value}\n")));
                        }
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
                        polyline.widths()[index],
                        [default_width(40), default_width(41)],
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
                // Parent defaults remain source-backed. Effective widths are
                // patched on vertices, including explicit zero overrides.
                if matches!(entity, Entity::Polyline(_)) && matches!(code, 40 | 41) {
                    continue;
                }
                let value = entity.coordinate(*code);
                if value.to_bits() != record.original.coordinate(*code).to_bits() {
                    changes.push((field.start, field.end, value.to_string()));
                }
            }
        }
        if self.reordered {
            let (section_marker, newline) = if self
                .source
                .contains("\r\n0\r\nSECTION\r\n2\r\nENTITIES\r\n")
            {
                ("0\r\nSECTION\r\n2\r\nENTITIES\r\n", "\r\n")
            } else {
                ("0\nSECTION\n2\nENTITIES\n", "\n")
            };
            if let Some(section_start) = self.source.find(section_marker) {
                let entity_start = section_start + section_marker.len();
                let mut serialized = String::new();
                for (index, (((entity, record), layer), style)) in self
                    .entities
                    .iter()
                    .zip(&self.records)
                    .zip(&self.entity_layers)
                    .zip(&self.entity_styles)
                    .enumerate()
                {
                    let mut entity_text = String::new();
                    write_entity(&mut entity_text, entity, layer, style);
                    let handle = generated_handles.get(&index).cloned().or_else(|| {
                        record
                            .as_ref()
                            .and_then(|record| record_handle_at(&self.source, record.start))
                    });
                    if let Some(handle) = handle {
                        insert_entity_handle(&mut entity_text, &handle);
                    }
                    serialized.push_str(&entity_text.replace('\n', newline));
                }
                changes.retain(|(start, end, _)| *end <= entity_start || *start >= self.insertion);
                changes.push((entity_start, self.insertion, serialized));
                added.clear();
            }
        }
        let mut tables = String::new();
        let mut header = String::new();
        self.linetype_data.write_changes(
            &mut changes,
            &mut tables,
            &mut header,
            self.layer_data.table.as_ref(),
        );
        self.dimension_data.write_changes(&mut changes, &mut header);
        append_header_style(
            &mut header,
            "$TEXTSTYLE",
            &self.current_text_style,
            &self.source,
        );
        append_header_style(
            &mut header,
            "$DIMSTYLE",
            &self.current_dimension_style,
            &self.source,
        );
        append_header_style(
            &mut header,
            "$CMLEADERSTYLE",
            &self.current_mleader_style,
            &self.source,
        );
        if !header.is_empty() {
            if let Some(at) = self.linetype_data.header_end {
                changes.push((at, at, header));
            } else {
                changes.push((
                    0,
                    0,
                    format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n{header}0\nENDSEC\n"),
                ));
            }
        }
        self.layer_data.write_changes(&mut changes, &mut tables);
        let mline_style_objects = serialize_mline_styles(&self.mline_styles, "\n");
        let mline_style_records = serialize_mline_style_records(&self.mline_styles, "\n");
        let mline_style_spans = mline_style_spans(&self.source);
        if mline_style_spans.is_empty() {
            tables.push_str(&mline_style_objects);
        } else {
            for (index, (start, end)) in mline_style_spans.iter().copied().enumerate() {
                changes.push((
                    start,
                    end,
                    if index == 0 {
                        mline_style_records.clone()
                    } else {
                        String::new()
                    },
                ));
            }
        }
        tables.push_str(&serialize_annotation_tables(
            &self.text_styles,
            &self.dimension_styles,
            "\n",
        ));
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
        if !self.block_definitions.is_empty() {
            let has_blocks = self.source.contains("\n2\nBLOCKS\n");
            let mut blocks = if has_blocks {
                String::new()
            } else {
                String::from("0\nSECTION\n2\nBLOCKS\n")
            };
            for block in &self.block_replacements {
                let (newline, marker) = if self.source.contains("\r\n0\r\nBLOCK\r\n") {
                    ("\r\n", format!("0\r\nBLOCK\r\n2\r\n{}\r\n", block.name()))
                } else {
                    ("\n", format!("0\nBLOCK\n2\n{}\n", block.name()))
                };
                if let Some(start) = self.source.find(&marker)
                    && let Some(relative_end) = self.source[start + marker.len()..]
                        .find(&format!("0{}ENDBLK{}", newline, newline))
                {
                    let end = start
                        + marker.len()
                        + relative_end
                        + format!("0{}ENDBLK{}", newline, newline).len();
                    changes.push((start, end, serialize_block(block, newline)));
                }
            }
            for block in self
                .block_definitions
                .iter()
                .skip(self.imported_block_count)
            {
                blocks.push_str(&serialize_block(block, "\n"));
            }
            if has_blocks {
                if let Some(section) = self.source.find("0\nSECTION\n2\nBLOCKS\n")
                    && let Some(relative) = self.source[section..].find("0\nENDSEC\n")
                {
                    changes.push((section + relative, section + relative, blocks));
                }
            } else {
                blocks.push_str("0\nENDSEC\n");
                if let Some(at) = self.source.find("0\nSECTION\n2\nENTITIES\n") {
                    changes.push((at, at, blocks));
                }
            }
        }
        let source_group_spans = group_spans(&self.source);
        let all_group_objects = serialize_groups(
            &self.groups,
            &self.source,
            &generated_handles,
            &mut object_handle,
        );
        let mut group_objects = all_group_objects.clone();
        if !source_group_spans.is_empty() {
            for (index, (start, end)) in source_group_spans.iter().copied().enumerate() {
                changes.push((
                    start,
                    end,
                    if index == 0 {
                        all_group_objects.clone()
                    } else {
                        String::new()
                    },
                ));
            }
            group_objects.clear();
        }
        let viewport_entities = if self.source.contains("\n0\nVIEWPORT\n")
            || self.source.contains("\r\n0\r\nVIEWPORT\r\n")
        {
            String::new()
        } else {
            serialize_viewports(&self.layouts, &self.source, &mut object_handle, "\n")
        };
        if !viewport_entities.is_empty() {
            changes.push((self.insertion, self.insertion, viewport_entities));
        }
        let images = self
            .entities
            .iter()
            .filter_map(|entity| match entity {
                Entity::Image(image) => Some(image),
                _ => None,
            })
            .collect::<Vec<_>>();
        let layout_objects = serialize_layouts(&self.layouts, &self.source, &mut object_handle);
        let constraint_objects = serialize_constraints(
            &self.constraints,
            &self.source,
            &generated_handles,
            &mut object_handle,
        );
        let constraint_spans = constraint_spans(&self.source);
        let mut constraint_objects = constraint_objects;
        let mut mleader_style_objects =
            serialize_mleader_styles(&self.mleader_styles, &self.mleader_style_handles, "\n");
        let mleader_style_spans = mleader_style_spans(&self.source);
        if !mleader_style_spans.is_empty() {
            for (index, (start, end)) in mleader_style_spans.iter().copied().enumerate() {
                changes.push((
                    start,
                    end,
                    if index == 0 {
                        mleader_style_objects.clone()
                    } else {
                        String::new()
                    },
                ));
            }
            mleader_style_objects.clear();
        }
        if !constraint_spans.is_empty() {
            for (index, (start, end)) in constraint_spans.iter().copied().enumerate() {
                changes.push((
                    start,
                    end,
                    if index == 0 {
                        constraint_objects.clone()
                    } else {
                        String::new()
                    },
                ));
            }
            constraint_objects.clear();
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
        if !images.is_empty()
            || !layout_objects.is_empty()
            || !group_objects.is_empty()
            || !constraint_objects.is_empty()
            || !mleader_style_objects.is_empty()
        {
            let objects = format!(
                "{}{}{}{}{}",
                images
                    .iter()
                    .map(|image| serialize_image_def(image))
                    .collect::<String>(),
                group_objects,
                layout_objects,
                constraint_objects,
                mleader_style_objects
            );
            let marker = if output.contains("\r\n0\r\nEOF") {
                "\r\n0\r\nEOF"
            } else {
                "\n0\nEOF"
            };
            let newline = if marker.starts_with("\r") {
                "\r\n"
            } else {
                "\n"
            };
            let section_marker = format!("0{newline}SECTION{newline}2{newline}OBJECTS{newline}");
            let end_marker = format!("0{newline}ENDSEC{newline}");
            if let Some(section_start) = output.find(&section_marker) {
                if let Some(relative_end) =
                    output[section_start + section_marker.len()..].find(&end_marker)
                {
                    let at = section_start + section_marker.len() + relative_end;
                    output.insert_str(at, &objects.replace('\n', newline));
                }
            } else if let Some(at) = output.find(marker) {
                output.insert_str(
                    at,
                    &format!(
                        "0{newline}SECTION{newline}2{newline}OBJECTS{newline}{}0{newline}ENDSEC{newline}",
                        objects.replace('\n', newline)
                    ),
                );
            }
        }
        replace_header_style(&mut output, "$TEXTSTYLE", &self.current_text_style);
        replace_header_style(&mut output, "$DIMSTYLE", &self.current_dimension_style);
        replace_header_style(&mut output, "$CMLEADERSTYLE", &self.current_mleader_style);
        if self.entities.iter().any(|entity| {
            matches!(
                entity,
                Entity::Spline(_)
                    | Entity::Text(_)
                    | Entity::MText(_)
                    | Entity::Leader(_)
                    | Entity::MLeader(_)
                    | Entity::Table(_)
                    | Entity::Dimension(_)
                    | Entity::Hatch(_)
                    | Entity::Insert(_)
                    | Entity::XLine(_)
                    | Entity::Ray(_)
                    | Entity::Image(_)
                    | Entity::Mesh(_)
            )
        }) {
            output = output.replacen("$ACADVER\n1\nAC1009", "$ACADVER\n1\nAC1027", 1);
            output = output.replacen("$ACADVER\r\n1\r\nAC1009", "$ACADVER\r\n1\r\nAC1027", 1);
        }
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

fn group_error(code: u16) -> DxfError {
    DxfError {
        line: 0,
        kind: DxfErrorKind::UnsupportedProperty(code),
    }
}

fn remap_constraints(constraints: &ConstraintSet, mapping: &[Option<usize>]) -> ConstraintSet {
    let anchor = |anchor: Anchor| -> Option<Anchor> {
        let map_index = |index: usize| mapping.get(index).copied().flatten();
        match anchor {
            Anchor::EntityPoint(index) => map_index(index).map(Anchor::EntityPoint),
            Anchor::LineStart(index) => map_index(index).map(Anchor::LineStart),
            Anchor::LineEnd(index) => map_index(index).map(Anchor::LineEnd),
            Anchor::CircleCenter(index) => map_index(index).map(Anchor::CircleCenter),
        }
    };
    let index = |value: usize| mapping.get(value).copied().flatten();
    let kind = |kind: ConstraintKind| -> Option<ConstraintKind> {
        Some(match kind {
            ConstraintKind::Horizontal(value) => ConstraintKind::Horizontal(index(value)?),
            ConstraintKind::Vertical(value) => ConstraintKind::Vertical(index(value)?),
            ConstraintKind::Coincident(a, b) => ConstraintKind::Coincident(anchor(a)?, anchor(b)?),
            ConstraintKind::Parallel(LinePair::Lines(a, b)) => {
                ConstraintKind::Parallel(LinePair::Lines(index(a)?, index(b)?))
            }
            ConstraintKind::Perpendicular(LinePair::Lines(a, b)) => {
                ConstraintKind::Perpendicular(LinePair::Lines(index(a)?, index(b)?))
            }
            ConstraintKind::Concentric(a, b) => ConstraintKind::Concentric(index(a)?, index(b)?),
            ConstraintKind::EqualLength(LinePair::Lines(a, b)) => {
                ConstraintKind::EqualLength(LinePair::Lines(index(a)?, index(b)?))
            }
            ConstraintKind::EqualRadius(a, b) => ConstraintKind::EqualRadius(index(a)?, index(b)?),
            ConstraintKind::Tangent { line, circle } => ConstraintKind::Tangent {
                line: index(line)?,
                circle: index(circle)?,
            },
            ConstraintKind::Fix(value) => ConstraintKind::Fix(anchor(value)?),
            ConstraintKind::Distance { a, b } => ConstraintKind::Distance {
                a: anchor(a)?,
                b: anchor(b)?,
            },
            ConstraintKind::LineLength(value) => ConstraintKind::LineLength(index(value)?),
            ConstraintKind::Radius(value) => ConstraintKind::Radius(index(value)?),
            ConstraintKind::Diameter(value) => ConstraintKind::Diameter(index(value)?),
            ConstraintKind::Angle(LinePair::Lines(a, b)) => {
                ConstraintKind::Angle(LinePair::Lines(index(a)?, index(b)?))
            }
        })
    };
    let mut result = ConstraintSet::new();
    for record in constraints.records() {
        let Some(kind) = kind(record.kind()) else {
            continue;
        };
        let added = if matches!(kind, ConstraintKind::Fix(_)) {
            record.fixed_point().and_then(|point| {
                result
                    .add_fixed(
                        match kind {
                            ConstraintKind::Fix(anchor) => anchor,
                            _ => unreachable!(),
                        },
                        point,
                    )
                    .ok()
            })
        } else {
            result.add(kind, record.value()).ok()
        };
        if let Some(id) = added {
            let _ = result.set_enabled(id, record.enabled());
        }
    }
    result
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
        header: &mut String,
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
            write!(header, "9\n$LTSCALE\n40\n{}\n", self.scale).expect("String write");
        }
    }
}

impl DimensionData {
    fn names() -> impl Iterator<Item = &'static str> {
        CornerParameter::ALL
            .into_iter()
            .map(CornerParameter::name)
            .chain(std::iter::once("PLINEWID"))
    }

    fn write_changes(&self, changes: &mut Vec<(usize, usize, String)>, header: &mut String) {
        for (index, name) in Self::names().enumerate() {
            let value = self.values[index];
            if let Some(field) = self.fields[index] {
                if value != field.value {
                    changes.push((field.start, field.end, value.to_string()));
                }
            } else if value != 0.0 {
                write!(header, "9\n${name}\n40\n{value}\n").expect("String write");
            }
        }
    }
}

fn read_dimensions(input: &str) -> Result<DimensionData, DxfError> {
    let mut data = DimensionData::default();
    let mut groups = Groups::new(input);
    let mut in_header = false;
    while let Some(group) = groups.next()? {
        match (group.code, group.value) {
            (0, "SECTION") => in_header = groups.required()?.value == "HEADER",
            (0, "ENDSEC") => in_header = false,
            (9, variable) if in_header => {
                let Some(index) = DimensionData::names()
                    .position(|name| variable.strip_prefix('$') == Some(name))
                else {
                    continue;
                };
                if data.fields[index].is_some() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::DuplicateField(9),
                    });
                }
                let field = groups.required()?;
                if field.code != 40 {
                    return Err(DxfError {
                        line: field.line,
                        kind: DxfErrorKind::Expected("drawing dimension in group 40"),
                    });
                }
                let coordinate = if index == 3 {
                    read_width(field)?
                } else {
                    coordinate(field)?
                };
                let value = if index == 3 {
                    coordinate.value
                } else {
                    crate::corner::dimension(coordinate.value).map_err(|error| DxfError {
                        line: field.line,
                        kind: DxfErrorKind::Corner(error),
                    })?
                };
                if groups
                    .peek()?
                    .is_some_and(|next| !matches!(next.code, 0 | 9))
                {
                    return Err(DxfError {
                        line: field.line,
                        kind: DxfErrorKind::Expected("one drawing dimension value"),
                    });
                }
                data.values[index] = value;
                data.fields[index] = Some(coordinate);
            }
            _ => {}
        }
    }
    Ok(data)
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
                if version.code != 1
                    || !matches!(
                        version.value,
                        "AC1009"
                            | "AC1012"
                            | "AC1014"
                            | "AC1015"
                            | "AC1018"
                            | "AC1021"
                            | "AC1024"
                            | "AC1027"
                            | "AC1032"
                            | "AC1038"
                    )
                {
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
    if section == "OBJECTS" {
        loop {
            let group = groups.required()?;
            if group.code == 0 && group.value == "ENDSEC" {
                return Ok(());
            }
            if matches!(group.code, 5 | 105) {
                read_handle(group, handles)?;
            }
        }
    }
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

fn read_block_definitions(input: &str) -> Result<Vec<BlockDefinition>, DxfError> {
    let mut groups = Groups::new(input);
    let mut in_blocks = false;
    let mut result = Vec::new();
    let mut handles = HashSet::new();
    while let Some(group) = groups.next()? {
        if group.code == 2 && group.value == "BLOCKS" {
            in_blocks = true;
            continue;
        }
        if !in_blocks {
            continue;
        }
        if group.code == 0 && group.value == "ENDSEC" {
            break;
        }
        if group.code != 0 || group.value != "BLOCK" {
            continue;
        }
        let header = record_fields(&mut groups)?;
        let name = unique_field(&header, 2)?
            .map(|g| g.value.to_owned())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let base = Point3::new(
            field_number(&header, 10, group.line)?.unwrap_or(0.0),
            field_number(&header, 20, group.line)?.unwrap_or(0.0),
            field_number(&header, 30, group.line)?.unwrap_or(0.0),
        )
        .map_err(|_| DxfError {
            line: group.line,
            kind: DxfErrorKind::InvalidCoordinate(10),
        })?;
        let mut definition = BlockDefinition::new(name, base, Vec::new());
        let flags = field_number(&header, 70, group.line)?.unwrap_or(0.0) as i32;
        if (flags & 4 != 0 || flags & 8 != 0)
            && let Some(path) = field_text(&header, 1)?
        {
            definition.set_xref_source(Some(path), flags & 8 != 0);
        }
        loop {
            let Some(child) = groups.next()? else {
                break;
            };
            if child.code != 0 {
                continue;
            }
            if child.value == "ENDBLK" {
                let _ = record_fields(&mut groups)?;
                break;
            }
            if child.value == "ATTDEF" {
                let fields = record_fields(&mut groups)?;
                let tag = field_text(&fields, 2)?.unwrap_or_default();
                let prompt = field_text(&fields, 3)?.unwrap_or_default();
                let value = field_text(&fields, 1)?.unwrap_or_default();
                let position = Point3::new(
                    field_number(&fields, 10, child.line)?.unwrap_or(0.0),
                    field_number(&fields, 20, child.line)?.unwrap_or(0.0),
                    field_number(&fields, 30, child.line)?.unwrap_or(0.0),
                )
                .map_err(|_| DxfError {
                    line: child.line,
                    kind: DxfErrorKind::InvalidCoordinate(10),
                })?;
                definition.add_attribute_definition(crate::AttributeDefinition::new(
                    tag, prompt, value, position,
                ));
                continue;
            }
            let parsed = match child.value {
                "POINT" | "LINE" | "XLINE" | "RAY" | "CIRCLE" | "ARC" | "ELLIPSE" | "3DFACE" => {
                    read_entity(&mut groups, child, &mut handles)
                }
                "SPLINE" | "TEXT" | "MTEXT" | "LEADER" | "MULTILEADER" | "TABLE" | "ACAD_TABLE"
                | "DIMENSION" | "HATCH" | "INSERT" | "IMAGE" | "MESH" => read_extended_entity(
                    &mut groups,
                    child,
                    &mut handles,
                    &[],
                    &[],
                    &HashMap::new(),
                    &[],
                    &[],
                ),
                "POLYLINE" => read_polyline(&mut groups, child, &mut handles),
                _ => {
                    let _ = record_fields(&mut groups)?;
                    continue;
                }
            };
            if let Ok((entity, _)) = parsed {
                definition.add_entity(entity);
            }
        }
        result.push(definition);
    }
    Ok(result)
}

fn read_image_def_paths(input: &str) -> HashMap<String, String> {
    let mut groups = Groups::new(input);
    let mut objects = false;
    let mut paths = HashMap::new();
    while let Ok(Some(group)) = groups.next() {
        if group.code == 2 && group.value == "OBJECTS" {
            objects = true;
            continue;
        }
        if !objects {
            continue;
        }
        if group.code == 0 && group.value == "ENDSEC" {
            break;
        }
        if group.code == 0 && group.value == "IMAGEDEF" {
            let Ok(fields) = record_fields(&mut groups) else {
                break;
            };
            let Some(handle) = fields
                .iter()
                .find(|field| field.code == 5)
                .map(|field| field.value.to_owned())
            else {
                continue;
            };
            if let Some(path) = fields
                .iter()
                .find(|field| field.code == 1)
                .map(|field| field.value.to_owned())
            {
                paths.insert(handle, path);
            }
        }
    }
    paths
}

fn read_groups(input: &str) -> Vec<EntityGroup> {
    let handles = entity_handles(input);
    let mut groups = Groups::new(input);
    let mut in_objects = false;
    let mut result = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "OBJECTS" {
            in_objects = true;
            continue;
        }
        if !in_objects {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "GROUP" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields
            .iter()
            .find(|field| field.code == 2)
            .map(|field| field.value)
        else {
            continue;
        };
        let description = fields
            .iter()
            .find(|field| field.code == 300)
            .map_or("", |field| field.value);
        let selectable = fields
            .iter()
            .find(|field| field.code == 71)
            .is_none_or(|field| field.value.trim() != "0");
        let mut members = fields
            .iter()
            .filter(|field| field.code == 340)
            .filter_map(|field| handles.get(field.value).copied())
            .collect::<Vec<_>>();
        members.sort_unstable();
        members.dedup();
        if let Ok(group) = EntityGroup::new(name, description, selectable, members) {
            result.push(group);
        }
    }
    result
}

fn group_spans(input: &str) -> Vec<(usize, usize)> {
    let mut groups = Groups::new(input);
    let mut objects = false;
    let mut spans = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "OBJECTS" {
            objects = true;
            continue;
        }
        if !objects {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "GROUP" {
            continue;
        }
        if record_fields(&mut groups).is_err() {
            break;
        }
        let end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(input.len(), |next| next.start);
        spans.push((marker.start, end));
    }
    spans
}

fn entity_handles(input: &str) -> HashMap<String, usize> {
    let mut groups = Groups::new(input);
    let mut entities = false;
    let mut index = 0;
    let mut result = HashMap::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 0 && marker.value == "SECTION" {
            if let Ok(Some(section)) = groups.next() {
                entities = section.code == 2 && section.value == "ENTITIES";
            }
            continue;
        }
        if !entities || marker.code != 0 {
            continue;
        }
        if marker.value == "ENDSEC" {
            break;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        if !matches!(
            marker.value,
            "POINT"
                | "LINE"
                | "XLINE"
                | "RAY"
                | "CIRCLE"
                | "ARC"
                | "ELLIPSE"
                | "3DFACE"
                | "SPLINE"
                | "TEXT"
                | "MTEXT"
                | "LEADER"
                | "MULTILEADER"
                | "TABLE"
                | "ACAD_TABLE"
                | "DIMENSION"
                | "HATCH"
                | "IMAGE"
                | "INSERT"
                | "POLYLINE"
                | "LWPOLYLINE"
                | "MLINE"
                | "MESH"
        ) {
            continue;
        }
        if let Some(handle) = fields.iter().find(|field| field.code == 5) {
            result.insert(handle.value.to_owned(), index);
        }
        index += 1;
    }
    result
}

fn serialize_groups(
    groups: &[EntityGroup],
    source: &str,
    generated_handles: &HashMap<usize, String>,
    handle: &mut u64,
) -> String {
    if groups.is_empty() {
        return String::new();
    }
    let entity_handles = entity_handles(source);
    let mut output = String::new();
    let mut group_handles = Vec::with_capacity(groups.len());
    for group in groups {
        let group_handle = *handle;
        group_handles.push((group.name(), group_handle));
        output.push_str(&format!(
            "0\nGROUP\n5\n{:X}\n100\nAcDbGroup\n300\n{}\n70\n0\n71\n{}\n",
            group_handle,
            group.description(),
            u8::from(group.selectable())
        ));
        for member in group.members() {
            if let Some(entity_handle) = generated_handles.get(member).or_else(|| {
                entity_handles
                    .iter()
                    .find_map(|(value, index)| (*index == *member).then_some(value))
            }) {
                output.push_str(&format!("340\n{}\n", entity_handle));
            }
        }
        *handle = handle.saturating_add(1);
    }
    let dictionary_handle = *handle;
    output.push_str(&format!(
        "0\nDICTIONARY\n5\n{:X}\n100\nAcDbDictionary\n280\n0\n",
        dictionary_handle
    ));
    for (name, group_handle) in group_handles {
        output.push_str(&format!("3\n{name}\n350\n{group_handle:X}\n"));
    }
    *handle = handle.saturating_add(1);
    output
}

fn read_constraints(input: &str, handles: &HashMap<String, usize>) -> ConstraintSet {
    let mut groups = Groups::new(input);
    let mut result = ConstraintSet::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code != 0 || marker.value != "XRECORD" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        for field in fields.iter().filter(|field| field.code == 1000) {
            let Some(payload) = field.value.strip_prefix("OpenCadConstraints|v1|") else {
                continue;
            };
            let mut parts = payload.split('|');
            let kind = parts
                .next()
                .and_then(|value| decode_constraint_kind(value, handles));
            let value = parts.next().and_then(|value| value.parse::<f64>().ok());
            let fixed = parts.next().and_then(|value| {
                let mut coords = value.split(',').filter_map(|part| part.parse::<f64>().ok());
                Point3::new(coords.next()?, coords.next()?, coords.next()?).ok()
            });
            let enabled = parts
                .next()
                .and_then(|value| value.parse::<u8>().ok())
                .is_none_or(|value| value != 0);
            let Some(kind) = kind else { continue };
            if let Some(point) = fixed
                && let ConstraintKind::Fix(anchor) = kind
            {
                let id = result.add_fixed(anchor, point);
                if let Ok(id) = id {
                    let _ = result.set_enabled(id, enabled);
                }
                continue;
            }
            if let Ok(id) = result.add(kind, value) {
                let _ = result.set_enabled(id, enabled);
            }
        }
    }
    result
}

fn constraint_spans(input: &str) -> Vec<(usize, usize)> {
    let mut groups = Groups::new(input);
    let mut objects = false;
    let mut spans = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "OBJECTS" {
            objects = true;
            continue;
        }
        if !objects {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "XRECORD" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        if fields
            .iter()
            .any(|field| field.code == 1000 && field.value.starts_with("OpenCadConstraints|v1|"))
        {
            let end = groups
                .peek()
                .ok()
                .flatten()
                .map_or(input.len(), |next| next.start);
            spans.push((marker.start, end));
        }
    }
    spans
}

fn serialize_constraints(
    constraints: &ConstraintSet,
    source: &str,
    generated_handles: &HashMap<usize, String>,
    handle: &mut u64,
) -> String {
    if constraints.is_empty() {
        return String::new();
    }
    let mut by_index = entity_handles(source)
        .into_iter()
        .map(|(handle, index)| (index, handle))
        .collect::<HashMap<_, _>>();
    by_index.extend(
        generated_handles
            .iter()
            .map(|(index, handle)| (*index, handle.clone())),
    );
    let mut output = String::new();
    let mut records = String::new();
    for record in constraints.records() {
        let Some(kind) = encode_constraint_kind(record.kind(), &by_index) else {
            continue;
        };
        let value = record
            .value()
            .map_or_else(|| "-".to_owned(), |value| value.to_string());
        let fixed = record.fixed_point().map_or_else(
            || "-".to_owned(),
            |point| format!("{},{},{}", point.x(), point.y(), point.z()),
        );
        records.push_str(&format!(
            "1000\nOpenCadConstraints|v1|{kind}|{value}|{fixed}|{}\n",
            u8::from(record.enabled())
        ));
    }
    if records.is_empty() {
        return output;
    }
    output.push_str(&format!(
        "0\nXRECORD\n5\n{:X}\n100\nAcDbXrecord\n{}",
        *handle, records
    ));
    *handle = handle.saturating_add(1);
    output
}

fn insert_entity_handle(output: &mut String, handle: &str) {
    let Some(first_end) = output.find('\n') else {
        return;
    };
    let name_start = first_end + 1;
    let Some(name_end) = output[name_start..].find('\n') else {
        return;
    };
    let at = name_start + name_end + 1;
    output.insert_str(at, &format!("5\n{handle}\n"));
}

fn encode_constraint_kind(
    kind: ConstraintKind,
    handles: &HashMap<usize, String>,
) -> Option<String> {
    let entity = |index: usize| handles.get(&index).cloned();
    let anchor = |anchor: Anchor| -> Option<String> {
        let (tag, index) = match anchor {
            Anchor::EntityPoint(index) => ("E", index),
            Anchor::LineStart(index) => ("S", index),
            Anchor::LineEnd(index) => ("N", index),
            Anchor::CircleCenter(index) => ("C", index),
        };
        Some(format!("{tag}:{}", entity(index)?))
    };
    Some(match kind {
        ConstraintKind::Horizontal(index) => format!("H:{}", entity(index)?),
        ConstraintKind::Vertical(index) => format!("V:{}", entity(index)?),
        ConstraintKind::Coincident(a, b) => format!("K:{},{}", anchor(a)?, anchor(b)?),
        ConstraintKind::Parallel(LinePair::Lines(a, b)) => {
            format!("P:{},{}", entity(a)?, entity(b)?)
        }
        ConstraintKind::Perpendicular(LinePair::Lines(a, b)) => {
            format!("Q:{},{}", entity(a)?, entity(b)?)
        }
        ConstraintKind::Concentric(a, b) => format!("O:{},{}", entity(a)?, entity(b)?),
        ConstraintKind::EqualLength(LinePair::Lines(a, b)) => {
            format!("L:{},{}", entity(a)?, entity(b)?)
        }
        ConstraintKind::EqualRadius(a, b) => format!("R:{},{}", entity(a)?, entity(b)?),
        ConstraintKind::Tangent { line, circle } => {
            format!("T:{},{}", entity(line)?, entity(circle)?)
        }
        ConstraintKind::Fix(anchor_value) => format!("F:{}", anchor(anchor_value)?),
        ConstraintKind::Distance { a, b } => format!("D:{},{}", anchor(a)?, anchor(b)?),
        ConstraintKind::LineLength(index) => format!("I:{}", entity(index)?),
        ConstraintKind::Radius(index) => format!("A:{}", entity(index)?),
        ConstraintKind::Diameter(index) => format!("M:{}", entity(index)?),
        ConstraintKind::Angle(LinePair::Lines(a, b)) => format!("G:{},{}", entity(a)?, entity(b)?),
    })
}

fn decode_constraint_kind(value: &str, handles: &HashMap<String, usize>) -> Option<ConstraintKind> {
    let (tag, args) = value.split_once(':')?;
    let entity = |text: &str| handles.get(text).copied();
    let anchor = |text: &str| -> Option<Anchor> {
        let (tag, handle) = text.split_once(':')?;
        let index = entity(handle)?;
        Some(match tag {
            "E" => Anchor::EntityPoint(index),
            "S" => Anchor::LineStart(index),
            "N" => Anchor::LineEnd(index),
            "C" => Anchor::CircleCenter(index),
            _ => return None,
        })
    };
    let mut values = args.split(',');
    Some(match tag {
        "H" => ConstraintKind::Horizontal(entity(args)?),
        "V" => ConstraintKind::Vertical(entity(args)?),
        "K" => ConstraintKind::Coincident(anchor(values.next()?)?, anchor(values.next()?)?),
        "P" => ConstraintKind::Parallel(LinePair::Lines(
            entity(values.next()?)?,
            entity(values.next()?)?,
        )),
        "Q" => ConstraintKind::Perpendicular(LinePair::Lines(
            entity(values.next()?)?,
            entity(values.next()?)?,
        )),
        "O" => ConstraintKind::Concentric(entity(values.next()?)?, entity(values.next()?)?),
        "L" => ConstraintKind::EqualLength(LinePair::Lines(
            entity(values.next()?)?,
            entity(values.next()?)?,
        )),
        "R" => ConstraintKind::EqualRadius(entity(values.next()?)?, entity(values.next()?)?),
        "T" => ConstraintKind::Tangent {
            line: entity(values.next()?)?,
            circle: entity(values.next()?)?,
        },
        "F" => ConstraintKind::Fix(anchor(args)?),
        "D" => ConstraintKind::Distance {
            a: anchor(values.next()?)?,
            b: anchor(values.next()?)?,
        },
        "I" => ConstraintKind::LineLength(entity(args)?),
        "A" => ConstraintKind::Radius(entity(args)?),
        "M" => ConstraintKind::Diameter(entity(args)?),
        "G" => ConstraintKind::Angle(LinePair::Lines(
            entity(values.next()?)?,
            entity(values.next()?)?,
        )),
        _ => return None,
    })
}

fn read_mleader_fields(
    fields: &[Group<'_>],
    marker: Group<'_>,
    properties: EntityProperties,
    end: usize,
    styles: &[MLeaderStyle],
    style_handles: &HashMap<u64, String>,
    block_definitions: &[BlockDefinition],
) -> Result<(Entity, EntityRecord), DxfError> {
    let xs = fields
        .iter()
        .filter(|field| field.code == 10)
        .map(|field| read_number(*field))
        .collect::<Result<Vec<_>, _>>()?;
    let ys = fields
        .iter()
        .filter(|field| field.code == 20)
        .map(|field| read_number(*field))
        .collect::<Result<Vec<_>, _>>()?;
    let zs = fields
        .iter()
        .filter(|field| field.code == 30)
        .map(|field| read_number(*field))
        .collect::<Result<Vec<_>, _>>()?;
    if xs.len() < 2 || xs.len() != ys.len() {
        return Err(DxfError {
            line: marker.line,
            kind: DxfErrorKind::MissingField(10),
        });
    }
    let vertices = xs
        .into_iter()
        .zip(ys)
        .enumerate()
        .map(|(index, (x, y))| Point3::new(x, y, zs.get(index).copied().unwrap_or(0.0)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DxfError {
            line: marker.line,
            kind: DxfErrorKind::InvalidCoordinate(10),
        })?;
    let branch = MLeaderBranch::new(vertices).map_err(|error| DxfError {
        line: marker.line,
        kind: DxfErrorKind::Corner(error),
    })?;
    let location = branch.vertices()[branch.vertices().len() - 1];
    let content_text = fields
        .iter()
        .find(|field| field.code == 304)
        .map(|field| field.value.to_owned())
        .unwrap_or_default();
    let text = MText::new(content_text, location, 2.5, 1.0).map_err(|error| DxfError {
        line: marker.line,
        kind: DxfErrorKind::Corner(error),
    })?;
    let mut mleader =
        MLeader::new(branch.clone(), MLeaderContent::MText(text)).map_err(|error| DxfError {
            line: marker.line,
            kind: DxfErrorKind::Corner(error),
        })?;
    if fields
        .iter()
        .find(|field| field.code == 296)
        .and_then(|field| field.value.parse::<i32>().ok())
        == Some(1)
        && let Some(handle) = fields
            .iter()
            .find(|field| field.code == 341)
            .and_then(|field| u64::from_str_radix(field.value.trim(), 16).ok())
        && let Some(block) = block_definitions
            .iter()
            .find(|block| block_handle(block.name()) == handle)
    {
        let position = point_fields(fields, 15, 25, 35, marker.line).unwrap_or(location);
        let mut insert = Insert::new(block.name(), position);
        if let Some(scale) = fields
            .iter()
            .find(|field| field.code == 16)
            .and_then(|field| field.value.parse::<f64>().ok())
        {
            insert.set_scale([scale; 3]).map_err(|_| DxfError {
                line: marker.line,
                kind: DxfErrorKind::InvalidCoordinate(16),
            })?;
        }
        let matrix = fields
            .iter()
            .filter(|field| field.code == 47)
            .map(|field| {
                field.value.parse::<f64>().map_err(|_| DxfError {
                    line: field.line,
                    kind: DxfErrorKind::InvalidCoordinate(47),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if matrix.len() >= 16 {
            insert
                .set_scale([matrix[0], matrix[5], matrix[10]])
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(47),
                })?;
        }
        if let Some(rotation) = fields
            .iter()
            .find(|field| field.code == 46)
            .and_then(|field| field.value.parse::<f64>().ok())
        {
            insert.set_rotation(rotation).map_err(|_| DxfError {
                line: marker.line,
                kind: DxfErrorKind::InvalidCoordinate(46),
            })?;
        }
        mleader =
            MLeader::new(branch, MLeaderContent::Block(insert)).map_err(|error| DxfError {
                line: marker.line,
                kind: DxfErrorKind::Corner(error),
            })?;
    }
    if let Some(handle) = fields
        .iter()
        .find(|field| field.code == 340)
        .and_then(|field| u64::from_str_radix(field.value.trim(), 16).ok())
        && let Some(name) = style_handles.get(&handle).map(String::as_str).or_else(|| {
            styles
                .iter()
                .find(|style| mleader_style_handle(style.name()) == handle)
                .map(MLeaderStyle::name)
        })
    {
        mleader.set_style_name(name).map_err(|error| DxfError {
            line: marker.line,
            kind: DxfErrorKind::Corner(error),
        })?;
    }
    let entity = Entity::MLeader(mleader);
    Ok((
        entity.clone(),
        extended_record(&entity, marker, properties, end),
    ))
}

fn read_mesh_fields(fields: &[Group<'_>], line: usize) -> Result<Mesh3, DxfError> {
    if let Some(field) = fields.iter().find(|field| field.code == 71)
        && field.value.parse::<u16>().ok() != Some(2)
    {
        return Err(DxfError {
            line: field.line,
            kind: DxfErrorKind::UnsupportedProperty(71),
        });
    }
    for code in [91, 94, 95] {
        if let Some(field) = fields.iter().find(|field| field.code == code)
            && field.value.parse::<u32>().ok() != Some(0)
        {
            return Err(DxfError {
                line: field.line,
                kind: DxfErrorKind::UnsupportedProperty(code),
            });
        }
    }
    let xs = fields
        .iter()
        .filter(|field| field.code == 10)
        .map(|field| read_number(*field))
        .collect::<Result<Vec<_>, _>>()?;
    let ys = fields
        .iter()
        .filter(|field| field.code == 20)
        .map(|field| read_number(*field))
        .collect::<Result<Vec<_>, _>>()?;
    let zs = fields
        .iter()
        .filter(|field| field.code == 30)
        .map(|field| read_number(*field))
        .collect::<Result<Vec<_>, _>>()?;
    if xs.is_empty() || xs.len() != ys.len() || (!zs.is_empty() && zs.len() != xs.len()) {
        return Err(DxfError {
            line,
            kind: DxfErrorKind::MissingField(10),
        });
    }
    let vertex_header = fields
        .iter()
        .find(|field| field.code == 92)
        .ok_or(DxfError {
            line,
            kind: DxfErrorKind::MissingField(92),
        })?;
    let expected = vertex_header.value.parse::<usize>().map_err(|_| DxfError {
        line: vertex_header.line,
        kind: DxfErrorKind::UnsupportedProperty(92),
    })?;
    if expected != xs.len() {
        return Err(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(92),
        });
    }
    let vertices = xs
        .into_iter()
        .zip(ys)
        .enumerate()
        .map(|(index, (x, y))| {
            Point3::new(x, y, zs.get(index).copied().unwrap_or(0.0)).map_err(|_| DxfError {
                line,
                kind: DxfErrorKind::InvalidCoordinate(10),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let list_header = fields
        .iter()
        .position(|field| field.code == 93)
        .ok_or(DxfError {
            line,
            kind: DxfErrorKind::MissingField(93),
        })?;
    let expected = fields[list_header]
        .value
        .parse::<usize>()
        .map_err(|_| DxfError {
            line: fields[list_header].line,
            kind: DxfErrorKind::UnsupportedProperty(93),
        })?;
    let list_end = fields[list_header + 1..]
        .iter()
        .position(|field| matches!(field.code, 94 | 95))
        .map_or(fields.len(), |offset| list_header + 1 + offset);
    let lists = fields[list_header + 1..list_end]
        .iter()
        .filter(|field| field.code == 90)
        .take(expected)
        .map(|field| {
            let value = field.value.parse::<i64>().map_err(|_| DxfError {
                line: field.line,
                kind: DxfErrorKind::UnsupportedProperty(90),
            })?;
            usize::try_from(value).map_err(|_| DxfError {
                line: field.line,
                kind: DxfErrorKind::UnsupportedProperty(90),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if expected != lists.len() {
        return Err(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(93),
        });
    }
    let mut faces = Vec::new();
    let mut cursor = 0;
    while cursor < lists.len() {
        let count = lists[cursor];
        cursor += 1;
        if count < 3 || count > lists.len().saturating_sub(cursor) {
            return Err(DxfError {
                line,
                kind: DxfErrorKind::UnsupportedProperty(90),
            });
        }
        let polygon = &lists[cursor..cursor + count];
        cursor += count;
        let mut points = Vec::with_capacity(count);
        for value in polygon {
            let point = vertices.get(*value).copied().ok_or(DxfError {
                line,
                kind: DxfErrorKind::UnsupportedProperty(90),
            })?;
            points.push(point);
        }
        for [a, b, c] in triangulate_mesh_polygon(&points).ok_or(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(90),
        })? {
            let face =
                Face3::triangle(points[a], points[b], points[c]).map_err(|error| DxfError {
                    line,
                    kind: DxfErrorKind::Corner(error),
                })?;
            faces.push(face);
        }
    }
    Mesh3::new(faces).map_err(|error| DxfError {
        line,
        kind: DxfErrorKind::Corner(error),
    })
}

fn triangulate_mesh_polygon(points: &[Point3]) -> Option<Vec<[usize; 3]>> {
    if !(3..=4096).contains(&points.len()) {
        return None;
    }
    let mut normal = [0.0; 3];
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        normal[0] += (a.y() - b.y()) * (a.z() + b.z());
        normal[1] += (a.z() - b.z()) * (a.x() + b.x());
        normal[2] += (a.x() - b.x()) * (a.y() + b.y());
    }
    let axis = normal
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map(|(axis, _)| axis)?;
    let project = |point: Point3| match axis {
        0 => (point.y(), point.z()),
        1 => (point.x(), point.z()),
        _ => (point.x(), point.y()),
    };
    let projected = points.iter().copied().map(project).collect::<Vec<_>>();
    let area = projected
        .iter()
        .zip(projected.iter().cycle().skip(1))
        .take(projected.len())
        .map(|(&(ax, ay), &(bx, by))| ax * by - bx * ay)
        .sum::<f64>();
    if !area.is_finite() || area.abs() <= f64::EPSILON {
        return None;
    }
    let orientation = area.signum();
    let mut remaining = (0..points.len()).collect::<Vec<_>>();
    let mut triangles = Vec::with_capacity(points.len() - 2);
    while remaining.len() > 3 {
        let mut clipped = false;
        for position in 0..remaining.len() {
            let previous = remaining[(position + remaining.len() - 1) % remaining.len()];
            let current = remaining[position];
            let next = remaining[(position + 1) % remaining.len()];
            let (ax, ay) = projected[previous];
            let (bx, by) = projected[current];
            let (cx, cy) = projected[next];
            let cross = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            if cross * orientation <= f64::EPSILON {
                continue;
            }
            let contains = remaining.iter().any(|index| {
                if *index == previous || *index == current || *index == next {
                    return false;
                }
                let (px, py) = projected[*index];
                point_in_triangle_2d((px, py), (ax, ay), (bx, by), (cx, cy), orientation)
            });
            if contains {
                continue;
            }
            // Keep the authored polygon winding.  `orientation` is only used
            // to identify convex ears; reversing here silently changes the
            // surface normal of clockwise mesh faces.
            triangles.push([previous, current, next]);
            remaining.remove(position);
            clipped = true;
            break;
        }
        if !clipped {
            return None;
        }
    }
    let [a, b, c] = [remaining[0], remaining[1], remaining[2]];
    triangles.push([a, b, c]);
    Some(triangles)
}

fn point_in_triangle_2d(
    point: (f64, f64),
    a: (f64, f64),
    b: (f64, f64),
    c: (f64, f64),
    orientation: f64,
) -> bool {
    let cross = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    cross(a, b, point) * orientation >= -f64::EPSILON
        && cross(b, c, point) * orientation >= -f64::EPSILON
        && cross(c, a, point) * orientation >= -f64::EPSILON
}

fn read_mline_styles(input: &str) -> Vec<MLineStyle> {
    let mut groups = Groups::new(input);
    let mut tables = false;
    let mut result = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "TABLES" {
            tables = true;
            continue;
        }
        if !tables {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "MLINESTYLE" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields
            .iter()
            .find(|field| field.code == 2)
            .map(|field| field.value)
        else {
            continue;
        };
        let offsets = fields
            .iter()
            .filter(|field| field.code == 49)
            .filter_map(|field| field.value.parse::<f64>().ok())
            .collect::<Vec<_>>();
        let Ok(mut style) = MLineStyle::new(
            name,
            if offsets.is_empty() {
                vec![-0.5, 0.5]
            } else {
                offsets
            },
        ) else {
            continue;
        };
        let _ = style.set_elements(
            fields
                .iter()
                .filter(|field| field.code == 49)
                .filter_map(|field| MLineElement::new(field.value.parse().ok()?).ok())
                .collect(),
        );
        style.set_caps(
            fields
                .iter()
                .find(|field| field.code == 73)
                .and_then(|field| field.value.parse::<i32>().ok())
                .is_some_and(|value| value != 0),
            fields
                .iter()
                .find(|field| field.code == 74)
                .and_then(|field| field.value.parse::<i32>().ok())
                .is_some_and(|value| value != 0),
        );
        result.push(style);
    }
    result
}

fn read_text_styles(input: &str) -> Vec<TextStyle> {
    let mut groups = Groups::new(input);
    let mut tables = false;
    let mut result = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "TABLES" {
            tables = true;
            continue;
        }
        if !tables {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "STYLE" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields
            .iter()
            .find(|field| field.code == 2)
            .map(|field| field.value)
        else {
            continue;
        };
        let Ok(mut style) = TextStyle::new(name) else {
            continue;
        };
        if let Some(value) = fields
            .iter()
            .find(|field| field.code == 40)
            .and_then(|field| field.value.parse().ok())
        {
            let _ = style.set_height(value);
        }
        if let Some(value) = fields
            .iter()
            .find(|field| field.code == 41)
            .and_then(|field| field.value.parse().ok())
        {
            let _ = style.set_width_factor(value);
        }
        if let Some(value) = fields
            .iter()
            .find(|field| field.code == 50)
            .and_then(|field| field.value.parse().ok())
        {
            let _ = style.set_oblique(value);
        }
        if let Some(value) = fields.iter().find(|field| field.code == 3) {
            let _ = style.set_font(value.value);
        }
        result.push(style);
    }
    result
}

type DimensionStyleSetter = fn(&mut DimensionStyle, f64) -> bool;
type MLeaderStyleSetter = fn(&mut MLeaderStyle, f64) -> bool;

fn read_dimension_styles(input: &str) -> Vec<DimensionStyle> {
    let mut groups = Groups::new(input);
    let mut tables = false;
    let mut result = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "TABLES" {
            tables = true;
            continue;
        }
        if !tables {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "DIMSTYLE" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields
            .iter()
            .find(|field| field.code == 2)
            .map(|field| field.value)
        else {
            continue;
        };
        let Ok(mut style) = DimensionStyle::new(name) else {
            continue;
        };
        let setters: &[(u16, DimensionStyleSetter)] = &[
            (140, |s: &mut DimensionStyle, v: f64| {
                s.set_text_height(v).is_ok()
            }),
            (41, |s: &mut DimensionStyle, v: f64| {
                s.set_arrow_size(v).is_ok()
            }),
            (40, |s: &mut DimensionStyle, v: f64| {
                s.set_overall_scale(v).is_ok()
            }),
            (144, |s: &mut DimensionStyle, v: f64| {
                s.set_linear_scale(v).is_ok()
            }),
            (147, |s: &mut DimensionStyle, v: f64| s.set_gap(v).is_ok()),
            (42, |s: &mut DimensionStyle, v: f64| {
                s.set_extension_offset(v).is_ok()
            }),
        ];
        for &(code, setter) in setters {
            if let Some(value) = fields
                .iter()
                .find(|field| field.code == code)
                .and_then(|field| field.value.parse().ok())
            {
                let _ = setter(&mut style, value);
            }
        }
        if let Some(value) = fields
            .iter()
            .find(|field| field.code == 271)
            .and_then(|field| field.value.parse().ok())
        {
            let _ = style.set_precision(value);
        }
        result.push(style);
    }
    result
}

fn read_mleader_styles(input: &str) -> (Vec<MLeaderStyle>, HashMap<u64, String>) {
    let mut groups = Groups::new(input);
    let mut objects = false;
    let mut result = Vec::new();
    let mut handles = HashMap::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "OBJECTS" {
            objects = true;
            continue;
        }
        if !objects {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "MLEADERSTYLE" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields
            .iter()
            .find(|field| field.code == 2)
            .map(|field| field.value)
        else {
            continue;
        };
        if let Some(handle) = fields
            .iter()
            .find(|field| field.code == 5)
            .and_then(|field| u64::from_str_radix(field.value.trim(), 16).ok())
        {
            handles.insert(handle, name.to_owned());
        }
        let Ok(mut style) = MLeaderStyle::new(name) else {
            continue;
        };
        let setters: &[(u16, MLeaderStyleSetter)] = &[
            (40, |s: &mut MLeaderStyle, v: f64| {
                s.set_text_height(v).is_ok()
            }),
            (42, |s: &mut MLeaderStyle, v: f64| {
                s.set_arrow_size(v).is_ok()
            }),
            (43, |s: &mut MLeaderStyle, v: f64| {
                s.set_landing_gap(v).is_ok()
            }),
            (44, |s: &mut MLeaderStyle, v: f64| {
                s.set_dogleg_length(v).is_ok()
            }),
        ];
        for &(code, setter) in setters {
            if let Some(value) = fields
                .iter()
                .find(|field| field.code == code)
                .and_then(|field| field.value.parse().ok())
            {
                let _ = setter(&mut style, value);
            }
        }
        if let Some(value) = fields
            .iter()
            .find(|field| field.code == 290)
            .and_then(|field| field.value.parse::<i32>().ok())
        {
            style.set_text_frame(value != 0);
        }
        result.push(style);
    }
    (result, handles)
}

fn read_current_style_names(input: &str) -> (String, String, String) {
    let mut groups = Groups::new(input);
    let mut header = false;
    let mut text = "Standard".to_owned();
    let mut dimension = "Standard".to_owned();
    let mut mleader = "Standard".to_owned();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "HEADER" {
            header = true;
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            header = false;
        }
        if !header || marker.code != 9 {
            continue;
        }
        let Ok(Some(value)) = groups.next() else {
            continue;
        };
        if value.code != 1 {
            continue;
        }
        match marker.value {
            "$TEXTSTYLE" => text = value.value.to_owned(),
            "$DIMSTYLE" => dimension = value.value.to_owned(),
            "$MLEADERSTYLE" | "$CMLEADERSTYLE" => mleader = value.value.to_owned(),
            _ => {}
        }
    }
    (text, dimension, mleader)
}

fn serialize_mline_styles(styles: &[MLineStyle], newline: &str) -> String {
    if styles.is_empty() {
        return String::new();
    }
    let mut output = format!(
        "0{newline}TABLE{newline}2{newline}MLINESTYLE{newline}70{newline}{count}{newline}",
        count = styles.len()
    );
    output.push_str(&serialize_mline_style_records(styles, newline));
    output.push_str(&format!("0{newline}ENDTAB{newline}"));
    output
}

fn serialize_mline_style_records(styles: &[MLineStyle], newline: &str) -> String {
    let mut output = String::new();
    for style in styles {
        output.push_str(&format!("0{newline}MLINESTYLE{newline}2{newline}{name}{newline}70{newline}0{newline}71{newline}{count}{newline}73{newline}{start}{newline}74{newline}{end}{newline}", name = style.name(), count = style.elements().len(), start = i32::from(style.start_cap()), end = i32::from(style.end_cap())));
        for element in style.elements() {
            output.push_str(&format!("49{newline}{offset}{newline}62{newline}{color}{newline}6{newline}{linetype}{newline}", offset = element.offset(), color = element.color().map_or(256, i32::from), linetype = element.linetype().unwrap_or("BYLAYER")));
        }
    }
    output
}

fn serialize_annotation_tables(
    text_styles: &[TextStyle],
    dimension_styles: &[DimensionStyle],
    newline: &str,
) -> String {
    let mut output = String::new();
    if !text_styles.is_empty() {
        output.push_str(&format!(
            "0{newline}TABLE{newline}2{newline}STYLE{newline}70{newline}{}{newline}",
            text_styles.len()
        ));
        for style in text_styles {
            output.push_str(&format!("0{newline}STYLE{newline}2{newline}{}{newline}40{newline}{}{newline}41{newline}{}{newline}50{newline}{}{newline}3{newline}{}{newline}", style.name(), style.height(), style.width_factor(), style.oblique(), style.font()));
        }
        output.push_str(&format!("0{newline}ENDTAB{newline}"));
    }
    if !dimension_styles.is_empty() {
        output.push_str(&format!(
            "0{newline}TABLE{newline}2{newline}DIMSTYLE{newline}70{newline}{}{newline}",
            dimension_styles.len()
        ));
        for style in dimension_styles {
            output.push_str(&format!("0{newline}DIMSTYLE{newline}2{newline}{}{newline}105{newline}{:X}{newline}140{newline}{}{newline}41{newline}{}{newline}40{newline}{}{newline}144{newline}{}{newline}271{newline}{}{newline}147{newline}{}{newline}42{newline}{}{newline}", style.name(), style.name().bytes().map(u64::from).sum::<u64>(), style.text_height(), style.arrow_size(), style.overall_scale(), style.linear_scale(), style.precision(), style.gap(), style.extension_offset()));
        }
        output.push_str(&format!("0{newline}ENDTAB{newline}"));
    }
    output
}

fn append_header_style(header: &mut String, variable: &str, value: &str, source: &str) {
    if source.contains(variable) || header.contains(variable) {
        return;
    }
    header.push_str(&format!("9\n{variable}\n1\n{value}\n"));
}

fn validate_style_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<(), DxfError> {
    let mut seen = HashSet::new();
    for name in names {
        if name.trim().is_empty() || !seen.insert(name.to_ascii_lowercase()) {
            return Err(group_error(2));
        }
    }
    Ok(())
}

fn replace_header_style(output: &mut String, variable: &str, value: &str) {
    for ending in ["\r\n", "\n"] {
        let marker = format!("{ending}{variable}{ending}1{ending}");
        let Some(start) = output.find(&marker) else {
            continue;
        };
        let value_start = start + marker.len();
        let value_end = output[value_start..]
            .find(ending)
            .map_or(output.len(), |offset| value_start + offset);
        output.replace_range(value_start..value_end, value);
        return;
    }
}

fn serialize_mleader_styles(
    styles: &[MLeaderStyle],
    handles: &HashMap<u64, String>,
    newline: &str,
) -> String {
    styles
        .iter()
        .map(|style| {
            format!(
                "0{newline}MLEADERSTYLE{newline}5{newline}{:X}{newline}100{newline}AcDbMLeaderStyle{newline}2{newline}{}{newline}40{newline}{}{newline}42{newline}{}{newline}43{newline}{}{newline}44{newline}{}{newline}290{newline}{}{newline}",
                handles
                    .iter()
                    .find_map(|(handle, name)| {
                        name.eq_ignore_ascii_case(style.name()).then_some(*handle)
                    })
                    .unwrap_or_else(|| mleader_style_handle(style.name())),
                style.name(),
                style.text_height(),
                style.arrow_size(),
                style.landing_gap(),
                style.dogleg_length(),
                i32::from(style.text_frame())
            )
        })
        .collect()
}

/// Deterministic handles for authored MLEADERSTYLE records. Imported records are
/// resolved by their native handle when available; authored records use this stable
/// value so the entity 340 reference remains valid after a save and reopen.
fn mleader_style_handle(name: &str) -> u64 {
    name.bytes().fold(0xB000_u64, |hash, byte| {
        hash.wrapping_mul(33).wrapping_add(u64::from(byte))
    })
}

fn block_handle(name: &str) -> u64 {
    name.bytes().fold(0xC000_u64, |hash, byte| {
        hash.wrapping_mul(33).wrapping_add(u64::from(byte))
    })
}

fn mline_style_spans(input: &str) -> Vec<(usize, usize)> {
    let mut groups = Groups::new(input);
    let mut tables = false;
    let mut spans = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "TABLES" {
            tables = true;
            continue;
        }
        if !tables {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "MLINESTYLE" {
            continue;
        }
        if record_fields(&mut groups).is_err() {
            break;
        }
        let end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(input.len(), |next| next.start);
        spans.push((marker.start, end));
    }
    spans
}

fn mleader_style_spans(input: &str) -> Vec<(usize, usize)> {
    let mut groups = Groups::new(input);
    let mut objects = false;
    let mut spans = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "OBJECTS" {
            objects = true;
            continue;
        }
        if !objects {
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if marker.code != 0 || marker.value != "MLEADERSTYLE" {
            continue;
        }
        if record_fields(&mut groups).is_err() {
            break;
        }
        let end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(input.len(), |next| next.start);
        spans.push((marker.start, end));
    }
    spans
}

fn read_layouts(input: &str) -> crate::LayoutCollection {
    let mut layouts = crate::LayoutCollection::default();
    let mut groups = Groups::new(input);
    let mut objects = false;
    while let Ok(Some(group)) = groups.next() {
        if group.code == 2 && group.value == "OBJECTS" {
            objects = true;
            continue;
        }
        if !objects {
            continue;
        }
        if group.code == 0 && group.value == "ENDSEC" {
            break;
        }
        if group.code == 0 && group.value == "LAYOUT" {
            let Ok(fields) = record_fields(&mut groups) else {
                break;
            };
            let Some(name) = fields
                .iter()
                .find(|field| field.code == 1)
                .map(|field| field.value.to_owned())
            else {
                continue;
            };
            if name != "Model" {
                let width = field_number(&fields, 44, group.line)
                    .ok()
                    .flatten()
                    .filter(|v| *v > 0.0)
                    .unwrap_or(210.0);
                let height = field_number(&fields, 45, group.line)
                    .ok()
                    .flatten()
                    .filter(|v| *v > 0.0)
                    .unwrap_or(297.0);
                let Ok(mut setup) = crate::PageSetup::new(width, height) else {
                    continue;
                };
                let margins = [
                    field_number(&fields, 40, group.line)
                        .ok()
                        .flatten()
                        .unwrap_or(0.0),
                    field_number(&fields, 41, group.line)
                        .ok()
                        .flatten()
                        .unwrap_or(0.0),
                    field_number(&fields, 42, group.line)
                        .ok()
                        .flatten()
                        .unwrap_or(0.0),
                    field_number(&fields, 43, group.line)
                        .ok()
                        .flatten()
                        .unwrap_or(0.0),
                ];
                let _ = setup.set_margins(margins);
                if fields
                    .iter()
                    .find(|field| field.code == 73)
                    .and_then(|field| field.value.parse::<i32>().ok())
                    .is_some_and(|rotation| rotation % 2 != 0)
                {
                    setup.set_orientation(crate::Orientation::Landscape);
                }
                let numerator = field_number(&fields, 140, group.line)
                    .ok()
                    .flatten()
                    .unwrap_or(1.0);
                let denominator = field_number(&fields, 141, group.line)
                    .ok()
                    .flatten()
                    .unwrap_or(1.0);
                if denominator.abs() > f64::EPSILON {
                    let _ = setup.set_scale((numerator / denominator).abs().max(f64::EPSILON));
                }
                let _ = layouts.create(name, setup);
            }
        }
    }
    import_viewports(input, &mut layouts);
    layouts
}

fn serialize_layouts(layouts: &crate::LayoutCollection, source: &str, handle: &mut u64) -> String {
    let names = layouts.names().collect::<Vec<_>>();
    if names.len() <= 1
        || source.contains("\n0\nLAYOUT\n")
        || source.contains("\r\n0\r\nLAYOUT\r\n")
    {
        return String::new();
    }
    names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let layout = layouts.get(name)?;
            let setup = layout.setup();
            let paper = setup.paper();
            let margins = setup.margins();
            let (numerator, denominator) = (setup.scale(), 1.0);
            let rotation = matches!(setup.orientation(), crate::Orientation::Landscape) as u8;
            let result = format!(
                "0\nLAYOUT\n5\n{:X}\n100\nAcDbPlotSettings\n40\n{}\n41\n{}\n42\n{}\n43\n{}\n44\n{}\n45\n{}\n73\n{}\n140\n{}\n141\n{}\n100\nAcDbLayout\n1\n{}\n70\n0\n71\n{}\n",
                *handle,
                margins[0], margins[1], margins[2], margins[3], paper[0], paper[1],
                rotation, numerator, denominator, name, index
            );
            *handle = handle.saturating_add(1);
            Some(result)
        })
        .collect()
}

fn next_handle(source: &str) -> u64 {
    let mut groups = Groups::new(source);
    let mut maximum = 0_u64;
    while let Ok(Some(group)) = groups.next() {
        if matches!(group.code, 5 | 105 | 320..=369 | 390..=399 | 480 | 481)
            && let Ok(value) = u64::from_str_radix(group.value.trim(), 16)
        {
            maximum = maximum.max(value);
        }
    }
    maximum.saturating_add(1).max(1)
}

fn import_viewports(input: &str, layouts: &mut crate::LayoutCollection) {
    let fallback_target = layouts
        .names()
        .find(|name| *name != "Model")
        .unwrap_or("Model")
        .to_owned();
    let mut groups = Groups::new(input);
    let boundaries = viewport_clip_boundaries(input);
    while let Ok(Some(group)) = groups.next() {
        if group.code != 0 || group.value != "VIEWPORT" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let target = fields
            .iter()
            .find(|field| field.code == 410)
            .map(|field| field.value)
            .filter(|name| layouts.get(name).is_some())
            .unwrap_or(fallback_target.as_str())
            .to_owned();
        let number = |code| {
            fields
                .iter()
                .find(|field| field.code == code)
                .and_then(|field| field.value.parse::<f64>().ok())
                .filter(|value| value.is_finite())
        };
        let (cx, cy, width, height) = match (number(10), number(20), number(40), number(41)) {
            (Some(cx), Some(cy), Some(width), Some(height)) if width > 0.0 && height > 0.0 => {
                (cx, cy, width, height)
            }
            _ => continue,
        };
        let (wx, wy) = (number(12).unwrap_or(0.0), number(22).unwrap_or(0.0));
        let world_height = number(45).unwrap_or(height).max(f64::EPSILON);
        let scale = (height / world_height).max(f64::EPSILON);
        let world_width = (width / scale).max(f64::EPSILON);
        let world = [
            wx - world_width / 2.0,
            wy - world_height / 2.0,
            wx + world_width / 2.0,
            wy + world_height / 2.0,
        ];
        let paper = [
            cx - width / 2.0,
            cy - height / 2.0,
            cx + width / 2.0,
            cy + height / 2.0,
        ];
        if let Ok(mut viewport) = crate::Viewport::new(world, paper, scale) {
            if let Some(clip_handle) = fields
                .iter()
                .find(|field| field.code == 340)
                .map(|field| field.value)
                && let Some(boundary) = boundaries.get(clip_handle)
            {
                let _ = viewport.set_clip_boundary(boundary.clone());
                let _ = viewport.set_clip_enabled(
                    fields
                        .iter()
                        .find(|field| field.code == 90)
                        .and_then(|field| field.value.parse::<i64>().ok())
                        .is_some_and(|flags| flags & 65_536 != 0),
                );
            }
            let previous = layouts.active().name().to_owned();
            if layouts.set_active(&target).is_ok() {
                layouts.active_mut().add_viewport(viewport);
                let _ = layouts.set_active(&previous);
            }
        }
    }
}

fn serialize_viewports(
    layouts: &crate::LayoutCollection,
    _source: &str,
    handle: &mut u64,
    newline: &str,
) -> String {
    let mut output = String::new();
    for name in layouts.names().filter(|name| *name != "Model") {
        let Some(layout) = layouts.get(name) else {
            continue;
        };
        for viewport in layout.viewports() {
            let world = viewport.world();
            let paper = viewport.paper();
            let width = paper[2] - paper[0];
            let height = paper[3] - paper[1];
            let world_height = world[3] - world[1];
            let paper_cx = (paper[0] + paper[2]) / 2.0;
            let paper_cy = (paper[1] + paper[3]) / 2.0;
            let world_cx = (world[0] + world[2]) / 2.0;
            let world_cy = (world[1] + world[3]) / 2.0;
            let clip_handle = viewport
                .clip_boundary()
                .filter(|points| points.len() >= 3)
                .map(|_| {
                    let value = *handle;
                    *handle = handle.saturating_add(1);
                    value
                });
            if let Some(clip_handle) = clip_handle {
                output.push_str(&format!(
                    "0{newline}LWPOLYLINE{newline}5{newline}{clip_handle:X}{newline}100{newline}AcDbEntity{newline}8{newline}0{newline}67{newline}1{newline}410{newline}{name}{newline}100{newline}AcDbPolyline{newline}90{newline}{}{newline}70{newline}1{newline}",
                    viewport.clip_boundary().map_or(0, |points| points.len())
                ));
                for point in viewport.clip_boundary().unwrap_or_default() {
                    output.push_str(&format!(
                        "10{newline}{}{newline}20{newline}{}{newline}",
                        point.x(),
                        point.y()
                    ));
                }
            }
            let clip_flags = if viewport.clip_enabled() { 65_536 } else { 0 };
            let clip_fields = clip_handle.map_or_else(
                || format!("90{newline}{clip_flags}{newline}"),
                |value| format!("90{newline}{clip_flags}{newline}340{newline}{value:X}{newline}"),
            );
            output.push_str(&format!(
                "0{newline}VIEWPORT{newline}5{newline}{:X}{newline}100{newline}AcDbEntity{newline}8{newline}0{newline}67{newline}1{newline}410{newline}{name}{newline}100{newline}AcDbViewport{newline}10{newline}{paper_cx}{newline}20{newline}{paper_cy}{newline}40{newline}{width}{newline}41{newline}{height}{newline}12{newline}{world_cx}{newline}22{newline}{world_cy}{newline}45{newline}{world_height}{newline}{clip_fields}69{newline}{handle}{newline}",
                *handle,
                clip_fields = clip_fields,
            ));
            *handle = handle.saturating_add(1);
        }
    }
    output
}

fn viewport_clip_handles(input: &str) -> HashSet<String> {
    let mut groups = Groups::new(input);
    let mut result = HashSet::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 0
            && marker.value == "VIEWPORT"
            && let Ok(fields) = record_fields(&mut groups)
            && let Some(handle) = fields
                .iter()
                .find(|field| field.code == 340)
                .map(|field| field.value.to_owned())
        {
            result.insert(handle);
        }
    }
    result
}

fn viewport_clip_boundaries(input: &str) -> HashMap<String, Vec<Point3>> {
    let wanted = viewport_clip_handles(input);
    let mut groups = Groups::new(input);
    let mut result = HashMap::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code != 0 || !matches!(marker.value, "POLYLINE" | "LWPOLYLINE") {
            continue;
        }
        let Ok(header) = record_fields(&mut groups) else {
            break;
        };
        let Some(handle) = header
            .iter()
            .find(|field| field.code == 5)
            .map(|field| field.value.to_owned())
        else {
            continue;
        };
        if !wanted.contains(&handle) {
            continue;
        }
        if marker.value == "LWPOLYLINE" {
            let mut points = Vec::new();
            let mut x = None;
            for field in header {
                match field.code {
                    10 => x = field.value.parse::<f64>().ok(),
                    20 => {
                        if let Some(x) = x.take()
                            && let Some(y) = field.value.parse::<f64>().ok()
                            && let Ok(point) = Point3::new(x, y, 0.0)
                        {
                            points.push(point);
                        }
                    }
                    _ => {}
                }
            }
            if points.len() >= 3 {
                result.insert(handle, points);
            }
            continue;
        }
        let mut points = Vec::new();
        while let Ok(Some(child)) = groups.next() {
            if child.code != 0 {
                continue;
            }
            if child.value == "SEQEND" {
                let _ = record_fields(&mut groups);
                break;
            }
            if child.value != "VERTEX" {
                break;
            }
            let Ok(fields) = record_fields(&mut groups) else {
                break;
            };
            let Some(x) = fields
                .iter()
                .find(|field| field.code == 10)
                .and_then(|field| field.value.parse::<f64>().ok())
            else {
                continue;
            };
            let Some(y) = fields
                .iter()
                .find(|field| field.code == 20)
                .and_then(|field| field.value.parse::<f64>().ok())
            else {
                continue;
            };
            let z = fields
                .iter()
                .find(|field| field.code == 30)
                .and_then(|field| field.value.parse::<f64>().ok())
                .unwrap_or(0.0);
            if let Ok(point) = Point3::new(x, y, z) {
                points.push(point);
            }
        }
        if points.len() >= 3 {
            result.insert(handle, points);
        }
    }
    result
}

fn record_handle_at(input: &str, start: usize) -> Option<String> {
    let mut groups = Groups::new(input.get(start..)?);
    let _ = groups.next().ok()??;
    record_fields(&mut groups)
        .ok()?
        .iter()
        .find(|field| field.code == 5)
        .map(|field| field.value.to_owned())
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
        "POINT" => &[10, 20, 30],
        "LINE" => &[10, 20, 30, 11, 21, 31],
        "XLINE" | "RAY" => &[10, 20, 30, 11, 21, 31],
        "CIRCLE" => &[10, 20, 30, 40],
        "ARC" => &[10, 20, 30, 40, 50, 51],
        "ELLIPSE" => &[10, 20, 30, 11, 21, 31, 40, 41, 42],
        "3DFACE" => &[10, 20, 30, 11, 21, 31, 12, 22, 32, 13, 23, 33, 70],
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
        "POINT" => Entity::Point(crate::PointEntity::new(point(0))),
        "LINE" => Entity::Line(Line3::new(point(0), point(3))),
        "XLINE" => {
            Entity::XLine(
                XLine3::from_points(point(0), point(3)).map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(11),
                })?,
            )
        }
        "RAY" => Entity::Ray(Ray3::from_points(point(0), point(3)).map_err(|_| DxfError {
            line: marker.line,
            kind: DxfErrorKind::InvalidCoordinate(11),
        })?),
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
        "ELLIPSE" => Entity::Ellipse(
            crate::Ellipse3::new(
                point(0),
                point(3),
                fields[6].1.value,
                fields[7].1.value,
                fields[8].1.value,
            )
            .map_err(|_| DxfError {
                line: marker.line,
                kind: DxfErrorKind::InvalidCoordinate(40),
            })?,
        ),
        "3DFACE" => {
            let vertices = [point(0), point(3), point(6), point(9)];
            let flags = fields
                .iter()
                .find(|(code, _)| *code == 70)
                .map_or(0u8, |(_, field)| field.value as u8);
            let count = if vertices[2] == vertices[3] { 3 } else { 4 };
            Entity::Face3(
                Face3::new(
                    vertices,
                    count,
                    [
                        flags & 1 != 0,
                        flags & 2 != 0,
                        flags & 4 != 0,
                        flags & 8 != 0,
                    ],
                )
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(10),
                })?,
            )
        }
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

fn field_number(fields: &[Group<'_>], code: u16, line: usize) -> Result<Option<f64>, DxfError> {
    unique_field(fields, code)?
        .map(read_number)
        .transpose()
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::InvalidCoordinate(code),
        })
}

fn field_text(fields: &[Group<'_>], code: u16) -> Result<Option<String>, DxfError> {
    Ok(fields
        .iter()
        .find(|g| g.code == code)
        .map(|g| g.value.to_owned()))
}

fn point_fields(
    fields: &[Group<'_>],
    x: u16,
    y: u16,
    z: u16,
    line: usize,
) -> Result<Point3, DxfError> {
    let x_code = x;
    let x = field_number(fields, x_code, line)?.ok_or(DxfError {
        line,
        kind: DxfErrorKind::MissingField(x_code),
    })?;
    let y = field_number(fields, y, line)?.ok_or(DxfError {
        line,
        kind: DxfErrorKind::MissingField(y),
    })?;
    let z = field_number(fields, z, line)?.unwrap_or(0.0);
    Point3::new(x, y, z).map_err(|_| DxfError {
        line,
        kind: DxfErrorKind::InvalidCoordinate(x_code),
    })
}

fn extended_record(
    entity: &Entity,
    marker: Group<'_>,
    properties: EntityProperties,
    end: usize,
) -> EntityRecord {
    EntityRecord {
        original: entity.clone(),
        source_line: marker.line,
        start: marker.start,
        end,
        fields_end: end,
        fields: Vec::new(),
        properties,
        polyline: None,
    }
}

fn read_insert_entity(
    groups: &mut Groups<'_>,
    marker: Group<'_>,
    handles: &mut HashSet<u64>,
) -> Result<(Entity, EntityRecord), DxfError> {
    let (mut entity, mut record) =
        read_extended_entity(groups, marker, handles, &[], &[], &HashMap::new(), &[], &[])?;
    let Entity::Insert(ref mut insert) = entity else {
        return Ok((entity, record));
    };
    let mut attributes = Vec::new();
    loop {
        let Some(next) = groups.peek()? else {
            break;
        };
        if next.code != 0 {
            let _ = groups.next()?;
            continue;
        }
        if next.value == "ATTRIB" {
            let marker = groups.next()?.expect("peeked ATTRIB");
            let fields = record_fields(groups)?;
            let tag = field_text(&fields, 2)?.unwrap_or_default();
            let value = field_text(&fields, 1)?.unwrap_or_default();
            let position = point_fields(&fields, 10, 20, 30, marker.line).ok();
            let attribute = match position {
                Some(p) => crate::Attribute::new(tag, value).with_position(p),
                None => crate::Attribute::new(tag, value),
            };
            attributes.push(attribute);
        } else if next.value == "SEQEND" {
            let _ = groups.next()?;
            let _ = record_fields(groups)?;
            break;
        } else {
            break;
        }
    }
    insert.set_attributes(attributes);
    record.end = groups.peek()?.map_or(groups.offset, |group| group.start);
    Ok((entity, record))
}

/// Read modern entities while retaining their complete source record.  Their source bytes
/// remain authoritative when an imported entity is written back; newly created entities are
/// emitted by `write_entity` below.
#[allow(clippy::too_many_arguments)]
fn read_extended_entity(
    groups: &mut Groups<'_>,
    marker: Group<'_>,
    handles: &mut HashSet<u64>,
    mline_styles: &[MLineStyle],
    mleader_styles: &[MLeaderStyle],
    mleader_style_handles: &HashMap<u64, String>,
    dimension_styles: &[DimensionStyle],
    block_definitions: &[BlockDefinition],
) -> Result<(Entity, EntityRecord), DxfError> {
    let source = record_fields(groups)?;
    let properties = read_entity_properties(&source, marker.line, handles)?;
    let end = groups.peek()?.map_or(groups.offset, |g| g.start);
    let point = |x, y, z| point_fields(&source, x, y, z, marker.line);
    let entity =
        match marker.value {
            "TEXT" => {
                let mut text = Text::new(
                    field_text(&source, 1)?.unwrap_or_default(),
                    point(10, 20, 30)?,
                    field_number(&source, 40, marker.line)?.unwrap_or(1.0),
                )
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(40),
                })?;
                if let Some(v) = field_number(&source, 50, marker.line)? {
                    text.set_rotation(v.to_radians()).map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(50),
                    })?;
                }
                if let Some(v) = field_number(&source, 41, marker.line)? {
                    text.set_width_factor(v).map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(41),
                    })?;
                }
                if let Some(style) = field_text(&source, 7)? {
                    text.set_style_name(style).map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(7),
                    })?;
                }
                Entity::Text(text)
            }
            "MTEXT" => {
                let mut text = MText::new(
                    field_text(&source, 1)?.unwrap_or_default(),
                    point(10, 20, 30)?,
                    field_number(&source, 40, marker.line)?.unwrap_or(1.0),
                    field_number(&source, 41, marker.line)?.unwrap_or(1.0),
                )
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(40),
                })?;
                if let Some(v) = field_number(&source, 50, marker.line)? {
                    text.text_mut()
                        .set_rotation(v.to_radians())
                        .map_err(|_| DxfError {
                            line: marker.line,
                            kind: DxfErrorKind::InvalidCoordinate(50),
                        })?;
                }
                if let Some(style) = field_text(&source, 7)? {
                    text.text_mut()
                        .set_style_name(style)
                        .map_err(|_| DxfError {
                            line: marker.line,
                            kind: DxfErrorKind::InvalidCoordinate(7),
                        })?;
                }
                Entity::MText(text)
            }
            "LEADER" | "MULTILEADER" => {
                if marker.value == "MULTILEADER" {
                    return read_mleader_fields(
                        &source,
                        marker,
                        properties,
                        end,
                        mleader_styles,
                        mleader_style_handles,
                        block_definitions,
                    );
                }
                let xs: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 10)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let ys: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 20)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let zs: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 30)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                if marker.value == "MULTILEADER" && xs.len() == 1 {
                    let x2 = source
                        .iter()
                        .find(|g| g.code == 12)
                        .map(|g| read_number(*g))
                        .transpose()?
                        .unwrap_or(xs[0]);
                    let y2 = source
                        .iter()
                        .find(|g| g.code == 22)
                        .map(|g| read_number(*g))
                        .transpose()?
                        .unwrap_or(ys[0]);
                    let mut xs = xs;
                    let mut ys = ys;
                    xs.push(x2);
                    ys.push(y2);
                    let zs = source
                        .iter()
                        .filter(|g| g.code == 30)
                        .map(|g| read_number(*g))
                        .collect::<Result<Vec<_>, _>>()?;
                    let vertices = xs
                        .into_iter()
                        .zip(ys)
                        .enumerate()
                        .map(|(i, (x, y))| Point3::new(x, y, zs.get(i).copied().unwrap_or(0.0)))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|_| DxfError {
                            line: marker.line,
                            kind: DxfErrorKind::InvalidCoordinate(10),
                        })?;
                    let mut leader = Leader::new(vertices).map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::MissingField(10),
                    })?;
                    leader.set_annotation(field_text(&source, 304)?.or(field_text(&source, 3)?));
                    return Ok((
                        Entity::Leader(leader.clone()),
                        extended_record(&Entity::Leader(leader), marker, properties, end),
                    ));
                }
                if xs.len() < 2 || xs.len() != ys.len() {
                    return Err(DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::MissingField(10),
                    });
                }
                let vertices = xs
                    .into_iter()
                    .zip(ys)
                    .enumerate()
                    .map(|(i, (x, y))| Point3::new(x, y, zs.get(i).copied().unwrap_or(0.0)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(10),
                    })?;
                let mut leader = Leader::new(vertices).map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::MissingField(10),
                })?;
                leader.set_annotation(field_text(&source, 3)?);
                Entity::Leader(leader)
            }
            "MLINE" => {
                let xs = source
                    .iter()
                    .filter(|field| field.code == 10)
                    .map(|field| read_number(*field))
                    .collect::<Result<Vec<_>, _>>()?;
                let ys = source
                    .iter()
                    .filter(|field| field.code == 20)
                    .map(|field| read_number(*field))
                    .collect::<Result<Vec<_>, _>>()?;
                let zs = source
                    .iter()
                    .filter(|field| field.code == 30)
                    .map(|field| read_number(*field))
                    .collect::<Result<Vec<_>, _>>()?;
                if xs.len() < 2 || xs.len() != ys.len() {
                    return Err(DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::MissingField(10),
                    });
                }
                let vertices = xs
                    .into_iter()
                    .zip(ys)
                    .enumerate()
                    .map(|(index, (x, y))| Point3::new(x, y, zs.get(index).copied().unwrap_or(0.0)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(10),
                    })?;
                let style_name = field_text(&source, 2)?.unwrap_or_else(|| "Standard".into());
                let style = mline_styles
                    .iter()
                    .find(|style| style.name().eq_ignore_ascii_case(&style_name))
                    .cloned()
                    .map_or_else(|| MLineStyle::new(style_name, vec![-0.5, 0.5]), Ok)
                    .map_err(|error| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::Corner(error),
                    })?;
                let mut mline = MLine2::new(vertices, style).map_err(|error| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::Corner(error),
                })?;
                if let Some(scale) = field_number(&source, 40, marker.line)? {
                    mline.set_scale(scale).map_err(|error| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::Corner(error),
                    })?;
                }
                if let Some(flags) = field_number(&source, 70, marker.line)? {
                    mline
                        .set_closed(flags as u8 & 1 != 0)
                        .map_err(|error| DxfError {
                            line: marker.line,
                            kind: DxfErrorKind::Corner(error),
                        })?;
                }
                if let Some(justification) = field_number(&source, 71, marker.line)? {
                    mline.set_justification(match justification as i32 {
                        0 => MLineJustification::Top,
                        1 => MLineJustification::Zero,
                        2 => MLineJustification::Bottom,
                        _ => MLineJustification::Zero,
                    });
                }
                Entity::MLine(mline)
            }
            "MESH" => Entity::Mesh(read_mesh_fields(&source, marker.line)?),
            "SPLINE" => {
                let degree = field_number(&source, 71, marker.line)?.unwrap_or(1.0) as usize;
                let knots = source
                    .iter()
                    .filter(|g| g.code == 40)
                    .map(|g| read_number(*g))
                    .collect::<Result<Vec<_>, _>>()?;
                let xs: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 10)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let ys: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 20)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let zs: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 30)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let weights = source
                    .iter()
                    .filter(|g| g.code == 41)
                    .map(|g| read_number(*g))
                    .collect::<Result<Vec<_>, _>>()?;
                let points = xs
                    .into_iter()
                    .zip(ys)
                    .enumerate()
                    .map(|(i, (x, y))| Point3::new(x, y, zs.get(i).copied().unwrap_or(0.0)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(10),
                    })?;
                let weights = if weights.is_empty() {
                    vec![1.0; points.len()]
                } else {
                    weights
                };
                Entity::Spline(
                    Spline3::new(
                        degree,
                        knots,
                        points,
                        weights,
                        field_number(&source, 70, marker.line)?.unwrap_or(0.0) as i32 & 1 != 0,
                    )
                    .map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(71),
                    })?,
                )
            }
            "TABLE" | "ACAD_TABLE" => {
                let anchor = point(10, 20, 30)?;
                let rows = field_number(&source, 91, marker.line)?
                    .unwrap_or(1.0)
                    .max(1.0) as usize;
                let cols = field_number(&source, 92, marker.line)?
                    .unwrap_or(1.0)
                    .max(1.0) as usize;
                let heights = source
                    .iter()
                    .filter(|g| g.code == 40)
                    .map(|g| read_number(*g))
                    .collect::<Result<Vec<_>, _>>()?;
                let widths = source
                    .iter()
                    .filter(|g| g.code == 41)
                    .map(|g| read_number(*g))
                    .collect::<Result<Vec<_>, _>>()?;
                let heights = if heights.len() == rows {
                    heights
                } else {
                    vec![1.0; rows]
                };
                let widths = if widths.len() == cols {
                    widths
                } else {
                    vec![1.0; cols]
                };
                let values = source
                    .iter()
                    .filter(|g| g.code == 1)
                    .map(|g| g.value.to_owned())
                    .collect::<Vec<_>>();
                let cells = (0..rows)
                    .map(|r| {
                        (0..cols)
                            .map(|c| values.get(r * cols + c).cloned().unwrap_or_default())
                            .collect()
                    })
                    .collect();
                Entity::Table(
                    Table::new(anchor, heights, widths, cells).map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(40),
                    })?,
                )
            }
            "DIMENSION" => {
                let first = point(10, 20, 30)?;
                let second = point_fields(&source, 13, 23, 33, marker.line).unwrap_or(first);
                let offset = point_fields(&source, 14, 24, 34, marker.line).unwrap_or(first);
                let kind = match field_number(&source, 70, marker.line)?.unwrap_or(0.0) as i32 & 7 {
                    1 => DimensionKind::Aligned {
                        first,
                        second,
                        offset,
                    },
                    3 => DimensionKind::Angular {
                        center: first,
                        first: second,
                        second: offset,
                    },
                    4 => DimensionKind::Diametric { first, second },
                    2 => DimensionKind::Radial {
                        center: first,
                        definition: second,
                    },
                    _ => DimensionKind::Linear {
                        first,
                        second,
                        offset,
                    },
                };
                let mut d = Dimension::new(kind).map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(10),
                })?;
                d.set_text_override(field_text(&source, 1)?);
                if let Some(style_name) = field_text(&source, 3)? {
                    if let Some(style) = dimension_styles
                        .iter()
                        .find(|style| style.name().eq_ignore_ascii_case(&style_name))
                    {
                        d.apply_style(style).map_err(|_| DxfError {
                            line: marker.line,
                            kind: DxfErrorKind::InvalidCoordinate(3),
                        })?;
                    }
                    d.set_style_name(style_name).map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(3),
                    })?;
                }
                if let Some(v) = field_number(&source, 40, marker.line)? {
                    d.set_text_height(v).ok();
                }
                Entity::Dimension(d)
            }
            "HATCH" => {
                let xs: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 10)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let ys: Vec<f64> = source
                    .iter()
                    .filter(|g| g.code == 20)
                    .map(|g| read_number(*g))
                    .collect::<Result<_, _>>()?;
                let points = xs
                    .into_iter()
                    .zip(ys)
                    .map(|(x, y)| Point3::new(x, y, 0.0))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(10),
                    })?;
                let solid = field_number(&source, 70, marker.line)?.unwrap_or(0.0) != 0.0;
                let pattern = if solid {
                    HatchPattern::Solid
                } else {
                    HatchPattern::Lines {
                        angle: field_number(&source, 52, marker.line)?
                            .unwrap_or(0.0)
                            .to_radians(),
                        spacing: field_number(&source, 41, marker.line)?.unwrap_or(1.0),
                    }
                };
                Entity::Hatch(Hatch::new(vec![points], pattern).map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::MissingField(10),
                })?)
            }
            "IMAGE" => {
                let path = field_text(&source, 1)?
                    .or_else(|| field_text(&source, 340).ok().flatten())
                    .unwrap_or_default();
                let width = field_number(&source, 13, marker.line)?
                    .unwrap_or(1.0)
                    .max(1.0) as u32;
                let height = field_number(&source, 23, marker.line)?
                    .unwrap_or(1.0)
                    .max(1.0) as u32;
                let u_pixel = point(11, 21, 31)?;
                let v_pixel = point(12, 22, 32)?;
                let u = Point3::new(
                    u_pixel.x() * f64::from(width),
                    u_pixel.y() * f64::from(width),
                    u_pixel.z() * f64::from(width),
                )
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(11),
                })?;
                let v = Point3::new(
                    v_pixel.x() * f64::from(height),
                    v_pixel.y() * f64::from(height),
                    v_pixel.z() * f64::from(height),
                )
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(12),
                })?;
                let image = RasterImage::new(path, point(10, 20, 30)?, u, v, width, height)
                    .map_err(|_| DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::InvalidCoordinate(10),
                    })?;
                Entity::Image(image)
            }
            "INSERT" => {
                let mut i = Insert::new(
                    field_text(&source, 2)?.unwrap_or_default(),
                    point(10, 20, 30)?,
                );
                i.set_scale([
                    field_number(&source, 41, marker.line)?.unwrap_or(1.0),
                    field_number(&source, 42, marker.line)?.unwrap_or(1.0),
                    field_number(&source, 43, marker.line)?.unwrap_or(1.0),
                ])
                .map_err(|_| DxfError {
                    line: marker.line,
                    kind: DxfErrorKind::InvalidCoordinate(41),
                })?;
                if let Some(v) = field_number(&source, 50, marker.line)? {
                    i.set_rotation(v.to_radians()).ok();
                }
                Entity::Insert(i)
            }
            _ => unreachable!(),
        };
    Ok((
        entity.clone(),
        extended_record(&entity, marker, properties, end),
    ))
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

fn read_lwpolyline_fields(
    fields: Vec<Group<'_>>,
    marker: Group<'_>,
    end: usize,
    handles: &mut HashSet<u64>,
) -> Result<(Entity, EntityRecord), DxfError> {
    let allowed = [10, 20, 38, 39, 40, 41, 42, 43, 70, 90, 210, 220, 230];
    for field in &fields {
        if !COMMON_ENTITY_CODES.contains(&field.code) && !allowed.contains(&field.code) {
            return Err(DxfError {
                line: field.line,
                kind: DxfErrorKind::UnsupportedProperty(field.code),
            });
        }
        if !matches!(field.code, 10 | 20 | 40 | 41 | 42) {
            unique_field(&fields, field.code)?;
        }
    }
    let properties = read_entity_properties(&fields, marker.line, handles)?;
    let elevation = fields
        .iter()
        .find(|field| field.code == 38)
        .map(|field| read_number(*field))
        .transpose()?
        .unwrap_or(0.0);
    let normal = [210, 220, 230]
        .into_iter()
        .map(|code| {
            fields
                .iter()
                .find(|field| field.code == code)
                .map(|field| read_number(*field))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    if normal[0].unwrap_or(0.0) != 0.0
        || normal[1].unwrap_or(0.0) != 0.0
        || normal[2].unwrap_or(1.0) != 1.0
    {
        return Err(DxfError {
            line: marker.line,
            kind: DxfErrorKind::UnsupportedProperty(210),
        });
    }
    let flags = fields
        .iter()
        .find(|field| field.code == 70)
        .map(|field| read_number(*field))
        .transpose()?
        .unwrap_or(0.0);
    if flags != 0.0 && flags != 1.0 {
        return Err(DxfError {
            line: marker.line,
            kind: DxfErrorKind::UnsupportedProperty(70),
        });
    }
    let count = fields.iter().filter(|field| field.code == 10).count();
    if count < 2
        || fields.iter().filter(|field| field.code == 20).count() != count
        || fields
            .iter()
            .find(|field| field.code == 90)
            .map(|field| read_number(*field))
            .transpose()?
            .is_some_and(|value| value != count as f64)
    {
        return Err(DxfError {
            line: marker.line,
            kind: DxfErrorKind::Expected("matching LWPOLYLINE vertices"),
        });
    }
    let constant_width = fields
        .iter()
        .find(|field| field.code == 43)
        .map(|field| read_width(*field))
        .transpose()?
        .map_or(0.0, |field| field.value);
    let mut vertices = Vec::with_capacity(count);
    let mut bulges = Vec::with_capacity(count);
    let mut widths = Vec::with_capacity(count);
    let mut chunks: Vec<Vec<Group<'_>>> = Vec::with_capacity(count);
    for field in fields
        .iter()
        .copied()
        .filter(|field| matches!(field.code, 10 | 20 | 40 | 41 | 42))
    {
        if field.code == 10 {
            chunks.push(Vec::new());
        }
        if let Some(chunk) = chunks.last_mut() {
            chunk.push(field);
        }
    }
    for chunk in chunks {
        let x = chunk
            .iter()
            .find(|field| field.code == 10)
            .map(|field| read_number(*field))
            .transpose()?
            .ok_or(DxfError {
                line: marker.line,
                kind: DxfErrorKind::MissingField(10),
            })?;
        let y = chunk
            .iter()
            .find(|field| field.code == 20)
            .map(|field| read_number(*field))
            .transpose()?
            .ok_or(DxfError {
                line: marker.line,
                kind: DxfErrorKind::MissingField(20),
            })?;
        vertices.push(Point3::new(x, y, elevation).map_err(|_| DxfError {
            line: marker.line,
            kind: DxfErrorKind::InvalidCoordinate(10),
        })?);
        bulges.push(
            chunk
                .iter()
                .find(|field| field.code == 42)
                .map(|field| read_number(*field))
                .transpose()?
                .unwrap_or(0.0),
        );
        let start = chunk
            .iter()
            .find(|field| field.code == 40)
            .map(|field| read_width(*field))
            .transpose()?
            .map_or(constant_width, |field| field.value);
        let end = chunk
            .iter()
            .find(|field| field.code == 41)
            .map(|field| read_width(*field))
            .transpose()?
            .map_or(constant_width, |field| field.value);
        widths.push(PolylineWidth::new(start, end).map_err(|error| DxfError {
            line: marker.line,
            kind: DxfErrorKind::Corner(error),
        })?);
    }
    let mut polyline =
        Polyline2::with_bulges(vertices, bulges, flags == 1.0).map_err(|error| DxfError {
            line: marker.line,
            kind: DxfErrorKind::Corner(error),
        })?;
    polyline.set_widths(widths).map_err(|error| DxfError {
        line: marker.line,
        kind: DxfErrorKind::Corner(error),
    })?;
    let entity = Entity::Polyline(polyline);
    let record_fields = fields
        .iter()
        .filter_map(|field| coordinate(*field).ok().map(|value| (field.code, value)))
        .collect();
    Ok((
        entity.clone(),
        EntityRecord {
            original: entity,
            source_line: marker.line,
            start: marker.start,
            end,
            fields_end: end,
            fields: record_fields,
            properties,
            polyline: None,
        },
    ))
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
    if unique_field(&header, 70)?
        .map(read_number)
        .transpose()?
        .is_some_and(|flags| flags as u32 & 8 != 0)
    {
        return read_polyline3(&header, groups, marker, handles);
    }
    validate_polyline_fields(
        &header,
        &[10, 20, 30, 39, 40, 41, 66, 70, 100, 210, 220, 230],
    )?;
    let properties = read_entity_properties(&header, marker.line, handles)?;
    let mut fields = [10, 20, 30]
        .into_iter()
        .map(|code| Ok((code, required_coordinate(&header, code, marker.line)?)))
        .collect::<Result<Vec<_>, DxfError>>()?;
    require_zero(&header, &[10, 20, 39])?;
    let mut defaults = [0.0; 2];
    for (index, code) in [40, 41].into_iter().enumerate() {
        if let Some(field) = unique_field(&header, code)? {
            let value = read_width(field)?;
            defaults[index] = value.value;
            fields.push((code, value));
        }
    }
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
    let mut widths = Vec::new();
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
            require_zero(&source_fields, &[70])?;
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
            let mut width = defaults;
            for (index, code) in [40, 41].into_iter().enumerate() {
                if let Some(field) = unique_field(&source_fields, code)? {
                    let value = read_width(field)?;
                    width[index] = value.value;
                    fields.push((code, value));
                }
            }
            widths.push(PolylineWidth::new(width[0], width[1]).expect("validated widths"));
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
    let mut polyline = Polyline2::with_bulges(
        vertices,
        bulges,
        flags.is_some_and(|field| field.value == 1.0),
    )
    .map_err(|_| DxfError {
        line: marker.line,
        kind: DxfErrorKind::Expected("at least two POLYLINE vertices"),
    })?;
    polyline
        .set_widths(widths)
        .expect("one width pair per vertex");
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

fn read_polyline3(
    header: &[Group<'_>],
    groups: &mut Groups<'_>,
    marker: Group<'_>,
    handles: &mut HashSet<u64>,
) -> Result<(Entity, EntityRecord), DxfError> {
    validate_polyline_fields(header, &[10, 20, 30, 66, 70, 100, 210, 220, 230])?;
    let properties = read_entity_properties(header, marker.line, handles)?;
    let flags = unique_field(header, 70)?
        .map(read_number)
        .transpose()?
        .unwrap_or(8.0) as u32;
    if flags & 8 == 0 || flags & !(1 | 8) != 0 {
        return Err(DxfError {
            line: marker.line,
            kind: DxfErrorKind::UnsupportedProperty(70),
        });
    }
    let mut vertices = Vec::new();
    let fields_end = groups.peek()?.map_or(groups.offset, |group| group.start);
    loop {
        let child = groups.required()?;
        if child.code != 0 || !matches!(child.value, "VERTEX" | "SEQEND") {
            return Err(DxfError {
                line: child.line,
                kind: DxfErrorKind::Expected("VERTEX or SEQEND in 3D POLYLINE"),
            });
        }
        let source = record_fields(groups)?;
        if child.value == "SEQEND" {
            let end = groups.peek()?.map_or(groups.offset, |group| group.start);
            let entity =
                Entity::Polyline3(Polyline3::new(vertices, flags & 1 != 0).map_err(|error| {
                    DxfError {
                        line: marker.line,
                        kind: DxfErrorKind::Corner(error),
                    }
                })?);
            return Ok((
                entity.clone(),
                EntityRecord {
                    original: entity,
                    source_line: marker.line,
                    start: marker.start,
                    end,
                    fields_end,
                    fields: header
                        .iter()
                        .filter_map(|field| {
                            coordinate(*field).ok().map(|value| (field.code, value))
                        })
                        .collect(),
                    properties,
                    polyline: None,
                },
            ));
        }
        validate_polyline_fields(&source, &[10, 20, 30, 70, 100])?;
        let vertex_flags = unique_field(&source, 70)?
            .map(read_number)
            .transpose()?
            .unwrap_or(32.0) as u32;
        if vertex_flags & 32 == 0 || vertex_flags & !32 != 0 {
            return Err(DxfError {
                line: child.line,
                kind: DxfErrorKind::UnsupportedProperty(70),
            });
        }
        let point = Point3::new(
            required_coordinate(&source, 10, child.line)?.value,
            required_coordinate(&source, 20, child.line)?.value,
            required_coordinate(&source, 30, child.line)?.value,
        )
        .map_err(|_| DxfError {
            line: child.line,
            kind: DxfErrorKind::InvalidCoordinate(10),
        })?;
        vertices.push(point);
    }
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

fn read_width(group: Group<'_>) -> Result<Coordinate, DxfError> {
    let field = coordinate(group)?;
    if field.value < 0.0 {
        return Err(DxfError {
            line: group.line,
            kind: DxfErrorKind::InvalidCoordinate(group.code),
        });
    }
    Ok(field)
}

fn serialize_block(block: &BlockDefinition, newline: &str) -> String {
    let base = block.base();
    let flags = if block.xref_path().is_some() {
        if block.xref_overlay() { 8 } else { 4 }
    } else {
        0
    };
    let mut output = format!(
        "0{n}BLOCK{n}2{n}{name}{n}5{n}{handle:X}{n}70{n}{flags}{n}10{n}{x}{n}20{n}{y}{n}30{n}{z}{n}",
        n = newline,
        name = block.name(),
        handle = block_handle(block.name()),
        flags = flags,
        x = base.x(),
        y = base.y(),
        z = base.z()
    );
    if let Some(path) = block.xref_path() {
        output.push_str(&format!("1{n}{path}{n}", n = newline));
    }
    for entity in block.entities() {
        write_entity(&mut output, entity, "0", &EntityStyle::default());
    }
    for definition in block.attribute_definitions() {
        output.push_str(&format!("0{n}ATTDEF{n}2{n}{tag}{n}3{n}{prompt}{n}1{n}{default_value}{n}10{n}{x}{n}20{n}{y}{n}30{n}{z}{n}", n = newline, tag = definition.tag(), prompt = definition.prompt(), default_value = definition.default_value(), x = definition.position().x(), y = definition.position().y(), z = definition.position().z()));
    }
    output.push_str(&format!("0{n}ENDBLK{n}", n = newline));
    output
}

fn write_entity(output: &mut String, entity: &Entity, layer: &str, style: &EntityStyle) {
    let entity_name = if matches!(entity, Entity::Table(_)) {
        "ACAD_TABLE"
    } else {
        entity.name()
    };
    write!(output, "0\n{}\n8\n{layer}\n", entity_name).expect("String write");
    write_style(output, style);
    let codes: &[u16] = match entity {
        Entity::Point(_) => &[10, 20, 30],
        Entity::Line(_) => &[10, 20, 30, 11, 21, 31],
        Entity::Circle(_) => &[10, 20, 30, 40],
        Entity::Arc(_) => &[10, 20, 30, 40, 50, 51],
        Entity::Ellipse(_) => &[10, 20, 30, 11, 21, 31, 40, 41, 42],
        Entity::Face3(face) => {
            let (vertices, count) = face.vertices();
            let flags = face
                .invisible_edges()
                .iter()
                .enumerate()
                .map(|(i, value)| if *value { 1u8 << i } else { 0 })
                .sum::<u8>();
            for index in 0..4 {
                let point = vertices[index.min(count.saturating_sub(1))];
                let x_code = 10 + index as u16;
                let y_code = 20 + index as u16;
                let z_code = 30 + index as u16;
                writeln!(
                    output,
                    "{x_code}\n{}\n{y_code}\n{}\n{z_code}\n{}",
                    point.x(),
                    point.y(),
                    point.z()
                )
                .expect("String write");
            }
            writeln!(output, "70\n{flags}").expect("String write");
            return;
        }
        Entity::Mesh(mesh) => {
            writeln!(output, "100\nAcDbEntity").expect("String write");
            let mut vertices = Vec::new();
            let mut face_lists = Vec::new();
            for face in mesh.faces().iter().take(4096) {
                let (points, count) = face.vertices();
                let start = vertices.len();
                vertices.extend_from_slice(&points[..count]);
                let mut indices = Vec::with_capacity(count + 1);
                indices.push(count.to_string());
                for index in 0..count {
                    let value = start + index;
                    indices.push(value.to_string());
                }
                face_lists.extend(indices);
            }
            writeln!(
                output,
                "100\nAcDbSubDMesh\n71\n2\n72\n0\n91\n0\n92\n{}",
                vertices.len()
            )
            .expect("String write");
            for point in vertices {
                writeln!(
                    output,
                    "10\n{}\n20\n{}\n30\n{}",
                    point.x(),
                    point.y(),
                    point.z()
                )
                .expect("String write");
            }
            writeln!(output, "93\n{}", face_lists.len()).expect("String write");
            for value in face_lists {
                writeln!(output, "90\n{value}").expect("String write");
            }
            writeln!(output, "94\n0\n95\n0\n90\n0").expect("String write");
            return;
        }
        Entity::MLine(mline) => {
            let flags = u8::from(mline.is_closed());
            let justification = match mline.justification() {
                MLineJustification::Top => 0,
                MLineJustification::Zero => 1,
                MLineJustification::Bottom => 2,
            };
            writeln!(
                output,
                "2\n{}\n70\n{flags}\n71\n{justification}\n72\n{}\n40\n{}",
                mline.style().name(),
                mline.vertices().len(),
                mline.scale()
            )
            .expect("String write");
            for point in mline.vertices() {
                writeln!(
                    output,
                    "10\n{}\n20\n{}\n30\n{}",
                    point.x(),
                    point.y(),
                    point.z()
                )
                .expect("String write");
            }
            return;
        }
        Entity::Text(text) => {
            let p = text.location();
            writeln!(
                output,
                "10\n{}\n20\n{}\n30\n{}\n40\n{}\n1\n{}\n7\n{}\n50\n{}\n41\n{}",
                p.x(),
                p.y(),
                p.z(),
                text.height(),
                text.content(),
                text.style_name(),
                text.rotation().to_degrees(),
                text.width_factor()
            )
            .expect("String write");
            return;
        }
        Entity::XLine(line) => {
            let origin = line.origin();
            let end = line
                .point_at(1.0)
                .expect("unit construction direction is finite");
            writeln!(
                output,
                "10\n{}\n20\n{}\n30\n{}\n11\n{}\n21\n{}\n31\n{}",
                origin.x(),
                origin.y(),
                origin.z(),
                end.x(),
                end.y(),
                end.z()
            )
            .expect("String write");
            return;
        }
        Entity::Ray(ray) => {
            let origin = ray.origin();
            let end = ray
                .point_at(1.0)
                .expect("unit construction direction is finite");
            writeln!(
                output,
                "10\n{}\n20\n{}\n30\n{}\n11\n{}\n21\n{}\n31\n{}",
                origin.x(),
                origin.y(),
                origin.z(),
                end.x(),
                end.y(),
                end.z()
            )
            .expect("String write");
            return;
        }
        Entity::MText(text) => {
            let p = text.text().location();
            writeln!(
                output,
                "10\n{}\n20\n{}\n30\n{}\n40\n{}\n41\n{}\n1\n{}\n7\n{}\n50\n{}",
                p.x(),
                p.y(),
                p.z(),
                text.text().height(),
                text.width(),
                text.text().content(),
                text.text().style_name(),
                text.text().rotation().to_degrees()
            )
            .expect("String write");
            return;
        }
        Entity::Leader(leader) => {
            writeln!(output, "76\n0\n71\n0\n").expect("String write");
            for p in leader.vertices() {
                writeln!(output, "10\n{}\n20\n{}\n30\n{}", p.x(), p.y(), p.z())
                    .expect("String write");
            }
            if let Some(text) = leader.annotation() {
                writeln!(output, "3\n{text}").expect("String write");
            }
            return;
        }
        Entity::MLeader(mleader) => {
            writeln!(
                output,
                "100\nAcDbMLeader\n340\n{:X}\n",
                mleader_style_handle(mleader.style_name())
            )
            .expect("String write");
            if let MLeaderContent::Block(insert) = mleader.content() {
                let p = insert.insertion();
                let scale = insert.scale();
                writeln!(
                    output,
                    "296\n1\n341\n{:X}\n15\n{}\n25\n{}\n35\n{}\n16\n{}\n46\n{}",
                    block_handle(insert.block_name()),
                    p.x(),
                    p.y(),
                    p.z(),
                    scale[0],
                    insert.rotation()
                )
                .expect("String write");
                if scale
                    .iter()
                    .any(|value| (*value - scale[0]).abs() > f64::EPSILON)
                {
                    let matrix = [
                        scale[0], 0.0, 0.0, 0.0, 0.0, scale[1], 0.0, 0.0, 0.0, 0.0, scale[2], 0.0,
                        0.0, 0.0, 0.0, 1.0,
                    ];
                    for value in matrix {
                        writeln!(output, "47\n{value}").expect("String write");
                    }
                }
            }
            for branch in mleader.branches() {
                writeln!(output, "302\nCONTEXT_DATA\n290\n1\n").expect("String write");
                for point in branch.vertices() {
                    writeln!(
                        output,
                        "10\n{}\n20\n{}\n30\n{}",
                        point.x(),
                        point.y(),
                        point.z()
                    )
                    .expect("String write");
                }
                writeln!(output, "303\nCONTEXT_DATA\n").expect("String write");
            }
            if let MLeaderContent::MText(text) = mleader.content() {
                writeln!(output, "304\n{}", text.text().content()).expect("String write");
            }
            return;
        }
        Entity::Spline(spline) => {
            writeln!(
                output,
                "70\n{}\n71\n{}\n72\n{}\n73\n{}",
                u8::from(spline.is_closed()),
                spline.degree(),
                spline.knots().len(),
                spline.control_points().len()
            )
            .expect("String write");
            for knot in spline.knots() {
                writeln!(output, "40\n{knot}").expect("String write");
            }
            for weight in spline.weights() {
                writeln!(output, "41\n{weight}").expect("String write");
            }
            for p in spline.control_points() {
                writeln!(output, "10\n{}\n20\n{}\n30\n{}", p.x(), p.y(), p.z())
                    .expect("String write");
            }
            return;
        }
        Entity::Table(table) => {
            let p = table.anchor();
            writeln!(
                output,
                "2\nACAD_TABLE\n10\n{}\n20\n{}\n30\n{}\n91\n{}\n92\n{}",
                p.x(),
                p.y(),
                p.z(),
                table.row_heights().len(),
                table.column_widths().len()
            )
            .expect("String write");
            for h in table.row_heights() {
                writeln!(output, "40\n{h}").expect("String write");
            }
            for w in table.column_widths() {
                writeln!(output, "41\n{w}").expect("String write");
            }
            for row in table.cells() {
                for cell in row {
                    writeln!(output, "1\n{cell}").expect("String write");
                }
            }
            return;
        }
        Entity::Dimension(dimension) => {
            let kind = dimension.kind();
            let (first, second, offset, flags) = match kind {
                DimensionKind::Linear {
                    first,
                    second,
                    offset,
                } => (first, second, offset, 0),
                DimensionKind::Aligned {
                    first,
                    second,
                    offset,
                } => (first, second, offset, 1),
                DimensionKind::Radial { center, definition } => (center, definition, definition, 2),
                DimensionKind::Diametric { first, second } => (first, second, second, 3),
                DimensionKind::Angular {
                    center,
                    first,
                    second,
                } => (center, first, second, 2),
            };
            writeln!(output,"70\n{flags}\n3\n{}\n10\n{}\n20\n{}\n30\n{}\n13\n{}\n23\n{}\n33\n{}\n14\n{}\n24\n{}\n34\n{}",dimension.style_name(),first.x(),first.y(),first.z(),second.x(),second.y(),second.z(),offset.x(),offset.y(),offset.z()).expect("String write");
            return;
        }
        Entity::Hatch(hatch) => {
            let solid = matches!(hatch.pattern(), HatchPattern::Solid);
            writeln!(
                output,
                "70\n{}\n71\n0\n91\n{}",
                u8::from(solid),
                hatch.boundaries().len()
            )
            .expect("String write");
            for boundary in hatch.boundaries() {
                writeln!(output, "92\n1\n73\n1\n93\n{}", boundary.len()).expect("String write");
                for p in boundary {
                    writeln!(output, "10\n{}\n20\n{}", p.x(), p.y()).expect("String write");
                }
            }
            if let HatchPattern::Lines { angle, spacing } = hatch.pattern() {
                writeln!(
                    output,
                    "75\n0\n76\n1\n52\n{}\n41\n{}\n2\nUSER",
                    angle.to_degrees(),
                    spacing
                )
                .expect("String write");
            }
            return;
        }
        Entity::Insert(insert) => {
            let p = insert.insertion();
            let s = insert.scale();
            writeln!(
                output,
                "2\n{}\n10\n{}\n20\n{}\n30\n{}\n41\n{}\n42\n{}\n43\n{}\n50\n{}",
                insert.block_name(),
                p.x(),
                p.y(),
                p.z(),
                s[0],
                s[1],
                s[2],
                insert.rotation().to_degrees()
            )
            .expect("String write");
            if let (Some(rows), Some(cols)) = (insert.rows(), insert.columns()) {
                writeln!(
                    output,
                    "70\n{cols}\n71\n{rows}\n44\n{}\n45\n{}",
                    insert.column_spacing(),
                    insert.row_spacing()
                )
                .expect("String write");
            }
            for attribute in insert.attributes() {
                let p = attribute.position().unwrap_or(insert.insertion());
                writeln!(
                    output,
                    "0\nATTRIB\n8\n{layer}\n10\n{}\n20\n{}\n30\n{}\n2\n{}\n1\n{}",
                    p.x(),
                    p.y(),
                    p.z(),
                    attribute.tag(),
                    attribute.value()
                )
                .expect("String write");
            }
            if !insert.attributes().is_empty() {
                output.push_str("0\nSEQEND\n");
            }
            return;
        }
        Entity::Image(image) => {
            let insertion = image.insertion();
            let u = image.u_vector();
            let v = image.v_vector();
            let (width, height) = image.pixel_dimensions();
            let width_f = f64::from(width.max(1));
            let height_f = f64::from(height.max(1));
            writeln!(output, "10\n{}\n20\n{}\n30\n{}\n11\n{}\n21\n{}\n31\n{}\n12\n{}\n22\n{}\n32\n{}\n13\n{}\n23\n{}\n340\n{}", insertion.x(), insertion.y(), insertion.z(), u.x() / width_f, u.y() / width_f, u.z() / width_f, v.x() / height_f, v.y() / height_f, v.z() / height_f, width, height, image_handle(image)).expect("String write");
            return;
        }
        Entity::Polyline3(polyline) => {
            writeln!(
                output,
                "100\nAcDb3dPolyline\n66\n1\n10\n0\n20\n0\n30\n0\n70\n{}",
                u8::from(polyline.is_closed()) | 8
            )
            .expect("String write");
            for point in polyline.vertices() {
                write_polyline3_vertex(output, *point, layer, style);
            }
            writeln!(output, "0\nSEQEND\n8\n{layer}").expect("String write");
            write_style(output, style);
            return;
        }
        Entity::Polyline(polyline) => {
            writeln!(
                output,
                "66\n1\n10\n0\n20\n0\n30\n{}\n70\n{}",
                polyline.elevation(),
                u8::from(polyline.is_closed())
            )
            .expect("String write");
            for (index, point) in polyline.vertices().iter().enumerate() {
                write_vertex(
                    output,
                    *point,
                    polyline.bulges()[index],
                    polyline.widths()[index],
                    [0.0; 2],
                    layer,
                    style,
                );
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

fn image_handle(image: &RasterImage) -> String {
    let mut hasher = DefaultHasher::new();
    image.path().to_string_lossy().hash(&mut hasher);
    format!(
        "{:X}",
        0xA000_0000_0000_0000_u64 | (hasher.finish() & 0x0FFF_FFFF_FFFF_FFFF)
    )
}

fn serialize_image_def(image: &RasterImage) -> String {
    let (width, height) = image.pixel_dimensions();
    format!(
        "0\nIMAGEDEF\n5\n{}\n100\nAcDbRasterImageDef\n90\n0\n1\n{}\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
        image_handle(image),
        image.path().display(),
        width,
        height,
        image.u_vector().x() / f64::from(width.max(1)),
        image.v_vector().y() / f64::from(height.max(1))
    )
}

fn write_vertex(
    output: &mut String,
    point: Point3,
    bulge: f64,
    width: PolylineWidth,
    defaults: [f64; 2],
    layer: &str,
    style: &EntityStyle,
) {
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
    for (code, value, default) in [
        (40, width.start(), defaults[0]),
        (41, width.end(), defaults[1]),
    ] {
        if value != default {
            writeln!(output, "{code}\n{value}").expect("String write");
        }
    }
}

fn write_polyline3_vertex(output: &mut String, point: Point3, layer: &str, style: &EntityStyle) {
    writeln!(
        output,
        "0\nVERTEX\n8\n{layer}\n100\nAcDb3dPolylineVertex\n10\n{}\n20\n{}\n30\n{}\n70\n32",
        point.x(),
        point.y(),
        point.z()
    )
    .expect("String write");
    write_style(output, style);
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
