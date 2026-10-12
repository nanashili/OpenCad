//! ASCII R12 geometry exchange with preservation of existing document records.
//! Geometry uses finite coordinates and named layers. Other table and block
//! contents are retained without interpreting their contents.

use crate::EntitySpace;
use crate::group::EntityGroup;
use crate::{Anchor, ConstraintError, ConstraintKind, ConstraintRecord, ConstraintSet, LinePair};
use crate::{
    Arc3, BlockDefinition, Camera3, Circle3, CornerParameter, Dimension, DimensionKind,
    DimensionStyle, Entity, EntityStyle, EntityStyleError, Face3, GeometryError, Hatch,
    HatchPattern, Insert, Layer, LayerError, Leader, Line3, Linetype, LinetypeError, MLeader,
    MLeaderBranch, MLeaderContent, MLeaderStyle, MLine2, MLineElement, MLineJustification,
    MLineStyle, MText, Mesh3, PdfUnderlay, Point3, Polyline2, Polyline3, PolylineWidth,
    RasterImage, Ray3, Spline3, Table, Text, TextStyle, XLine3,
};
use crate::{DataLink, FieldExpression, NamedField};
use crate::{
    DynamicAction, DynamicActionAxis, DynamicActionOptions, DynamicBlockDefinition,
    DynamicBlockOverrides, DynamicParameter, DynamicParameterType, DynamicValue,
    DynamicValueComponent, DynamicValueSet, DynamicVisibilityState, LookupAssignment, LookupRow,
};
use crate::{NamedUcs, NamedUcsCatalog, NamedView, NamedViewCatalog};
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

fn entity_space_from_record(
    input: &str,
    record: &EntityRecord,
    line: usize,
) -> Result<EntitySpace, DxfError> {
    let source = input.get(record.start..record.end).ok_or(DxfError {
        line,
        kind: DxfErrorKind::UnexpectedEnd,
    })?;
    let mut groups = Groups::new(source);
    let mut paper_flag = None;
    let mut layout = None;
    let mut entity_started = false;
    while let Some(group) = groups.next()? {
        if group.code == 0 {
            if entity_started {
                break;
            }
            entity_started = true;
            continue;
        }
        match group.code {
            67 => {
                if paper_flag.is_some() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::DuplicateField(67),
                    });
                }
                paper_flag = Some(group.value.parse::<u8>().map_err(|_| DxfError {
                    line: group.line,
                    kind: DxfErrorKind::InvalidCoordinate(67),
                })?);
            }
            410 => {
                if layout.is_some() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::DuplicateField(410),
                    });
                }
                if group.value.trim().is_empty() {
                    return Err(DxfError {
                        line: group.line,
                        kind: DxfErrorKind::InvalidCoordinate(410),
                    });
                }
                layout = Some(group.value.to_owned());
            }
            _ => {}
        }
    }
    match paper_flag.unwrap_or(0) {
        0 => Ok(EntitySpace::Model),
        1 => Ok(EntitySpace::Paper(layout.unwrap_or_default())),
        _ => Err(DxfError {
            line,
            kind: DxfErrorKind::InvalidCoordinate(67),
        }),
    }
}

fn read_entity_spaces(
    input: &str,
    records: &[Option<EntityRecord>],
    layouts: &mut crate::LayoutCollection,
) -> Result<Vec<EntitySpace>, DxfError> {
    let mut fallback = layouts
        .names()
        .find(|name| !name.eq_ignore_ascii_case("Model"))
        .map(|name| name.to_owned());
    let mut spaces = Vec::with_capacity(records.len());
    for record in records {
        let Some(record) = record else {
            return Err(DxfError {
                line: 0,
                kind: DxfErrorKind::Expected("imported entity record"),
            });
        };
        let mut space = entity_space_from_record(input, record, record.source_line)?;
        if let EntitySpace::Paper(name) = &mut space {
            if name.is_empty() {
                if fallback.is_none() {
                    let setup = crate::PageSetup::new(210.0, 297.0).map_err(|_| DxfError {
                        line: record.source_line,
                        kind: DxfErrorKind::InvalidCoordinate(410),
                    })?;
                    layouts.create("Layout1", setup).map_err(|_| DxfError {
                        line: record.source_line,
                        kind: DxfErrorKind::InvalidCoordinate(410),
                    })?;
                    fallback = Some("Layout1".to_owned());
                }
                *name = fallback.clone().ok_or(DxfError {
                    line: record.source_line,
                    kind: DxfErrorKind::InvalidCoordinate(410),
                })?;
            } else if !valid_space_name(name) {
                return Err(DxfError {
                    line: record.source_line,
                    kind: DxfErrorKind::InvalidCoordinate(410),
                });
            } else if let Some(canonical) = layouts
                .names()
                .find(|layout| layout.eq_ignore_ascii_case(name))
                .map(|layout| layout.to_owned())
            {
                *name = canonical;
            } else {
                let setup = crate::PageSetup::new(210.0, 297.0).map_err(|_| DxfError {
                    line: record.source_line,
                    kind: DxfErrorKind::InvalidCoordinate(410),
                })?;
                layouts.create(name.clone(), setup).map_err(|_| DxfError {
                    line: record.source_line,
                    kind: DxfErrorKind::InvalidCoordinate(410),
                })?;
            }
        }
        spaces.push(space);
    }
    Ok(spaces)
}

fn valid_space_name(name: &str) -> bool {
    !name.trim().is_empty()
        && !name.eq_ignore_ascii_case("Model")
        && !name.chars().any(char::is_control)
}

fn canonical_layout_name(layouts: &crate::LayoutCollection, name: &str) -> Option<String> {
    layouts
        .names()
        .find(|candidate| candidate.eq_ignore_ascii_case(name))
        .map(str::to_owned)
}

fn normalized_entity_space(space: EntitySpace, layouts: &crate::LayoutCollection) -> EntitySpace {
    match space {
        EntitySpace::Paper(name) if name.is_empty() => layouts
            .names()
            .find(|layout| !layout.eq_ignore_ascii_case("Model"))
            .map_or_else(
                || EntitySpace::Paper("Layout1".to_owned()),
                |layout| EntitySpace::Paper(layout.to_owned()),
            ),
        space => space,
    }
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
    layout_source_names: Vec<String>,
    removed_layout_names: HashSet<String>,
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
    named_views: NamedViewCatalog,
    named_ucs: NamedUcsCatalog,
    current_ucs_name: String,
    current_ucs: crate::Ucs3,
    active_view: Option<NamedView>,
    entity_handles: Vec<u64>,
    data_links: Vec<DataLink>,
    named_fields: Vec<NamedField>,
    entity_spaces: Vec<EntitySpace>,
}

impl DxfDocument {
    /// Validate supported sections, entity fields, layer membership and handles.
    pub fn parse(input: &str) -> Result<Self, DxfError> {
        let mut groups = Groups::new(input);
        let pdf_definitions = read_pdf_definitions(input);
        let mline_styles = read_mline_styles(input);
        let text_styles = read_text_styles(input);
        let dimension_styles = read_dimension_styles(input);
        let (mleader_styles, mleader_style_handles) = read_mleader_styles(input);
        let current_styles = read_current_style_names(input);
        let mut block_definitions = read_block_definitions(input)?;
        let named_views = read_named_views(input);
        let named_ucs = read_named_ucs(input);
        let (current_ucs_name, current_ucs) = read_current_ucs(input);
        let active_view = read_active_view(input);
        let (data_links, named_fields) = read_data_metadata(input);
        let mut layouts = read_layouts(input);
        let layout_source_names = source_layout_names(input);
        read_page_setup_metadata(input, &mut layouts)?;
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
                let pdf_metadata = read_pdf_metadata(input);
                for entity in &mut entities {
                    if let Entity::Image(image) = entity {
                        let key = image.path().to_string_lossy().to_string();
                        if let Some(path) = image_paths.get(&key) {
                            image.set_path(path.clone());
                        }
                    }
                }
                for (index, entity) in entities.iter_mut().enumerate() {
                    let Entity::PdfUnderlay(underlay) = entity else {
                        continue;
                    };
                    let Some(record) = records.get(index).and_then(Option::as_ref) else {
                        continue;
                    };
                    let Some(definition) = record_field_text_at(input, record.start, 340) else {
                        continue;
                    };
                    let Ok(definition_handle) = u64::from_str_radix(definition.trim(), 16) else {
                        continue;
                    };
                    let metadata_handle = record_handle_at(input, record.start)
                        .and_then(|value| u64::from_str_radix(value.trim(), 16).ok())
                        .unwrap_or(definition_handle);
                    if let Some(metadata) = pdf_metadata.get(&metadata_handle) {
                        apply_pdf_metadata(underlay, *metadata, record.source_line)?;
                    }
                }
                read_dynamic_metadata(input, &mut block_definitions, &mut entities, &records)?;
                let entity_spaces = read_entity_spaces(input, &records, &mut layouts)?;
                if let Some(active) = read_active_layout(input)
                    && let Some(canonical) = canonical_layout_name(&layouts, &active)
                {
                    layouts
                        .set_active(&canonical)
                        .expect("canonical active layout must be present");
                }
                let mut used_handles = HashSet::new();
                let mut next_entity_handle = next_handle(input);
                let mut stable_entity_handles = Vec::with_capacity(records.len());
                for record in &records {
                    let handle = record
                        .as_ref()
                        .and_then(|record| record_handle_at(input, record.start))
                        .and_then(|value| u64::from_str_radix(value.trim(), 16).ok());
                    if let Some(handle) = handle {
                        used_handles.insert(handle);
                        stable_entity_handles.push(handle);
                    } else {
                        while used_handles.contains(&next_entity_handle) {
                            next_entity_handle = next_entity_handle.saturating_add(1);
                        }
                        stable_entity_handles.push(next_entity_handle);
                        used_handles.insert(next_entity_handle);
                        next_entity_handle = next_entity_handle.saturating_add(1);
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
                    layouts,
                    layout_source_names,
                    removed_layout_names: HashSet::new(),
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
                    named_views,
                    named_ucs,
                    current_ucs_name,
                    current_ucs,
                    active_view,
                    entity_handles: stable_entity_handles,
                    data_links,
                    named_fields,
                    entity_spaces,
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
                            | "3DFACE" | "PDFUNDERLAY",
                        ) => {
                            let (entity, record) =
                                read_entity(&mut groups, group, &mut handles, &pdf_definitions)?;
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

    pub fn entity_space(&self, index: usize) -> Option<&EntitySpace> {
        self.entity_spaces.get(index)
    }

    pub fn set_entity_space(&mut self, index: usize, space: EntitySpace) -> Result<bool, DxfError> {
        if index >= self.entities.len() {
            return Err(group_error(67));
        }
        self.validate_entity_space(&space)?;
        let changed = self.entity_spaces[index] != space;
        self.entity_spaces[index] = space;
        Ok(changed)
    }

    fn validate_entity_space(&self, space: &EntitySpace) -> Result<(), DxfError> {
        if let EntitySpace::Paper(name) = space
            && (!valid_space_name(name)
                || self
                    .layouts
                    .names()
                    .all(|layout| !layout.eq_ignore_ascii_case(name)))
        {
            return Err(group_error(410));
        }
        Ok(())
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
    pub fn named_views(&self) -> &NamedViewCatalog {
        &self.named_views
    }
    pub fn set_named_views(&mut self, views: NamedViewCatalog) {
        self.named_views = views;
    }
    pub fn named_ucs(&self) -> &NamedUcsCatalog {
        &self.named_ucs
    }
    pub fn set_named_ucs(&mut self, ucs: NamedUcsCatalog) {
        self.named_ucs = ucs;
    }
    pub fn current_ucs_name(&self) -> &str {
        &self.current_ucs_name
    }
    pub fn current_ucs(&self) -> crate::Ucs3 {
        self.current_ucs
    }
    pub fn set_current_ucs(
        &mut self,
        name: impl Into<String>,
        ucs: crate::Ucs3,
    ) -> Result<(), DxfError> {
        let name = name.into();
        if !name.is_empty()
            && !name.eq_ignore_ascii_case("World")
            && self.named_ucs.get(&name).is_err()
        {
            return Err(group_error(2));
        }
        self.current_ucs_name = name;
        self.current_ucs = ucs;
        Ok(())
    }
    pub fn set_current_ucs_name(&mut self, name: impl Into<String>) -> Result<(), DxfError> {
        let name = name.into();
        if !name.is_empty()
            && self.named_ucs.get(&name).is_err()
            && !name.eq_ignore_ascii_case("World")
        {
            return Err(group_error(2));
        }
        self.current_ucs_name = name;
        Ok(())
    }
    pub fn active_view(&self) -> Option<&NamedView> {
        self.active_view.as_ref()
    }
    pub fn set_active_view(&mut self, view: Option<NamedView>) {
        self.active_view = view;
    }
    /// Return the stable native handle assigned to an entity.
    pub fn entity_handle(&self, index: usize) -> Option<u64> {
        self.entity_handles.get(index).copied()
    }
    /// Find an entity by its native DXF handle.
    pub fn entity_index_by_handle(&self, handle: u64) -> Option<usize> {
        self.entity_handles
            .iter()
            .position(|value| *value == handle)
    }
    pub fn data_links(&self) -> &[DataLink] {
        &self.data_links
    }
    pub fn set_data_links(&mut self, links: Vec<DataLink>) {
        self.data_links = links;
    }
    pub fn named_fields(&self) -> &[NamedField] {
        &self.named_fields
    }
    pub fn set_named_fields(&mut self, fields: Vec<NamedField>) {
        self.named_fields = fields;
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
        let spaces = order
            .iter()
            .map(|index| self.entity_spaces[*index].clone())
            .collect();
        let mut inverse = vec![0; order.len()];
        for (new, old) in order.iter().copied().enumerate() {
            inverse[old] = new;
        }
        self.entities = entities;
        self.records = records;
        self.entity_layers = layers;
        self.entity_styles = styles;
        self.entity_spaces = spaces;
        self.entity_handles = order
            .iter()
            .map(|index| self.entity_handles[*index])
            .collect();
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

    pub fn copy_layout(&mut self, source: &str, target: &str) -> Result<(), DxfError> {
        if source.eq_ignore_ascii_case("Model")
            || !valid_space_name(target)
            || target.eq_ignore_ascii_case("Model")
        {
            return Err(group_error(2));
        }
        let source = canonical_layout_name(&self.layouts, source).ok_or_else(|| group_error(2))?;
        if self.layouts.get(&source).is_none()
            || self
                .layouts
                .names()
                .any(|name| name.eq_ignore_ascii_case(target))
        {
            return Err(group_error(2));
        }
        let copies = self
            .entities
            .iter()
            .zip(self.entity_layers.iter().zip(&self.entity_styles))
            .zip(&self.entity_spaces)
            .filter(|((_, _), space)| space.matches_layout(&source))
            .map(|((entity, (layer, style)), _)| (entity.clone(), layer.clone(), style.clone()))
            .collect::<Vec<_>>();
        for (_, layer, _) in &copies {
            if self.layer(layer).is_none() {
                return Err(group_error(8));
            }
        }
        self.layouts
            .copy(&source, target)
            .map_err(|_| group_error(2))?;
        self.removed_layout_names
            .retain(|name| !name.eq_ignore_ascii_case(target));
        for (entity, layer, style) in copies {
            self.push_with_style_in_space(
                entity,
                &layer,
                style,
                EntitySpace::Paper(target.to_owned()),
            )
            .map_err(|error| DxfError {
                line: 0,
                kind: DxfErrorKind::Layer(error),
            })?;
        }
        Ok(())
    }

    pub fn rename_layout(&mut self, old: &str, new: &str) -> Result<(), DxfError> {
        if old.eq_ignore_ascii_case("Model")
            || !valid_space_name(new)
            || self
                .layouts
                .names()
                .any(|name| !name.eq_ignore_ascii_case(old) && name.eq_ignore_ascii_case(new))
        {
            return Err(group_error(2));
        }
        let old = canonical_layout_name(&self.layouts, old).ok_or_else(|| group_error(2))?;
        self.layouts.rename(&old, new).map_err(|_| group_error(2))?;
        for source_name in &mut self.layout_source_names {
            if source_name.eq_ignore_ascii_case(&old) {
                *source_name = new.to_owned();
            }
        }
        for space in &mut self.entity_spaces {
            if let EntitySpace::Paper(name) = space
                && name.eq_ignore_ascii_case(&old)
            {
                *name = new.to_owned();
            }
        }
        Ok(())
    }

    pub fn delete_layout(&mut self, name: &str) -> Result<(), DxfError> {
        if !valid_space_name(name)
            || name.eq_ignore_ascii_case("Model")
            || canonical_layout_name(&self.layouts, name).is_none()
        {
            return Err(group_error(2));
        }
        let name = canonical_layout_name(&self.layouts, name).ok_or_else(|| group_error(2))?;
        self.layouts.delete(&name).map_err(|_| group_error(2))?;
        self.removed_layout_names.insert(name.to_owned());
        let mut indices = self
            .entity_spaces
            .iter()
            .enumerate()
            .filter_map(|(index, space)| space.matches_layout(&name).then_some(index))
            .collect::<Vec<_>>();
        indices.sort_unstable_by(|left, right| right.cmp(left));
        for index in indices {
            if self.remove(index).is_none() {
                return Err(group_error(67));
            }
        }
        Ok(())
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
        self.push_with_style_in_space(entity, name, style, EntitySpace::Model)
    }

    pub(crate) fn push_with_style_in_space(
        &mut self,
        entity: Entity,
        name: &str,
        style: EntityStyle,
        space: EntitySpace,
    ) -> Result<(), LayerError> {
        if let EntitySpace::Paper(layout) = &space
            && (!valid_space_name(layout)
                || self
                    .layouts
                    .names()
                    .all(|name| !name.eq_ignore_ascii_case(layout)))
        {
            return Err(LayerError::UnknownLayer(layout.clone()));
        }
        let name = self
            .layer(name)
            .ok_or_else(|| LayerError::UnknownLayer(name.into()))?
            .name
            .clone();
        self.entities.push(entity);
        self.records.push(None);
        self.entity_layers.push(name);
        self.entity_styles.push(style);
        let next = self
            .entity_handles
            .iter()
            .copied()
            .max()
            .unwrap_or_else(|| next_handle(&self.source).saturating_sub(1))
            .saturating_add(1)
            .max(next_handle(&self.source));
        self.entity_handles.push(next);
        self.entity_spaces.push(space);
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
        self.entity_spaces.remove(index);
        let removed_handle = self.entity_handles.remove(index);
        let removed = self.entities.remove(index);
        self.data_links
            .retain(|link| link.target_entity_handle() != Some(removed_handle));
        self.named_fields.retain(|field| {
            field.source_handle() != Some(removed_handle)
                && field.target_handle() != Some(removed_handle)
        });
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
        let mut object_handle = next_handle(&self.source).max(
            self.entity_handles
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
                .saturating_add(1),
        );
        let mut generated_handles = HashMap::new();
        for (entity_index, (((entity, record), layer), style)) in self
            .entities
            .iter()
            .zip(&self.records)
            .zip(&self.entity_layers)
            .zip(&self.entity_styles)
            .enumerate()
        {
            if let Some(handle) = self.entity_handles.get(entity_index) {
                generated_handles.insert(entity_index, format!("{handle:X}"));
            }
            let Some(record) = record else {
                let mut serialized = String::new();
                write_entity(&mut serialized, entity, layer, style);
                let handle = generated_handles
                    .get(&entity_index)
                    .cloned()
                    .unwrap_or_else(|| format!("{:X}", object_handle));
                insert_entity_handle(&mut serialized, &handle);
                insert_entity_space(&mut serialized, self.entity_spaces[entity_index].clone());
                added.push_str(&serialized);
                generated_handles.insert(entity_index, handle);
                if let Ok(value) = u64::from_str_radix(
                    generated_handles
                        .get(&entity_index)
                        .map(String::as_str)
                        .unwrap_or("0"),
                    16,
                ) {
                    object_handle = object_handle.max(value.saturating_add(1));
                }
                continue;
            };
            if record_handle_at(&self.source, record.start).is_none()
                && entity == &record.original
                && let Some(handle) = self.entity_handles.get(entity_index)
            {
                let newline = if self.source.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let at = entity_handle_insertion_offset(&self.source, record.start);
                changes.push((at, at, format!("5{newline}{handle:X}{newline}")));
            }
            let space_changed = self
                .entity_spaces
                .get(entity_index)
                .zip(Some(record))
                .is_some_and(|(space, record)| {
                    entity_space_from_record(&self.source, record, record.source_line).is_ok_and(
                        |original| normalized_entity_space(original, &self.layouts) != *space,
                    )
                });
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
                if let Some(handle) = self.entity_handles.get(entity_index) {
                    insert_entity_handle(&mut replacement, &format!("{handle:X}"));
                }
                insert_entity_space(&mut replacement, self.entity_spaces[entity_index].clone());
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
            if space_changed {
                patch_entity_space(
                    &self.source,
                    record,
                    &self.entity_spaces[entity_index],
                    &mut changes,
                );
            }
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
                    insert_entity_space(&mut entity_text, self.entity_spaces[index].clone());
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
        append_header_ucs(&mut header, &self.current_ucs_name, &self.source);
        append_header_point(
            &mut header,
            "$UCSORG",
            self.current_ucs.origin(),
            &self.source,
        );
        append_header_point(
            &mut header,
            "$UCSXDIR",
            self.current_ucs.x_axis(),
            &self.source,
        );
        append_header_point(
            &mut header,
            "$UCSYDIR",
            self.current_ucs.y_axis(),
            &self.source,
        );
        if let Some(view) = &self.active_view {
            let camera = view.camera();
            let target = camera.target();
            let direction = camera
                .direction()
                .unwrap_or_else(|_| Point3::new(0.0, 0.0, 1.0).expect("finite"));
            append_header_point(&mut header, "$VIEWDIR", direction, &self.source);
            append_header_point(&mut header, "$TARGET", target, &self.source);
            append_header_pair(&mut header, "$VIEWCTR", view.center(), &self.source);
            append_header_scalar(&mut header, "$VIEWSIZE", view.view_height(), &self.source);
        }
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
        tables.push_str(&serialize_named_views(&self.named_views, "\n"));
        tables.push_str(&serialize_named_ucs(&self.named_ucs, "\n"));
        if let Some(view) = &self.active_view {
            tables.push_str(&serialize_active_vport(view, "\n"));
        }
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
        let viewport_entities = serialize_viewports(
            &self.layouts,
            &self.source,
            &self.layout_source_names,
            &mut object_handle,
            "\n",
        );
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
        let pdf_underlays = self
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| match entity {
                Entity::PdfUnderlay(underlay) => self
                    .entity_handles
                    .get(index)
                    .copied()
                    .map(|handle| (handle, underlay)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let pdf_definition_objects = serialize_pdf_definitions(
            &pdf_underlays
                .iter()
                .map(|(_, underlay)| *underlay)
                .collect::<Vec<_>>(),
            &self.source,
        );
        let mut pdf_metadata_objects = serialize_pdf_metadata(&pdf_underlays, &mut object_handle);
        let pdf_metadata_spans = pdf_metadata_spans(&self.source);
        if !pdf_metadata_spans.is_empty() {
            for (index, (start, end)) in pdf_metadata_spans.iter().copied().enumerate() {
                changes.push((
                    start,
                    end,
                    if index == 0 {
                        pdf_metadata_objects.clone()
                    } else {
                        String::new()
                    },
                ));
            }
            pdf_metadata_objects.clear();
        }
        let mut dynamic_objects = serialize_dynamic_metadata(
            &self.block_definitions,
            &self.entities,
            &self.entity_handles,
            &mut object_handle,
        );
        let dynamic_spans = dynamic_metadata_spans(&self.source);
        if !dynamic_spans.is_empty() {
            for (index, (start, end)) in dynamic_spans.iter().copied().enumerate() {
                changes.push((
                    start,
                    end,
                    if index == 0 {
                        dynamic_objects.clone()
                    } else {
                        String::new()
                    },
                ));
            }
            dynamic_objects.clear();
        }
        let layout_spans = layout_record_spans(&self.source);
        let layout_mapping = layout_spans
            .iter()
            .enumerate()
            .filter_map(|(index, (source_name, _, _, _))| {
                self.layout_source_names
                    .get(index)
                    .map(|current| (source_name.as_str(), current.as_str()))
            })
            .collect::<Vec<_>>();
        patch_layout_owned_records(
            &self.source,
            &self.layouts,
            &layout_mapping,
            &self.removed_layout_names,
            &mut changes,
        );
        patch_existing_viewports(&self.source, &self.layouts, &layout_mapping, &mut changes);
        for (index, (source_name, start, end, _source_handle)) in layout_spans.iter().enumerate() {
            let current_name = self
                .layout_source_names
                .get(index)
                .map(String::as_str)
                .unwrap_or(source_name);
            if self
                .removed_layout_names
                .iter()
                .any(|removed| removed.eq_ignore_ascii_case(source_name))
                && self.layouts.get(source_name).is_none()
            {
                changes.push((*start, *end, String::new()));
                continue;
            }
            if let Some(layout) = self.layouts.get(current_name) {
                if !layout_record_matches(&self.source, *start, *end, current_name, layout) {
                    patch_layout_record(
                        &self.source,
                        *start,
                        *end,
                        current_name,
                        layout,
                        &mut changes,
                    );
                }
            } else {
                changes.push((*start, *end, String::new()));
            }
        }
        let layout_objects =
            serialize_layouts(&self.layouts, &self.layout_source_names, &mut object_handle);
        let constraint_objects = serialize_constraints(
            &self.constraints,
            &self.source,
            &generated_handles,
            &mut object_handle,
        );
        let data_objects =
            serialize_data_metadata(&self.data_links, &self.named_fields, &mut object_handle);
        let mut page_setup_objects =
            serialize_page_setup_metadata(&self.layouts, &mut object_handle);
        let page_setup_spans = page_setup_metadata_spans(&self.source);
        if !page_setup_spans.is_empty() {
            let mut replaced_record = false;
            for span in page_setup_spans {
                let value = if span.record && !replaced_record {
                    replaced_record = true;
                    page_setup_objects.clone()
                } else {
                    String::new()
                };
                changes.push((span.start, span.end, value));
            }
            if replaced_record {
                page_setup_objects.clear();
            }
        }
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
            || !data_objects.is_empty()
            || !page_setup_objects.is_empty()
            || !pdf_definition_objects.is_empty()
            || !pdf_metadata_objects.is_empty()
            || !dynamic_objects.is_empty()
        {
            let objects = format!(
                "{}{}{}{}{}{}{}{}{}{}",
                images
                    .iter()
                    .map(|image| serialize_image_def(image))
                    .collect::<String>(),
                pdf_definition_objects,
                group_objects,
                layout_objects,
                constraint_objects,
                mleader_style_objects,
                data_objects,
                page_setup_objects,
                pdf_metadata_objects,
                dynamic_objects
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
                let separator = if at > 0 && !output[..at].ends_with('\n') {
                    newline
                } else {
                    ""
                };
                output.insert_str(
                    at,
                    &format!(
                        "{separator}0{newline}SECTION{newline}2{newline}OBJECTS{newline}{}0{newline}ENDSEC",
                        objects.replace('\n', newline)
                    ),
                );
            }
        }
        replace_header_style(&mut output, "$TEXTSTYLE", &self.current_text_style);
        replace_header_style(&mut output, "$DIMSTYLE", &self.current_dimension_style);
        replace_header_style(&mut output, "$CMLEADERSTYLE", &self.current_mleader_style);
        let active_layout = self.layouts.active().name();
        let source_active = read_active_layout(&self.source);
        let active_changed = source_active
            .as_ref()
            .is_some_and(|source_active| !source_active.eq_ignore_ascii_case(active_layout))
            || (source_active.is_none() && !active_layout.eq_ignore_ascii_case("Model"));
        if active_changed {
            replace_header_value(&mut output, "$CTAB", active_layout, "2");
            replace_header_value(
                &mut output,
                "$TILEMODE",
                if active_layout.eq_ignore_ascii_case("Model") {
                    "1"
                } else {
                    "0"
                },
                "70",
            );
            ensure_active_header(&mut output, active_layout);
        }
        replace_header_value(&mut output, "$UCSNAME", &self.current_ucs_name, "2");
        replace_header_point(&mut output, "$UCSORG", self.current_ucs.origin());
        replace_header_point(&mut output, "$UCSXDIR", self.current_ucs.x_axis());
        replace_header_point(&mut output, "$UCSYDIR", self.current_ucs.y_axis());
        if let Some(view) = &self.active_view {
            let camera = view.camera();
            replace_header_point(
                &mut output,
                "$VIEWDIR",
                camera
                    .direction()
                    .unwrap_or_else(|_| Point3::new(0.0, 0.0, 1.0).expect("finite")),
            );
            replace_header_point(&mut output, "$TARGET", camera.target());
            replace_header_pair(&mut output, "$VIEWCTR", view.center());
            replace_header_scalar(&mut output, "$VIEWSIZE", view.view_height());
        }
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
                    | Entity::PdfUnderlay(_)
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
    let pdf_definitions = read_pdf_definitions(input);
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
                "POINT" | "LINE" | "XLINE" | "RAY" | "CIRCLE" | "ARC" | "ELLIPSE" | "3DFACE"
                | "PDFUNDERLAY" => read_entity(&mut groups, child, &mut handles, &pdf_definitions),
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
                | "PDFUNDERLAY"
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

fn serialize_data_metadata(links: &[DataLink], fields: &[NamedField], handle: &mut u64) -> String {
    if links.is_empty() && fields.is_empty() {
        return String::new();
    }
    let mut records = String::new();
    for link in links {
        records.push_str(&format!(
            "1000\nOpenCadData|v1|DATALINK|{}\n",
            data_encode(link.to_metadata())
        ));
    }
    for field in fields {
        records.push_str(&format!(
            "1000\nOpenCadData|v1|FIELD|{}\n",
            data_encode(field_metadata(field))
        ));
    }
    let output = format!(
        "0\nXRECORD\n5\n{:X}\n100\nAcDbXrecord\n{}",
        *handle, records
    );
    *handle = handle.saturating_add(1);
    output
}

fn read_data_metadata(input: &str) -> (Vec<DataLink>, Vec<NamedField>) {
    let mut links = Vec::new();
    let mut fields = Vec::new();
    let mut groups = Groups::new(input);
    let mut objects = false;
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
        let Ok(values) = record_fields(&mut groups) else {
            break;
        };
        for value in values.iter().filter(|field| field.code == 1000) {
            let Some(payload) = value.value.strip_prefix("OpenCadData|v1|") else {
                continue;
            };
            let Some((kind, encoded)) = payload.split_once('|') else {
                continue;
            };
            let Ok(metadata) = data_decode(encoded) else {
                continue;
            };
            match kind {
                "DATALINK" => {
                    if let Ok(link) = DataLink::from_metadata(&metadata) {
                        links.push(link);
                    }
                }
                "FIELD" => {
                    if let Some(field) = field_from_metadata(&metadata) {
                        fields.push(field);
                    }
                }
                _ => {}
            }
        }
    }
    (links, fields)
}

fn field_metadata(field: &NamedField) -> String {
    let source = field
        .source_handle()
        .map_or_else(String::new, |value| value.to_string());
    let target = field
        .target_handle()
        .map_or_else(String::new, |value| value.to_string());
    let cell = field
        .table_cell()
        .map_or_else(String::new, |(row, column)| format!("{row},{column}"));
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
        data_encode(field.name()),
        source,
        target,
        cell,
        data_encode(field_expression_metadata(field.expression())),
        field.precision(),
        field.scale()
    )
}

fn field_from_metadata(value: &str) -> Option<NamedField> {
    let values: Vec<_> = value.split('\t').collect();
    if values.len() != 7 {
        return None;
    }
    let name = data_decode(values[0]).ok()?;
    let source = (!values[1].is_empty())
        .then(|| values[1].parse().ok())
        .flatten();
    let target = (!values[2].is_empty())
        .then(|| values[2].parse().ok())
        .flatten();
    let cell = if values[3].is_empty() {
        None
    } else {
        let (row, column) = values[3].split_once(',')?;
        Some((row.parse().ok()?, column.parse().ok()?))
    };
    let expression = field_expression_from_metadata(&data_decode(values[4]).ok()?)?;
    let precision = values[5].parse().ok()?;
    let scale = values[6].parse().ok()?;
    NamedField::new(name, source, target, cell, expression, precision, scale).ok()
}

fn field_expression_metadata(expression: &FieldExpression) -> String {
    match expression {
        FieldExpression::EntityHandle => "entityhandle".to_owned(),
        FieldExpression::Type => "type".to_owned(),
        FieldExpression::Layer => "layer".to_owned(),
        FieldExpression::Color => "color".to_owned(),
        FieldExpression::Linetype => "linetype".to_owned(),
        FieldExpression::Length => "length".to_owned(),
        FieldExpression::Area => "area".to_owned(),
        FieldExpression::Volume => "volume".to_owned(),
        FieldExpression::DrawingEntityCount => "drawingentitycount".to_owned(),
        FieldExpression::BlockAttribute(name) => format!("blockattr/{name}"),
    }
}

fn field_expression_from_metadata(value: &str) -> Option<FieldExpression> {
    FieldExpression::parse(value).ok()
}

fn data_encode(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}

fn data_decode(value: &str) -> Result<String, ()> {
    let mut output = String::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(bytes[index] as char);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(());
        }
        let byte = u8::from_str_radix(&value[index + 1..index + 3], 16).map_err(|_| ())?;
        output.push(byte as char);
        index += 3;
    }
    Ok(output)
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

fn insert_entity_space(output: &mut String, space: EntitySpace) {
    let fields = match space {
        EntitySpace::Model => "67\n0\n".to_owned(),
        EntitySpace::Paper(name) => format!("67\n1\n410\n{name}\n"),
    };
    let at = output
        .find("\n100\n")
        .map(|index| index + 1)
        .or_else(|| output.find("\n10\n").map(|index| index + 1))
        .unwrap_or(output.len());
    output.insert_str(at, &fields);
}

fn patch_entity_space(
    input: &str,
    record: &EntityRecord,
    space: &EntitySpace,
    changes: &mut Vec<(usize, usize, String)>,
) {
    let Some(source) = input.get(record.start..record.end) else {
        return;
    };
    let mut groups = Groups::new(source);
    let mut fields = Vec::new();
    let mut header_end = record.end;
    while let Ok(Some(group)) = groups.next() {
        if group.code == 0 && !fields.is_empty() {
            header_end = record.start + group.start;
            break;
        }
        fields.push(group);
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let insertion = fields
        .iter()
        .find(|field| field.code == 100)
        .map(|field| record.start + field.start)
        .unwrap_or(header_end);
    let field_end = |index: usize| {
        fields
            .get(index + 1)
            .map_or(header_end, |next| record.start + next.start)
    };
    let flag = fields.iter().position(|field| field.code == 67);
    let layout = fields.iter().position(|field| field.code == 410);
    match space {
        EntitySpace::Model => {
            if let Some(index) = flag {
                changes.push((
                    record.start + fields[index].value_start,
                    record.start + fields[index].value_end,
                    "0".to_owned(),
                ));
            } else {
                changes.push((insertion, insertion, format!("67{newline}0{newline}")));
            }
            if let Some(index) = layout {
                changes.push((
                    record.start + fields[index].start,
                    field_end(index),
                    String::new(),
                ));
            }
        }
        EntitySpace::Paper(name) => {
            if let Some(index) = flag {
                changes.push((
                    record.start + fields[index].value_start,
                    record.start + fields[index].value_end,
                    "1".to_owned(),
                ));
            } else {
                changes.push((insertion, insertion, format!("67{newline}1{newline}")));
            }
            if let Some(index) = layout {
                changes.push((
                    record.start + fields[index].value_start,
                    record.start + fields[index].value_end,
                    name.clone(),
                ));
            } else {
                changes.push((insertion, insertion, format!("410{newline}{name}{newline}")));
            }
        }
    }
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

fn read_named_views(input: &str) -> NamedViewCatalog {
    let mut catalog = NamedViewCatalog::new();
    let mut groups = Groups::new(input);
    let mut tables = false;
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
        if marker.code != 0 || marker.value != "VIEW" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields.iter().find(|f| f.code == 2).map(|f| f.value) else {
            continue;
        };
        let Some(target) = named_point(&fields, 12, 22, 32) else {
            continue;
        };
        let Some(direction) = named_point(&fields, 11, 21, 31) else {
            continue;
        };
        if fields
            .iter()
            .find(|f| f.code == 71)
            .and_then(|f| f.value.parse::<i32>().ok())
            .unwrap_or(0)
            != 0
        {
            continue;
        }
        let twist = named_number(&fields, 50).unwrap_or(0.0);
        let up = camera_up_for_twist(direction, twist)
            .unwrap_or_else(|| Point3::new(0.0, 1.0, 0.0).expect("finite"));
        let camera = Camera3::from_direction(target, 1.0, direction, up);
        let Ok(camera) = camera else {
            continue;
        };
        let center = [
            named_number(&fields, 10).unwrap_or(0.0),
            named_number(&fields, 20).unwrap_or(0.0),
        ];
        let height = named_number(&fields, 40)
            .filter(|v| *v > 0.0)
            .unwrap_or(1.0);
        let width = named_number(&fields, 41)
            .filter(|v| *v > 0.0)
            .unwrap_or(height);
        if let Ok(mut view) = NamedView::with_viewport(name, camera, center, height, width) {
            let associated = fields
                .iter()
                .find(|field| field.code == 72)
                .and_then(|field| field.value.parse::<i32>().ok())
                .is_some_and(|value| value != 0);
            if associated
                && let (Some(origin), Some(x), Some(y)) = (
                    named_point(&fields, 110, 120, 130),
                    named_point(&fields, 111, 121, 131),
                    named_point(&fields, 112, 122, 132),
                )
                && let Ok(ucs) = crate::Ucs3::from_points(origin, x, y)
            {
                let _ = view.set_associated_ucs(Some(ucs), None::<String>);
            }
            let _ = catalog.upsert(view);
        }
    }
    catalog
}

fn read_named_ucs(input: &str) -> NamedUcsCatalog {
    let mut catalog = NamedUcsCatalog::new();
    let mut groups = Groups::new(input);
    let mut tables = false;
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
        if marker.code != 0 || marker.value != "UCS" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(name) = fields.iter().find(|f| f.code == 2).map(|f| f.value) else {
            continue;
        };
        let (Some(origin), Some(x), Some(y)) = (
            named_point(&fields, 10, 20, 30),
            named_point(&fields, 11, 21, 31),
            named_point(&fields, 12, 22, 32),
        ) else {
            continue;
        };
        if let Ok(ucs) = crate::Ucs3::from_points(origin, x, y)
            && let Ok(value) = NamedUcs::new(name, ucs)
        {
            let _ = catalog.upsert(value);
        }
    }
    catalog
}

fn read_current_ucs(input: &str) -> (String, crate::Ucs3) {
    let mut groups = Groups::new(input);
    let mut header = false;
    let mut name = "World".to_owned();
    let mut origin = Point3::default();
    let mut x_point = Point3::new(1.0, 0.0, 0.0).expect("finite");
    let mut y_point = Point3::new(0.0, 1.0, 0.0).expect("finite");
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
        match marker.value {
            "$UCSNAME" => {
                if let Ok(Some(value)) = groups.next()
                    && value.code == 2
                {
                    name = value.value.to_owned();
                }
            }
            "$UCSORG" | "$UCSXDIR" | "$UCSYDIR" => {
                let Ok(Some(x)) = groups.next() else { continue };
                let Ok(Some(y)) = groups.next() else { continue };
                let Ok(Some(z)) = groups.next() else { continue };
                if x.code != 10 || y.code != 20 || z.code != 30 {
                    continue;
                }
                let Ok(point) = Point3::new(
                    x.value.parse().unwrap_or(f64::NAN),
                    y.value.parse().unwrap_or(f64::NAN),
                    z.value.parse().unwrap_or(f64::NAN),
                ) else {
                    continue;
                };
                match marker.value {
                    "$UCSORG" => origin = point,
                    "$UCSXDIR" => x_point = point,
                    "$UCSYDIR" => y_point = point,
                    _ => unreachable!(),
                }
            }
            _ => {
                let _ = groups.next();
            }
        }
    }
    let ucs =
        crate::Ucs3::from_points(origin, x_point, y_point).unwrap_or_else(|_| crate::Ucs3::world());
    (name, ucs)
}

fn read_active_view(input: &str) -> Option<NamedView> {
    if let Some(view) = read_active_vport(input) {
        return Some(view);
    }
    let mut groups = Groups::new(input);
    let mut header = false;
    let mut center = [0.0, 0.0];
    let mut size: f64 = 1.0;
    let mut direction = Point3::new(0.0, 0.0, 1.0).expect("finite");
    let mut target = Point3::default();
    let mut saw_view = false;
    while let Ok(Some(marker)) = groups.next() {
        if marker.code == 2 && marker.value == "HEADER" {
            header = true;
            continue;
        }
        if marker.code == 0 && marker.value == "ENDSEC" {
            break;
        }
        if !header || marker.code != 9 {
            continue;
        }
        match marker.value {
            "$VIEWCTR" => {
                let (Ok(Some(x)), Ok(Some(y))) = (groups.next(), groups.next()) else {
                    continue;
                };
                if x.code == 10 && y.code == 20 {
                    center = [
                        x.value.parse().unwrap_or(0.0),
                        y.value.parse().unwrap_or(0.0),
                    ];
                    saw_view = true;
                }
            }
            "$VIEWSIZE" => {
                if let Ok(Some(value)) = groups.next() {
                    size = value.value.parse().unwrap_or(1.0);
                    saw_view = true;
                }
            }
            "$VIEWDIR" | "$TARGET" | "$VIEWTARGET" => {
                let (Ok(Some(x)), Ok(Some(y)), Ok(Some(z))) =
                    (groups.next(), groups.next(), groups.next())
                else {
                    continue;
                };
                if x.code == 10
                    && y.code == 20
                    && z.code == 30
                    && let Ok(point) = Point3::new(
                        x.value.parse().unwrap_or(f64::NAN),
                        y.value.parse().unwrap_or(f64::NAN),
                        z.value.parse().unwrap_or(f64::NAN),
                    )
                {
                    if marker.value == "$VIEWDIR" {
                        direction = point;
                    } else {
                        target = point;
                    }
                    saw_view = true;
                }
            }
            _ => {
                let _ = groups.next();
            }
        }
    }
    if !saw_view {
        return None;
    }
    let camera = Camera3::from_basis(
        target,
        1.0,
        direction,
        Point3::new(0.0, 1.0, 0.0).expect("finite"),
    )
    .ok()?;
    NamedView::with_viewport(
        "ActiveView",
        camera,
        center,
        size.max(f64::EPSILON),
        size.max(f64::EPSILON),
    )
    .ok()
}

fn read_active_vport(input: &str) -> Option<NamedView> {
    let mut groups = Groups::new(input);
    let mut tables = false;
    let mut active = None;
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
        if marker.code != 0 || marker.value != "VPORT" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        if fields
            .iter()
            .find(|field| field.code == 2)
            .is_none_or(|field| field.value != "*ACTIVE")
        {
            continue;
        }
        let number = |code| {
            fields
                .iter()
                .find(|field| field.code == code)
                .and_then(|field| field.value.parse::<f64>().ok())
                .filter(|value| value.is_finite())
        };
        let (Some(cx), Some(cy), Some(height), Some(direction), Some(target)) = (
            Some(number(12).unwrap_or(0.0)),
            Some(number(22).unwrap_or(0.0)),
            number(45).or_else(|| number(41)),
            named_point(&fields, 16, 26, 36),
            named_point(&fields, 17, 27, 37),
        ) else {
            continue;
        };
        if height <= 0.0 {
            continue;
        }
        let aspect = number(40).filter(|value| *value > 0.0).unwrap_or(1.0);
        let twist = number(51).unwrap_or(0.0);
        let up = camera_up_for_twist(direction, twist)?;
        let camera = Camera3::from_direction(target, 1.0, direction, up).ok()?;
        active = NamedView::with_viewport(
            "*ACTIVE",
            camera,
            [cx, cy],
            height,
            (height * aspect).max(f64::EPSILON),
        )
        .ok();
    }
    active
}

fn named_number(fields: &[Group<'_>], code: u16) -> Option<f64> {
    fields
        .iter()
        .find(|f| f.code == code)
        .and_then(|f| f.value.parse().ok())
}

fn named_point(fields: &[Group<'_>], x: u16, y: u16, z: u16) -> Option<Point3> {
    Point3::new(
        named_number(fields, x)?,
        named_number(fields, y)?,
        named_number(fields, z).unwrap_or(0.0),
    )
    .ok()
}

fn camera_up_for_twist(direction: Point3, twist: f64) -> Option<Point3> {
    if !twist.is_finite() {
        return None;
    }
    let (dx, dy, dz) = (direction.x(), direction.y(), direction.z());
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    if !length.is_finite() || length <= f64::EPSILON {
        return None;
    }
    let f = [dx / length, dy / length, dz / length];
    let reference = if f[2].abs() < 0.999 {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let right = [
        reference[1] * f[2] - reference[2] * f[1],
        reference[2] * f[0] - reference[0] * f[2],
        reference[0] * f[1] - reference[1] * f[0],
    ];
    let right_length = (right[0] * right[0] + right[1] * right[1] + right[2] * right[2]).sqrt();
    if !right_length.is_finite() || right_length <= f64::EPSILON {
        return None;
    }
    let r = [
        right[0] / right_length,
        right[1] / right_length,
        right[2] / right_length,
    ];
    let up = [
        f[1] * r[2] - f[2] * r[1],
        f[2] * r[0] - f[0] * r[2],
        f[0] * r[1] - f[1] * r[0],
    ];
    let (sin, cos) = twist.sin_cos();
    Point3::new(
        up[0] * cos + r[0] * sin,
        up[1] * cos + r[1] * sin,
        up[2] * cos + r[2] * sin,
    )
    .ok()
}

fn camera_twist(camera: Camera3) -> f64 {
    let Ok(direction) = camera.direction() else {
        return 0.0;
    };
    let Ok(up) = camera.up() else {
        return 0.0;
    };
    let Some(reference) = camera_up_for_twist(direction, 0.0) else {
        return 0.0;
    };
    let (dx, dy, dz) = (direction.x(), direction.y(), direction.z());
    let (ux, uy, uz) = (up.x(), up.y(), up.z());
    let (rx, ry, rz) = (
        reference.y() * dz - reference.z() * dy,
        reference.z() * dx - reference.x() * dz,
        reference.x() * dy - reference.y() * dx,
    );
    (ux * rx + uy * ry + uz * rz)
        .atan2(ux * reference.x() + uy * reference.y() + uz * reference.z())
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

fn serialize_named_views(catalog: &NamedViewCatalog, newline: &str) -> String {
    if catalog.views().is_empty() {
        return String::new();
    }
    let mut output = format!(
        "0{newline}TABLE{newline}2{newline}VIEW{newline}70{newline}{}{newline}",
        catalog.views().len()
    );
    for view in catalog.views() {
        let target = view.camera().target();
        let direction = view
            .camera()
            .direction()
            .unwrap_or_else(|_| Point3::new(0.0, 0.0, 1.0).expect("finite"));
        let twist = camera_twist(view.camera());
        let associated = view.associated_ucs();
        output.push_str(&format!(
            "0{newline}VIEW{newline}2{newline}{}{newline}40{newline}{}{newline}41{newline}{}{newline}10{newline}{}{newline}20{newline}{}{newline}11{newline}{}{newline}21{newline}{}{newline}31{newline}{}{newline}12{newline}{}{newline}22{newline}{}{newline}32{newline}{}{newline}50{newline}{twist}{newline}71{newline}0{newline}72{newline}{}{newline}",
            view.name(),
            view.view_height(),
            view.view_width(),
            view.center()[0],
            view.center()[1],
            direction.x(),
            direction.y(),
            direction.z(),
            target.x(),
            target.y(),
            target.z(),
            u8::from(associated.is_some())
        ));
        if let Some(ucs) = associated {
            let origin = ucs.origin();
            let x = ucs
                .local_to_world(Point3::new(1.0, 0.0, 0.0).expect("finite"))
                .unwrap_or(origin);
            let y = ucs
                .local_to_world(Point3::new(0.0, 1.0, 0.0).expect("finite"))
                .unwrap_or(origin);
            output.push_str(&format!(
                "110{newline}{}{newline}120{newline}{}{newline}130{newline}{}{newline}111{newline}{}{newline}121{newline}{}{newline}131{newline}{}{newline}112{newline}{}{newline}122{newline}{}{newline}132{newline}{}{newline}",
                origin.x(), origin.y(), origin.z(), x.x(), x.y(), x.z(), y.x(), y.y(), y.z()
            ));
        }
    }
    output.push_str(&format!("0{newline}ENDTAB{newline}"));
    output
}

fn serialize_named_ucs(catalog: &NamedUcsCatalog, newline: &str) -> String {
    if catalog.ucs().is_empty() {
        return String::new();
    }
    let mut output = format!(
        "0{newline}TABLE{newline}2{newline}UCS{newline}70{newline}{}{newline}",
        catalog.ucs().len()
    );
    for named in catalog.ucs() {
        let ucs = named.ucs();
        let origin = ucs.origin();
        let x = ucs
            .local_to_world(Point3::new(1.0, 0.0, 0.0).expect("finite"))
            .unwrap_or(origin);
        let y = ucs
            .local_to_world(Point3::new(0.0, 1.0, 0.0).expect("finite"))
            .unwrap_or(origin);
        output.push_str(&format!("0{newline}UCS{newline}2{newline}{}{newline}10{newline}{}{newline}20{newline}{}{newline}30{newline}{}{newline}11{newline}{}{newline}21{newline}{}{newline}31{newline}{}{newline}12{newline}{}{newline}22{newline}{}{newline}32{newline}{}{newline}79{newline}0{newline}", named.name(), origin.x(), origin.y(), origin.z(), x.x(), x.y(), x.z(), y.x(), y.y(), y.z()));
    }
    output.push_str(&format!("0{newline}ENDTAB{newline}"));
    output
}

fn serialize_active_vport(view: &NamedView, newline: &str) -> String {
    let camera = view.camera();
    let direction = camera
        .direction()
        .unwrap_or_else(|_| Point3::new(0.0, 0.0, 1.0).expect("finite"));
    let target = camera.target();
    let height = view.view_height();
    let aspect = (view.view_width() / height).max(f64::EPSILON);
    format!(
        "0{newline}TABLE{newline}2{newline}VPORT{newline}70{newline}1{newline}0{newline}VPORT{newline}2{newline}*ACTIVE{newline}40{newline}{aspect}{newline}45{newline}{height}{newline}12{newline}{}{newline}22{newline}{}{newline}16{newline}{}{newline}26{newline}{}{newline}36{newline}{}{newline}17{newline}{}{newline}27{newline}{}{newline}37{newline}{}{newline}51{newline}{}{newline}0{newline}ENDTAB{newline}",
        view.center()[0],
        view.center()[1],
        direction.x(),
        direction.y(),
        direction.z(),
        target.x(),
        target.y(),
        target.z(),
        camera_twist(camera)
    )
}

fn append_header_style(header: &mut String, variable: &str, value: &str, source: &str) {
    if source.contains(variable) || header.contains(variable) {
        return;
    }
    header.push_str(&format!("9\n{variable}\n1\n{value}\n"));
}

fn append_header_ucs(header: &mut String, value: &str, source: &str) {
    if source.contains("$UCSNAME") || header.contains("$UCSNAME") {
        return;
    }
    header.push_str(&format!("9\n$UCSNAME\n2\n{value}\n"));
}

fn append_header_scalar(header: &mut String, variable: &str, value: f64, source: &str) {
    if source.contains(variable) || header.contains(variable) {
        return;
    }
    header.push_str(&format!("9\n{variable}\n40\n{value}\n"));
}

fn append_header_pair(header: &mut String, variable: &str, value: [f64; 2], source: &str) {
    if source.contains(variable) || header.contains(variable) {
        return;
    }
    header.push_str(&format!(
        "9\n{variable}\n10\n{}\n20\n{}\n",
        value[0], value[1]
    ));
}

fn append_header_point(header: &mut String, variable: &str, value: Point3, source: &str) {
    if source.contains(variable) || header.contains(variable) {
        return;
    }
    header.push_str(&format!(
        "9\n{variable}\n10\n{}\n20\n{}\n30\n{}\n",
        value.x(),
        value.y(),
        value.z()
    ));
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
    replace_header_value(output, variable, value, "1");
}

fn replace_header_value(output: &mut String, variable: &str, value: &str, code: &str) {
    for ending in ["\r\n", "\n"] {
        let marker = format!("{ending}{variable}{ending}{code}{ending}");
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

fn replace_header_scalar(output: &mut String, variable: &str, value: f64) {
    replace_header_value(output, variable, &value.to_string(), "40");
}

fn replace_header_pair(output: &mut String, variable: &str, value: [f64; 2]) {
    replace_header_component(output, variable, "10", &value[0].to_string());
    replace_header_component(output, variable, "20", &value[1].to_string());
}

fn replace_header_point(output: &mut String, variable: &str, value: Point3) {
    replace_header_component(output, variable, "10", &value.x().to_string());
    replace_header_component(output, variable, "20", &value.y().to_string());
    replace_header_component(output, variable, "30", &value.z().to_string());
}

fn replace_header_component(output: &mut String, variable: &str, code: &str, value: &str) {
    for ending in ["\r\n", "\n"] {
        let prefix = format!("{ending}{variable}{ending}");
        let Some(variable_start) = output.find(&prefix) else {
            continue;
        };
        let search_start = variable_start + prefix.len();
        let Some(code_start_rel) = output[search_start..].find(&format!("{code}{ending}")) else {
            continue;
        };
        let code_start = search_start + code_start_rel + code.len() + ending.len();
        let value_end = output[code_start..]
            .find(ending)
            .map_or(output.len(), |offset| code_start + offset);
        output.replace_range(code_start..value_end, value);
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
            let layout_fields = subclass_fields(&fields, "AcDbLayout").unwrap_or(&fields);
            let plot_fields = subclass_fields(&fields, "AcDbPlotSettings").unwrap_or(&fields);
            let Some(name) = layout_fields
                .iter()
                .find(|field| field.code == 1)
                .map(|field| field.value.to_owned())
            else {
                continue;
            };
            let Some(setup) = parse_native_page_setup(plot_fields, group.line) else {
                continue;
            };
            if name == "Model" {
                layouts.active_mut().set_setup(setup);
            } else {
                let _ = layouts.create(name, setup);
            }
        }
    }
    import_viewports(input, &mut layouts);
    layouts
}

fn subclass_fields<'a>(fields: &'a [Group<'a>], marker: &str) -> Option<&'a [Group<'a>]> {
    let start = fields
        .iter()
        .position(|field| field.code == 100 && field.value == marker)?;
    let end = fields[start + 1..]
        .iter()
        .position(|field| field.code == 100)
        .map_or(fields.len(), |offset| start + 1 + offset);
    Some(&fields[start + 1..end])
}

fn parse_native_page_setup(plot: &[Group<'_>], line: usize) -> Option<crate::PageSetup> {
    let width = native_optional_number(plot, 44, line)
        .ok()?
        .unwrap_or(210.0);
    let height = native_optional_number(plot, 45, line)
        .ok()?
        .unwrap_or(297.0);
    let mut setup = crate::PageSetup::new(width, height).ok()?;
    let rotation = plot
        .iter()
        .find(|field| field.code == 73)
        .map(|field| field.value.parse::<i32>())
        .transpose()
        .ok()?;
    if rotation.is_some_and(|value| value % 2 != 0) {
        setup.set_orientation(crate::Orientation::Landscape);
    }
    let margins = [
        native_optional_number(plot, 40, line).ok()?.unwrap_or(0.0),
        native_optional_number(plot, 41, line).ok()?.unwrap_or(0.0),
        native_optional_number(plot, 42, line).ok()?.unwrap_or(0.0),
        native_optional_number(plot, 43, line).ok()?.unwrap_or(0.0),
    ];
    setup.set_margins(margins).ok()?;
    let controls = import_native_plot_controls(plot, line, setup);
    Some(controls.unwrap_or(setup))
}

fn native_optional_number(plot: &[Group<'_>], code: u16, line: usize) -> Result<Option<f64>, ()> {
    field_number(plot, code, line).map_err(|_| ())
}

fn native_required_number(plot: &[Group<'_>], code: u16, line: usize) -> Result<f64, ()> {
    native_optional_number(plot, code, line)?.ok_or(())
}

fn import_native_plot_controls(
    plot: &[Group<'_>],
    line: usize,
    setup: crate::PageSetup,
) -> Result<crate::PageSetup, ()> {
    let setup = parse_native_plot_area(plot, line, setup)?;
    let setup = parse_native_plot_offset(plot, line, setup)?;
    let setup = parse_native_plot_scale(plot, line, setup)?;
    let mut candidate = setup;
    let flags = match plot.iter().find(|field| field.code == 70) {
        Some(field) => field.value.parse::<i32>().map_err(|_| ())?,
        None => 0,
    };
    candidate.set_centered(flags & 4 != 0).map_err(|_| ())?;
    Ok(candidate)
}

fn parse_native_plot_area(
    plot: &[Group<'_>],
    line: usize,
    setup: crate::PageSetup,
) -> Result<crate::PageSetup, ()> {
    let Some(field) = plot.iter().find(|field| field.code == 74) else {
        return Ok(setup);
    };
    let mut candidate = setup;
    match field.value.parse::<i32>().map_err(|_| ())? {
        1 => candidate.set_plot_area(crate::PlotArea::Extents),
        4 => candidate.set_plot_area(crate::PlotArea::Window([
            native_required_number(plot, 48, line)?,
            native_required_number(plot, 49, line)?,
            native_required_number(plot, 140, line)?,
            native_required_number(plot, 141, line)?,
        ])),
        5 => candidate.set_plot_area(crate::PlotArea::Layout),
        _ => return Err(()),
    }
    .map_err(|_| ())?;
    Ok(candidate)
}

fn parse_native_plot_offset(
    plot: &[Group<'_>],
    line: usize,
    setup: crate::PageSetup,
) -> Result<crate::PageSetup, ()> {
    let x = native_optional_number(plot, 46, line)?;
    let y = native_optional_number(plot, 47, line)?;
    let Some((x, y)) = x.zip(y) else {
        return if x.is_none() && y.is_none() {
            Ok(setup)
        } else {
            Err(())
        };
    };
    let mut candidate = setup;
    candidate.set_plot_offset([x, y]).map_err(|_| ())?;
    Ok(candidate)
}

fn parse_native_plot_scale(
    plot: &[Group<'_>],
    line: usize,
    setup: crate::PageSetup,
) -> Result<crate::PageSetup, ()> {
    let flags = match plot.iter().find(|field| field.code == 70) {
        Some(field) => field.value.parse::<i32>().map_err(|_| ())?,
        None => 0,
    };
    let use_standard = flags & 16 != 0;
    let Some(scale_field) = plot.iter().find(|field| field.code == 75) else {
        if use_standard {
            return Ok(setup);
        }
        return parse_native_custom_scale(plot, line, setup);
    };
    let scale_kind = scale_field.value.parse::<i32>().map_err(|_| ())?;
    if use_standard {
        if scale_kind == 0 {
            let mut candidate = setup;
            candidate.set_fit_to_paper(true);
            return Ok(candidate);
        }
        let scale = native_standard_scale(scale_kind).ok_or(())?;
        let mut candidate = setup;
        candidate.set_scale(scale).map_err(|_| ())?;
        return Ok(candidate);
    }
    parse_native_custom_scale(plot, line, setup)
}

fn parse_native_custom_scale(
    plot: &[Group<'_>],
    line: usize,
    setup: crate::PageSetup,
) -> Result<crate::PageSetup, ()> {
    let numerator = native_optional_number(plot, 142, line)?;
    let denominator = native_optional_number(plot, 143, line)?;
    let Some((numerator, denominator)) = numerator.zip(denominator) else {
        return if numerator.is_none() && denominator.is_none() {
            Ok(setup)
        } else {
            Err(())
        };
    };
    if numerator <= 0.0 || denominator <= 0.0 {
        return Err(());
    }
    let mut candidate = setup;
    candidate
        .set_scale(numerator / denominator)
        .map_err(|_| ())?;
    Ok(candidate)
}

fn native_standard_scale(value: i32) -> Option<f64> {
    match value {
        1 => Some(1.0 / 1536.0),
        2 => Some(1.0 / 768.0),
        3 => Some(1.0 / 384.0),
        4 => Some(1.0 / 192.0),
        5 => Some(1.0 / 128.0),
        6 => Some(1.0 / 96.0),
        7 => Some(1.0 / 64.0),
        8 => Some(1.0 / 48.0),
        9 => Some(1.0 / 32.0),
        10 => Some(1.0 / 24.0),
        11 => Some(1.0 / 16.0),
        12 => Some(1.0 / 12.0),
        13 => Some(1.0 / 4.0),
        14 => Some(0.5),
        15 => Some(1.0),
        16 => Some(1.0),
        17 => Some(0.5),
        18 => Some(0.25),
        19 => Some(0.125),
        20 => Some(0.1),
        21 => Some(0.0625),
        22 => Some(0.05),
        23 => Some(1.0 / 30.0),
        24 => Some(0.025),
        25 => Some(0.02),
        26 => Some(0.01),
        27 => Some(2.0),
        28 => Some(4.0),
        29 => Some(8.0),
        30 => Some(10.0),
        31 => Some(100.0),
        32 => Some(1000.0),
        _ => None,
    }
}

#[derive(Clone)]
struct PageSetupLayoutSnapshot {
    layout: String,
    applied: Option<String>,
    setup: crate::PageSetup,
}

struct PageSetupMetadataState {
    catalog: Vec<(String, crate::PageSetup)>,
    snapshots: Vec<PageSetupLayoutSnapshot>,
}

fn read_page_setup_metadata(
    input: &str,
    layouts: &mut crate::LayoutCollection,
) -> Result<(), DxfError> {
    let metadata = collect_page_setup_metadata(input)?;
    for (name, setup) in metadata.catalog {
        layouts
            .page_setups_mut()
            .add(crate::NamedPageSetup::new(name, setup).map_err(|_| dynamic_error())?)
            .map_err(|_| dynamic_error())?;
    }
    for snapshot in metadata.snapshots {
        layouts
            .restore_page_setup_snapshot(
                &snapshot.layout,
                snapshot.setup,
                snapshot.applied.as_deref(),
            )
            .map_err(|_| dynamic_error())?;
    }
    import_native_page_setups(input, layouts);
    Ok(())
}

fn import_native_page_setups(input: &str, layouts: &mut crate::LayoutCollection) {
    let mut groups = Groups::new(input);
    let mut objects = false;
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
        if marker.code != 0 || marker.value != "PLOTSETTINGS" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let plot = subclass_fields(&fields, "AcDbPlotSettings").unwrap_or(&fields);
        let Some(name) = plot
            .iter()
            .find(|field| field.code == 1)
            .map(|field| field.value.to_owned())
        else {
            continue;
        };
        if layouts.page_setups().get(&name).is_ok() {
            continue;
        }
        let Some(setup) = parse_native_page_setup(plot, marker.line) else {
            continue;
        };
        let Ok(entry) = crate::NamedPageSetup::new(name, setup) else {
            continue;
        };
        if layouts.page_setups_mut().insert_or_replace(entry).is_err() {
            continue;
        }
    }
}

fn collect_page_setup_metadata(input: &str) -> Result<PageSetupMetadataState, DxfError> {
    let mut page_setups = Vec::new();
    let mut snapshots = Vec::new();
    let mut groups = Groups::new(input);
    let mut objects = false;
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
        let fields = record_fields(&mut groups)?;
        for field in fields.iter().filter(|field| field.code == 1000) {
            let Some(payload) = field.value.strip_prefix("OpenCadData|v1|PAGESETUP|") else {
                continue;
            };
            let values: Vec<_> = payload.split('|').collect();
            let Some(kind) = values.first().copied() else {
                return Err(dynamic_error());
            };
            match kind {
                "CATALOG" => {
                    if values.len() != 10 && values.len() != 20 {
                        return Err(dynamic_error());
                    }
                    let name = dynamic_decode(values[1]).ok_or_else(dynamic_error)?;
                    let setup = parse_page_setup_values(&values[2..], marker.line)?;
                    page_setups.push((name, setup));
                }
                "LAYOUT" => {
                    if values.len() != 11 && values.len() != 21 {
                        return Err(dynamic_error());
                    }
                    let layout = dynamic_decode(values[1]).ok_or_else(dynamic_error)?;
                    let applied = if values[2].is_empty() {
                        None
                    } else {
                        Some(dynamic_decode(values[2]).ok_or_else(dynamic_error)?)
                    };
                    let setup = parse_page_setup_values(&values[3..], marker.line)?;
                    if snapshots.iter().any(|existing: &PageSetupLayoutSnapshot| {
                        existing.layout.eq_ignore_ascii_case(&layout)
                    }) {
                        return Err(dynamic_error());
                    }
                    snapshots.push(PageSetupLayoutSnapshot {
                        layout,
                        applied,
                        setup,
                    });
                }
                _ => return Err(dynamic_error()),
            }
        }
    }
    Ok(PageSetupMetadataState {
        catalog: page_setups,
        snapshots,
    })
}

fn parse_page_setup_values(values: &[&str], line: usize) -> Result<crate::PageSetup, DxfError> {
    if values.len() != 8 && values.len() != 18 {
        return Err(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(1000),
        });
    }
    let numbers = values[..7]
        .iter()
        .map(|value| value.parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(1000),
        })?;
    if numbers.iter().any(|value| !value.is_finite()) {
        return Err(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(1000),
        });
    }
    let mut setup = crate::PageSetup::new(numbers[0], numbers[1]).map_err(|_| dynamic_error())?;
    match values[7] {
        "0" => setup.set_orientation(crate::Orientation::Portrait),
        "1" => setup.set_orientation(crate::Orientation::Landscape),
        _ => return Err(dynamic_error()),
    }
    setup
        .set_margins([numbers[2], numbers[3], numbers[4], numbers[5]])
        .map_err(|_| dynamic_error())?;
    setup.set_scale(numbers[6]).map_err(|_| dynamic_error())?;
    if values.len() == 18 {
        if values[8] != "PLOT2" {
            return Err(dynamic_error());
        }
        let bounds = parse_page_setup_numbers(&values[14..18], line)?;
        let area = match values[9] {
            "E" => crate::PlotArea::Extents,
            "L" => crate::PlotArea::Layout,
            "W" => crate::PlotArea::Window([bounds[0], bounds[1], bounds[2], bounds[3]]),
            _ => return Err(dynamic_error()),
        };
        setup.set_plot_area(area).map_err(|_| dynamic_error())?;
        let fit = parse_page_setup_bool(values[10], line)?;
        setup.set_fit_to_paper(fit);
        let offset = parse_page_setup_numbers(&values[11..13], line)?;
        setup
            .set_plot_offset([offset[0], offset[1]])
            .map_err(|_| dynamic_error())?;
        let centered = parse_page_setup_bool(values[13], line)?;
        setup.set_centered(centered).map_err(|_| dynamic_error())?;
    }
    Ok(setup)
}

fn parse_page_setup_numbers(values: &[&str], line: usize) -> Result<Vec<f64>, DxfError> {
    values
        .iter()
        .map(|value| value.parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(1000),
        })
        .and_then(|numbers| {
            if numbers.iter().all(|value| value.is_finite()) {
                Ok(numbers)
            } else {
                Err(DxfError {
                    line,
                    kind: DxfErrorKind::UnsupportedProperty(1000),
                })
            }
        })
}

fn parse_page_setup_bool(value: &str, line: usize) -> Result<bool, DxfError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(1000),
        }),
    }
}

fn serialize_page_setup_metadata(layouts: &crate::LayoutCollection, handle: &mut u64) -> String {
    let default_setup = crate::PageSetup::new(210.0, 297.0).expect("finite defaults");
    let needs_metadata = !layouts.page_setups().entries().is_empty()
        || layouts.names().any(|name| name != "Model")
        || layouts.names().any(|name| {
            layouts
                .get(name)
                .is_some_and(|layout| layout.setup() != default_setup)
        });
    if !needs_metadata {
        return String::new();
    }
    let mut records = String::new();
    for entry in layouts.page_setups().entries() {
        let setup = entry.setup();
        records.push_str(&format!(
            "1000\nOpenCadData|v1|PAGESETUP|CATALOG|{}|{}\n",
            dynamic_encode(entry.name()),
            page_setup_values(setup)
        ));
    }
    for name in layouts.names() {
        let Some(layout) = layouts.get(name) else {
            continue;
        };
        let setup = layout.setup();
        let applied = layout
            .applied_page_setup()
            .map(dynamic_encode)
            .unwrap_or_default();
        records.push_str(&format!(
            "1000\nOpenCadData|v1|PAGESETUP|LAYOUT|{}|{}|{}\n",
            dynamic_encode(name),
            applied,
            page_setup_values(setup)
        ));
    }
    if records.is_empty() {
        return String::new();
    }
    let output = format!(
        "0\nXRECORD\n5\n{:X}\n100\nAcDbXrecord\n{}",
        *handle, records
    );
    *handle = handle.saturating_add(1);
    output
}

fn page_setup_values(setup: crate::PageSetup) -> String {
    let paper = setup.paper();
    let margins = setup.margins();
    let orientation = matches!(setup.orientation(), crate::Orientation::Landscape) as u8;
    let legacy = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        paper[0],
        paper[1],
        margins[0],
        margins[1],
        margins[2],
        margins[3],
        setup.scale(),
        orientation
    );
    let default_area = matches!(setup.plot_area(), crate::PlotArea::Extents);
    let default_controls = default_area
        && !setup.fit_to_paper()
        && setup.plot_offset() == [0.0, 0.0]
        && !setup.centered();
    if default_controls {
        return legacy;
    }
    let (area, bounds) = match setup.plot_area() {
        crate::PlotArea::Extents => ("E", [0.0; 4]),
        crate::PlotArea::Layout => ("L", [0.0; 4]),
        crate::PlotArea::Window(bounds) => ("W", bounds),
    };
    format!(
        "{legacy}|PLOT2|{area}|{}|{}|{}|{}|{}|{}|{}|{}",
        setup.fit_to_paper() as u8,
        setup.plot_offset()[0],
        setup.plot_offset()[1],
        setup.centered() as u8,
        bounds[0],
        bounds[1],
        bounds[2],
        bounds[3]
    )
}

struct PageSetupMetadataSpan {
    start: usize,
    end: usize,
    record: bool,
}

fn page_setup_metadata_spans(input: &str) -> Vec<PageSetupMetadataSpan> {
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
        let owned = fields
            .iter()
            .filter(|field| {
                field.code == 1000 && field.value.starts_with("OpenCadData|v1|PAGESETUP|")
            })
            .collect::<Vec<_>>();
        if owned.is_empty() {
            continue;
        }
        let record_end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(input.len(), |next| next.start);
        let xrecord_marker = fields
            .iter()
            .position(|field| field.code == 100 && field.value == "AcDbXrecord");
        let mixed = fields.iter().enumerate().any(|(index, field)| {
            if field.code == 1000 && field.value.starts_with("OpenCadData|v1|PAGESETUP|") {
                return false;
            }
            !is_page_setup_structural_field(field, index, xrecord_marker)
        });
        if mixed {
            for (index, field) in fields.iter().enumerate() {
                if field.code != 1000 || !field.value.starts_with("OpenCadData|v1|PAGESETUP|") {
                    continue;
                }
                let end = fields.get(index + 1).map_or(record_end, |next| next.start);
                spans.push(PageSetupMetadataSpan {
                    start: field.start,
                    end,
                    record: false,
                });
            }
        } else {
            spans.push(PageSetupMetadataSpan {
                start: marker.start,
                end: record_end,
                record: true,
            });
        }
    }
    spans
}

fn is_page_setup_structural_field(
    field: &Group<'_>,
    index: usize,
    xrecord_marker: Option<usize>,
) -> bool {
    let Some(marker) = xrecord_marker else {
        return false;
    };
    index <= marker
        && matches!(
            field.code,
            5 | 100 | 102 | 280 | 330 | 340 | 350 | 360 | 361 | 390
        )
}

fn serialize_layouts(
    layouts: &crate::LayoutCollection,
    existing: &[String],
    handle: &mut u64,
) -> String {
    let names = layouts
        .names()
        .filter(|name| !existing.iter().any(|item| item.eq_ignore_ascii_case(name)))
        .collect::<Vec<_>>();
    if names.is_empty() || (names.len() == 1 && names[0].eq_ignore_ascii_case("Model")) {
        return String::new();
    }
    names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let layout = layouts.get(name)?;
            let result = serialize_layout_record(name, layout, *handle, index, "\n");
            *handle = handle.saturating_add(1);
            Some(result)
        })
        .collect()
}

fn serialize_layout_record(
    name: &str,
    layout: &crate::Layout,
    handle: u64,
    index: usize,
    newline: &str,
) -> String {
    let setup = layout.setup();
    let paper = setup.paper();
    let margins = setup.margins();
    let (numerator, denominator) = (setup.scale(), 1.0);
    let rotation = matches!(setup.orientation(), crate::Orientation::Landscape) as u8;
    let flags = setup.centered() as u8 * 4 + setup.fit_to_paper() as u8 * 16;
    let (area, window) = match setup.plot_area() {
        crate::PlotArea::Extents => (1, [0.0; 4]),
        crate::PlotArea::Window(bounds) => (4, bounds),
        crate::PlotArea::Layout => (5, [0.0; 4]),
    };
    format!(
        "0{newline}LAYOUT{newline}5{newline}{handle:X}{newline}100{newline}AcDbPlotSettings{newline}40{newline}{}{newline}41{newline}{}{newline}42{newline}{}{newline}43{newline}{}{newline}44{newline}{}{newline}45{newline}{}{newline}46{newline}{}{newline}47{newline}{}{newline}48{newline}{}{newline}49{newline}{}{newline}140{newline}{}{newline}141{newline}{}{newline}70{newline}{flags}{newline}73{newline}{rotation}{newline}74{newline}{area}{newline}75{newline}{}{newline}72{newline}1{newline}142{newline}{numerator}{newline}143{newline}{denominator}{newline}147{newline}1.0{newline}100{newline}AcDbLayout{newline}1{newline}{name}{newline}70{newline}0{newline}71{newline}{index}{newline}",
        margins[0],
        margins[1],
        margins[2],
        margins[3],
        paper[0],
        paper[1],
        setup.plot_offset()[0],
        setup.plot_offset()[1],
        window[0],
        window[1],
        window[2],
        window[3],
        if setup.fit_to_paper() { 0 } else { 1 },
    )
}

fn layout_record_spans(source: &str) -> Vec<(String, usize, usize, u64)> {
    let mut groups = Groups::new(source);
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
        if marker.code != 0 || marker.value != "LAYOUT" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(source.len(), |next| next.start);
        let layout_fields = subclass_fields(&fields, "AcDbLayout").unwrap_or(&fields);
        let Some(name) = layout_fields
            .iter()
            .find(|field| field.code == 1)
            .map(|field| field.value.to_owned())
        else {
            continue;
        };
        let handle = fields
            .iter()
            .find(|field| field.code == 5)
            .and_then(|field| u64::from_str_radix(field.value.trim(), 16).ok())
            .unwrap_or(0);
        spans.push((name, marker.start, end, handle));
    }
    spans
}

fn layout_record_matches(
    source: &str,
    start: usize,
    end: usize,
    name: &str,
    layout: &crate::Layout,
) -> bool {
    let Some(record) = source.get(start..end) else {
        return false;
    };
    let mut groups = Groups::new(record);
    let Ok(Some(_)) = groups.next() else {
        return false;
    };
    let Ok(fields) = record_fields(&mut groups) else {
        return false;
    };
    let layout_fields = subclass_fields(&fields, "AcDbLayout").unwrap_or(&fields);
    let plot_fields = subclass_fields(&fields, "AcDbPlotSettings").unwrap_or(&fields);
    let source_name = layout_fields
        .iter()
        .find(|field| field.code == 1)
        .map(|field| field.value);
    source_name == Some(name)
        && parse_native_page_setup(plot_fields, 0).is_some_and(|setup| setup == layout.setup())
}

fn patch_layout_record(
    source: &str,
    start: usize,
    end: usize,
    name: &str,
    layout: &crate::Layout,
    changes: &mut Vec<(usize, usize, String)>,
) {
    let Some(record) = source.get(start..end) else {
        return;
    };
    let mut groups = Groups::new(record);
    let Ok(Some(_)) = groups.next() else {
        return;
    };
    let Ok(fields) = record_fields(&mut groups) else {
        return;
    };
    let newline = if record.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    patch_layout_plot_settings(&fields, start, newline, layout, changes);

    patch_layout_fields(
        &fields,
        "AcDbLayout",
        &[(1, name.to_owned())],
        start,
        newline,
        changes,
    );
}

fn patch_layout_plot_settings(
    fields: &[Group<'_>],
    record_start: usize,
    newline: &str,
    layout: &crate::Layout,
    changes: &mut Vec<(usize, usize, String)>,
) {
    let source_plot = subclass_fields(fields, "AcDbPlotSettings");
    let setup_changed = source_plot
        .and_then(|plot_fields| parse_native_page_setup(plot_fields, 0))
        .is_none_or(|source_setup| source_setup != layout.setup());
    if !setup_changed {
        return;
    }

    let setup = layout.setup();
    let paper = setup.paper();
    let margins = setup.margins();
    let window = match setup.plot_area() {
        crate::PlotArea::Window(bounds) => bounds,
        _ => [0.0; 4],
    };
    let source_flags = source_plot
        .and_then(|plot| plot.iter().find(|field| field.code == 70))
        .and_then(|field| field.value.parse::<i64>().ok())
        .unwrap_or(0);
    let centered_flag = i64::from(u8::from(setup.centered())) * 4;
    let fit_to_paper_flag = i64::from(u8::from(setup.fit_to_paper())) * 16;
    let modeled_flags = centered_flag + fit_to_paper_flag;
    let flags = (source_flags & !20) | modeled_flags;
    let area = match setup.plot_area() {
        crate::PlotArea::Extents => 1,
        crate::PlotArea::Window(_) => 4,
        crate::PlotArea::Layout => 5,
    };
    let values = [
        (40, margins[0].to_string()),
        (41, margins[1].to_string()),
        (42, margins[2].to_string()),
        (43, margins[3].to_string()),
        (44, paper[0].to_string()),
        (45, paper[1].to_string()),
        (46, setup.plot_offset()[0].to_string()),
        (47, setup.plot_offset()[1].to_string()),
        (48, window[0].to_string()),
        (49, window[1].to_string()),
        (140, window[2].to_string()),
        (141, window[3].to_string()),
        (70, flags.to_string()),
        (
            73,
            u8::from(matches!(setup.orientation(), crate::Orientation::Landscape)).to_string(),
        ),
        (74, area.to_string()),
        (75, u8::from(setup.fit_to_paper()).to_string()),
        (72, "1".to_owned()),
        (142, setup.scale().to_string()),
        (143, "1.0".to_owned()),
        (147, "1.0".to_owned()),
    ];
    patch_layout_fields(
        fields,
        "AcDbPlotSettings",
        &values,
        record_start,
        newline,
        changes,
    );
}

fn patch_layout_fields(
    all_fields: &[Group<'_>],
    subclass: &str,
    values: &[(u16, String)],
    record_start: usize,
    newline: &str,
    changes: &mut Vec<(usize, usize, String)>,
) {
    let fields = subclass_fields(all_fields, subclass).unwrap_or(all_fields);
    let mut missing = Vec::new();
    for (code, value) in values {
        if let Some(field) = fields.iter().find(|field| field.code == *code) {
            changes.push((
                record_start + field.value_start,
                record_start + field.value_end,
                value.clone(),
            ));
        } else {
            missing.push(format!("{}{newline}{}{newline}", code, value));
        }
    }
    if missing.is_empty() {
        return;
    }
    let existing_marker = all_fields
        .iter()
        .find(|field| field.code == 100 && field.value == subclass);
    let (insertion, prefix) = if let Some(marker) = existing_marker {
        (
            record_start + marker.value_end + newline.len(),
            String::new(),
        )
    } else if subclass == "AcDbPlotSettings" {
        if let Some(layout_marker) = all_fields
            .iter()
            .find(|field| field.code == 100 && field.value == "AcDbLayout")
        {
            (
                record_start + layout_marker.start,
                format!("100{newline}AcDbPlotSettings{newline}"),
            )
        } else {
            append_layout_fields_position(all_fields, record_start, newline)
        }
    } else {
        append_layout_fields_position(all_fields, record_start, newline)
    };
    changes.push((
        insertion,
        insertion,
        format!("{prefix}{}", missing.into_iter().collect::<String>()),
    ));
}

fn append_layout_fields_position(
    all_fields: &[Group<'_>],
    record_start: usize,
    newline: &str,
) -> (usize, String) {
    let insertion = all_fields.last().map_or(record_start, |field| {
        record_start + field.value_end + newline.len()
    });
    (insertion, String::new())
}

fn patch_layout_owned_records(
    source: &str,
    layouts: &crate::LayoutCollection,
    mapping: &[(&str, &str)],
    removed: &HashSet<String>,
    changes: &mut Vec<(usize, usize, String)>,
) {
    let mut groups = Groups::new(source);
    let clip_handles = viewport_clip_handles(source);
    while let Ok(Some(marker)) = groups.next() {
        if marker.code != 0
            || marker.value == "ENDSEC"
            || !matches!(marker.value, "VIEWPORT" | "LWPOLYLINE")
        {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(source.len(), |next| next.start);
        let Some(layout) = fields
            .iter()
            .find(|field| field.code == 410)
            .map(|field| field.value)
        else {
            continue;
        };
        if marker.value == "LWPOLYLINE"
            && !fields
                .iter()
                .find(|field| field.code == 5)
                .is_some_and(|field| clip_handles.contains(field.value))
        {
            continue;
        }
        if removed.iter().any(|name| name.eq_ignore_ascii_case(layout))
            && canonical_layout_name(layouts, layout).is_none()
        {
            changes.push((marker.start, end, String::new()));
            continue;
        }
        let Some((_, replacement)) = mapping
            .iter()
            .find(|(source_name, _)| source_name.eq_ignore_ascii_case(layout))
        else {
            continue;
        };
        if replacement.eq_ignore_ascii_case(layout) {
            continue;
        }
        if let Some(field) = fields.iter().find(|field| field.code == 410) {
            changes.push((
                field.value_start,
                field.value_end,
                (*replacement).to_owned(),
            ));
        }
    }
}

fn patch_existing_viewports(
    source: &str,
    layouts: &crate::LayoutCollection,
    mapping: &[(&str, &str)],
    changes: &mut Vec<(usize, usize, String)>,
) {
    let mut groups = Groups::new(source);
    let mut viewport_indices: HashMap<String, usize> = HashMap::new();
    let fallback_source_name = source_layout_names(source)
        .into_iter()
        .find(|name| !name.eq_ignore_ascii_case("Model"));
    while let Ok(Some(marker)) = groups.next() {
        if marker.code != 0 || marker.value != "VIEWPORT" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        if is_overall_paper_viewport(&fields) {
            continue;
        }
        let source_name = fields
            .iter()
            .find(|field| field.code == 410)
            .map(|field| field.value)
            .or(fallback_source_name.as_deref());
        let Some(source_name) = source_name else {
            continue;
        };
        let current_name = mapping
            .iter()
            .find(|(original, _)| original.eq_ignore_ascii_case(source_name))
            .map_or(source_name, |(_, current)| *current);
        let viewport_index = viewport_indices
            .entry(current_name.to_ascii_lowercase())
            .or_insert(0);
        let current_index = *viewport_index;
        *viewport_index += 1;
        let Some(viewport) = layouts
            .get(current_name)
            .and_then(|layout| layout.viewports().get(current_index))
        else {
            continue;
        };
        let record_end = groups
            .peek()
            .ok()
            .flatten()
            .map_or(source.len(), |next| next.start);
        let newline = if source[marker.start..record_end].contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        patch_existing_viewport_fields(&fields, record_end, newline, viewport, changes);
    }
}

fn patch_existing_viewport_fields(
    fields: &[Group<'_>],
    record_end: usize,
    newline: &str,
    viewport: &crate::Viewport,
    changes: &mut Vec<(usize, usize, String)>,
) {
    let paper = viewport.paper();
    let width = paper[2] - paper[0];
    let height = paper[3] - paper[1];
    let world = viewport.world();
    let world_width = (width / viewport.scale()).max(f64::EPSILON);
    let world_height = (height / viewport.scale()).max(f64::EPSILON);
    let values = [
        (10, ((paper[0] + paper[2]) / 2.0).to_string()),
        (20, ((paper[1] + paper[3]) / 2.0).to_string()),
        (40, width.to_string()),
        (41, height.to_string()),
        (12, (world[0] + world_width / 2.0).to_string()),
        (22, (world[1] + world_height / 2.0).to_string()),
        (45, world_height.to_string()),
    ];
    let mut missing = String::new();
    for (code, value) in values {
        let Some(field) = fields.iter().find(|field| field.code == code) else {
            missing.push_str(&format!("{code}{newline}{value}{newline}"));
            continue;
        };
        if numeric_values_equal(field.value, &value) {
            continue;
        }
        changes.push((field.value_start, field.value_end, value));
    }
    if missing.is_empty() {
        return;
    }
    let insertion = fields
        .iter()
        .find(|field| field.code == 100 && field.value == "AcDbViewport")
        .map_or(record_end, |marker| marker.value_end + newline.len());
    changes.push((insertion, insertion, missing));
}

fn numeric_values_equal(left: &str, right: &str) -> bool {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) if left.is_finite() && right.is_finite() => left == right,
        _ => left == right,
    }
}

fn source_layout_names(source: &str) -> Vec<String> {
    let mut groups = Groups::new(source);
    let mut objects = false;
    let mut names = Vec::new();
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
        if marker.code != 0 || marker.value != "LAYOUT" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let layout_fields = subclass_fields(&fields, "AcDbLayout").unwrap_or(&fields);
        if let Some(name) = layout_fields
            .iter()
            .find(|field| field.code == 1)
            .map(|field| field.value.to_owned())
        {
            names.push(name);
        }
    }
    names
}

fn read_active_layout(source: &str) -> Option<String> {
    let mut groups = Groups::new(source);
    let mut header = false;
    let mut ctab = None;
    let mut tilemode = None;
    while let Ok(Some(group)) = groups.next() {
        if group.code == 2 && group.value == "HEADER" {
            header = true;
            continue;
        }
        if !header {
            continue;
        }
        if group.code == 0 && group.value == "ENDSEC" {
            break;
        }
        if group.code != 9 {
            continue;
        }
        let Ok(Some(value)) = groups.next() else {
            break;
        };
        if value.code == 2 && group.value == "$CTAB" && !value.value.trim().is_empty() {
            ctab = Some(value.value.to_owned());
        } else if value.code == 70 && group.value == "$TILEMODE" {
            tilemode = value.value.trim().parse::<i32>().ok();
        }
    }
    if tilemode == Some(1) {
        Some("Model".to_owned())
    } else {
        ctab
    }
}

fn ensure_active_header(output: &mut String, active: &str) {
    let has_ctab = output.contains("\n$CTAB\n") || output.contains("\r\n$CTAB\r\n");
    let has_tilemode = output.contains("\n$TILEMODE\n") || output.contains("\r\n$TILEMODE\r\n");
    if has_ctab && has_tilemode {
        return;
    }
    let newline = if output.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let header_marker = format!("0{newline}SECTION{newline}2{newline}HEADER{newline}");
    let end_marker = format!("0{newline}ENDSEC{newline}");
    let Some(header_start) = output.find(&header_marker) else {
        return;
    };
    let Some(relative_end) = output[header_start + header_marker.len()..].find(&end_marker) else {
        return;
    };
    let at = header_start + header_marker.len() + relative_end;
    let mut fields = String::new();
    if !has_ctab {
        fields.push_str(&format!(
            "9{newline}$CTAB{newline}2{newline}{active}{newline}"
        ));
    }
    if !has_tilemode {
        let tilemode = if active.eq_ignore_ascii_case("Model") {
            1
        } else {
            0
        };
        fields.push_str(&format!(
            "9{newline}$TILEMODE{newline}70{newline}{tilemode}{newline}"
        ));
    }
    output.insert_str(at, &fields);
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

fn native_viewport_id(fields: &[Group<'_>]) -> Option<u64> {
    fields
        .iter()
        .find(|field| field.code == 69)
        .and_then(|field| field.value.parse::<u64>().ok())
}

fn is_overall_paper_viewport(fields: &[Group<'_>]) -> bool {
    native_viewport_id(fields) == Some(1)
}

fn allocate_native_viewport_id(used_ids: &mut Vec<u64>) -> Option<u64> {
    let mut candidate = 2_u64;
    loop {
        if !used_ids.contains(&candidate) {
            used_ids.push(candidate);
            return Some(candidate);
        }
        candidate = candidate.checked_add(1)?;
    }
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
        if is_overall_paper_viewport(&fields) {
            continue;
        }
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
    source: &str,
    current_layout_names: &[String],
    handle: &mut u64,
    newline: &str,
) -> String {
    let existing_states = source_viewport_states(source, current_layout_names);
    let mut output = String::new();
    for name in layouts.names().filter(|name| *name != "Model") {
        let Some(layout) = layouts.get(name) else {
            continue;
        };
        let (existing_count, used_ids) = existing_states
            .iter()
            .find(|(source_name, _, _)| source_name.eq_ignore_ascii_case(name))
            .map_or((0, Vec::new()), |(_, count, ids)| (*count, ids.clone()));
        let mut used_ids = used_ids;
        for viewport in layout.viewports().iter().skip(existing_count) {
            let Some(native_id) = allocate_native_viewport_id(&mut used_ids) else {
                continue;
            };
            let world = viewport.world();
            let paper = viewport.paper();
            let width = paper[2] - paper[0];
            let height = paper[3] - paper[1];
            let world_width = (width / viewport.scale()).max(f64::EPSILON);
            let world_height = (height / viewport.scale()).max(f64::EPSILON);
            let paper_cx = (paper[0] + paper[2]) / 2.0;
            let paper_cy = (paper[1] + paper[3]) / 2.0;
            // VIEWPORT stores the visible world span, while CHSPACE uses the
            // lower-left world origin and the affine scale.  Derive the
            // serialized center from that origin and visible span so a
            // non-fitting viewport keeps its transform after reopening.
            let world_cx = world[0] + world_width / 2.0;
            let world_cy = world[1] + world_height / 2.0;
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
                "0{newline}VIEWPORT{newline}5{newline}{:X}{newline}100{newline}AcDbEntity{newline}8{newline}0{newline}67{newline}1{newline}410{newline}{name}{newline}100{newline}AcDbViewport{newline}10{newline}{paper_cx}{newline}20{newline}{paper_cy}{newline}40{newline}{width}{newline}41{newline}{height}{newline}12{newline}{world_cx}{newline}22{newline}{world_cy}{newline}45{newline}{world_height}{newline}{clip_fields}69{newline}{}{newline}",
                *handle,
                native_id,
                clip_fields = clip_fields,
            ));
            *handle = handle.saturating_add(1);
        }
    }
    output
}

fn source_viewport_states(
    source: &str,
    current_layout_names: &[String],
) -> Vec<(String, usize, Vec<u64>)> {
    let mut groups = Groups::new(source);
    let fallback = source_layout_names(source)
        .into_iter()
        .find(|name| !name.eq_ignore_ascii_case("Model"));
    let source_layouts = layout_record_spans(source)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .collect::<Vec<_>>();
    let mut states: Vec<(String, usize, Vec<u64>)> = Vec::new();
    while let Ok(Some(marker)) = groups.next() {
        if marker.code != 0 || marker.value != "VIEWPORT" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        if is_overall_paper_viewport(&fields) {
            continue;
        }
        let native_id = native_viewport_id(&fields);
        let source_name = fields
            .iter()
            .find(|field| field.code == 410)
            .map(|field| field.value.to_owned())
            .or_else(|| fallback.clone());
        let Some(source_name) = source_name else {
            continue;
        };
        let name = source_layouts
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(&source_name))
            .and_then(|index| current_layout_names.get(index).cloned())
            .unwrap_or(source_name);
        if let Some((_, count, ids)) = states
            .iter_mut()
            .find(|(existing, _, _)| existing.eq_ignore_ascii_case(&name))
        {
            *count += 1;
            if let Some(native_id) = native_id {
                ids.push(native_id);
            }
        } else {
            states.push((name, 1, native_id.into_iter().collect()));
        }
    }
    states
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

fn record_field_text_at(input: &str, start: usize, code: u16) -> Option<String> {
    let mut groups = Groups::new(input.get(start..)?);
    let _ = groups.next().ok()??;
    record_fields(&mut groups)
        .ok()?
        .iter()
        .find(|field| field.code == code)
        .map(|field| field.value.to_owned())
}

fn entity_handle_insertion_offset(input: &str, start: usize) -> usize {
    let Some(rest) = input.get(start..) else {
        return start;
    };
    let Some(first_end) = rest.find('\n') else {
        return start;
    };
    let name_start = first_end + 1;
    let Some(name_end) = rest.get(name_start..).and_then(|value| value.find('\n')) else {
        return start + name_start;
    };
    start + name_start + name_end + 1
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

const COMMON_ENTITY_CODES: &[u16] = &[5, 8, 6, 62, 48, 67, 410];

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
    pdf_definitions: &HashMap<u64, (std::path::PathBuf, u32)>,
) -> Result<(Entity, EntityRecord), DxfError> {
    if marker.value == "PDFUNDERLAY" {
        return read_pdf_underlay(groups, marker, handles, pdf_definitions);
    }
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

fn read_pdf_underlay(
    groups: &mut Groups<'_>,
    marker: Group<'_>,
    handles: &mut HashSet<u64>,
    pdf_definitions: &HashMap<u64, (std::path::PathBuf, u32)>,
) -> Result<(Entity, EntityRecord), DxfError> {
    let source_fields = record_fields(groups)?;
    let properties = read_entity_properties(&source_fields, marker.line, handles)?;
    let (insertion, definition_handle) = read_pdf_reference(&source_fields, marker.line)?;
    let (path, page) = pdf_definitions.get(&definition_handle).ok_or(DxfError {
        line: marker.line,
        kind: DxfErrorKind::UnsupportedProperty(340),
    })?;
    let mut underlay =
        PdfUnderlay::unresolved(path.clone(), *page, insertion).map_err(|_| DxfError {
            line: marker.line,
            kind: DxfErrorKind::InvalidCoordinate(10),
        })?;
    configure_pdf_underlay(&mut underlay, &source_fields, marker.line)?;
    if let Some(clip) = read_pdf_clip(&source_fields, marker.line)? {
        underlay.set_page_clip(Some(clip)).map_err(|_| DxfError {
            line: marker.line,
            kind: DxfErrorKind::InvalidCoordinate(11),
        })?;
    }
    let entity = Entity::PdfUnderlay(underlay);
    let end = groups.peek()?.map_or(groups.offset, |group| group.start);
    let fields = source_fields
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
            fields,
            properties,
            polyline: None,
        },
    ))
}

fn read_pdf_reference(fields: &[Group<'_>], line: usize) -> Result<(Point3, u64), DxfError> {
    let insertion = Point3::new(
        field_number(fields, 10, line)?.ok_or(DxfError {
            line,
            kind: DxfErrorKind::MissingField(10),
        })?,
        field_number(fields, 20, line)?.ok_or(DxfError {
            line,
            kind: DxfErrorKind::MissingField(20),
        })?,
        field_number(fields, 30, line)?.unwrap_or(0.0),
    )
    .map_err(|_| DxfError {
        line,
        kind: DxfErrorKind::InvalidCoordinate(10),
    })?;
    let handle = unique_field(fields, 340)?.ok_or(DxfError {
        line,
        kind: DxfErrorKind::MissingField(340),
    })?;
    let handle = u64::from_str_radix(handle.value.trim(), 16).map_err(|_| DxfError {
        line: handle.line,
        kind: DxfErrorKind::InvalidHandle(340),
    })?;
    Ok((insertion, handle))
}

fn configure_pdf_underlay(
    underlay: &mut PdfUnderlay,
    fields: &[Group<'_>],
    line: usize,
) -> Result<(), DxfError> {
    validate_xy_normal(
        [
            field_number(fields, 210, line)?,
            field_number(fields, 220, line)?,
            field_number(fields, 230, line)?,
        ],
        line,
    )?;
    underlay
        .set_scale([
            field_number(fields, 41, line)?.unwrap_or(1.0),
            field_number(fields, 42, line)?.unwrap_or(1.0),
        ])
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::InvalidCoordinate(41),
        })?;
    underlay
        .set_rotation(field_number(fields, 50, line)?.unwrap_or(0.0).to_radians())
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::InvalidCoordinate(50),
        })?;
    let flags = field_number(fields, 280, line)?.unwrap_or(2.0);
    if !flags.is_finite() || flags.fract() != 0.0 || !(0.0..=f64::from(u8::MAX)).contains(&flags) {
        return Err(DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(280),
        });
    }
    let flags = flags as u8;
    underlay.set_clip_enabled(flags & 1 != 0);
    underlay.set_display_enabled(flags & 2 != 0);
    underlay.set_monochrome(flags & 4 != 0);
    underlay.set_frame_mode(1).map_err(|_| DxfError {
        line,
        kind: DxfErrorKind::UnsupportedProperty(280),
    })?;
    underlay
        .set_contrast(field_number(fields, 281, line)?.unwrap_or(100.0))
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(281),
        })?;
    underlay
        .set_fade(field_number(fields, 282, line)?.unwrap_or(0.0))
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(282),
        })?;
    Ok(())
}

fn read_pdf_clip(fields: &[Group<'_>], line: usize) -> Result<Option<Vec<[f64; 2]>>, DxfError> {
    let mut clip = Vec::new();
    for field in fields.iter().filter(|field| field.code == 11) {
        let x = field.value.parse::<f64>().map_err(|_| DxfError {
            line: field.line,
            kind: DxfErrorKind::InvalidCoordinate(11),
        })?;
        let y = fields
            .iter()
            .find(|candidate| candidate.code == 21 && candidate.start > field.start)
            .map(|candidate| {
                candidate.value.parse::<f64>().map_err(|_| DxfError {
                    line: candidate.line,
                    kind: DxfErrorKind::InvalidCoordinate(21),
                })
            })
            .transpose()?;
        let Some(y) = y else {
            return Err(DxfError {
                line,
                kind: DxfErrorKind::MissingField(21),
            });
        };
        if !x.is_finite() || !y.is_finite() {
            return Err(DxfError {
                line,
                kind: DxfErrorKind::InvalidCoordinate(11),
            });
        }
        clip.push([x, y]);
    }
    Ok((!clip.is_empty()).then_some(clip))
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
        Entity::PdfUnderlay(underlay) => {
            let insertion = underlay.insertion();
            let scale = underlay.scale();
            let mut flags = 0u8;
            if underlay.clip_enabled() {
                flags |= 1;
            }
            if underlay.display_enabled() {
                flags |= 2;
            }
            if underlay.monochrome() {
                flags |= 4;
            }
            writeln!(
                output,
                "340\n{:X}\n10\n{}\n20\n{}\n30\n{}\n41\n{}\n42\n{}\n43\n1\n50\n{}\n280\n{}\n281\n{}\n282\n{}",
                pdf_definition_handle(underlay),
                insertion.x(),
                insertion.y(),
                insertion.z(),
                scale[0],
                scale[1],
                underlay.rotation().to_degrees(),
                flags,
                underlay.contrast(),
                underlay.fade()
            )
            .expect("String write");
            if let Some(clip) = underlay.clip() {
                let page_size = underlay.resolved_page_size().ok();
                for point in clip {
                    let (x, y) = match page_size {
                        Some(size) => (point[0] * size[0], point[1] * size[1]),
                        None => (point[0], point[1]),
                    };
                    writeln!(output, "11\n{}\n21\n{}", x, y).expect("String write");
                }
            } else if let Some(clip) = underlay.pending_page_clip() {
                for point in clip {
                    writeln!(output, "11\n{}\n21\n{}", point[0], point[1]).expect("String write");
                }
            }
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

fn pdf_definition_handle(underlay: &PdfUnderlay) -> u64 {
    let mut hasher = DefaultHasher::new();
    underlay.path().to_string_lossy().hash(&mut hasher);
    underlay.page().hash(&mut hasher);
    0xB000_0000_0000_0000_u64 | (hasher.finish() & 0x0FFF_FFFF_FFFF_FFFF)
}

fn serialize_pdf_definition(underlay: &PdfUnderlay) -> String {
    format!(
        "0\nUNDERLAYDEFINITION\n5\n{:X}\n100\nAcDbUnderlayDefinition\n1\n{}\n2\n{}\n",
        pdf_definition_handle(underlay),
        underlay.path().display(),
        underlay.page()
    )
}

fn serialize_pdf_definitions(underlays: &[&PdfUnderlay], source: &str) -> String {
    let existing = read_pdf_definitions(source);
    let mut emitted = HashSet::new();
    underlays
        .iter()
        .filter(|underlay| {
            let handle = pdf_definition_handle(underlay);
            emitted.insert(handle) && !existing.contains_key(&handle)
        })
        .map(|underlay| serialize_pdf_definition(underlay))
        .collect()
}

fn read_pdf_definitions(input: &str) -> HashMap<u64, (std::path::PathBuf, u32)> {
    let mut definitions = HashMap::new();
    let mut groups = Groups::new(input);
    let mut objects = false;
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
        if marker.code != 0 || marker.value != "UNDERLAYDEFINITION" {
            continue;
        }
        let Ok(fields) = record_fields(&mut groups) else {
            break;
        };
        let Some(handle) = fields
            .iter()
            .find(|field| field.code == 5)
            .and_then(|field| u64::from_str_radix(field.value.trim(), 16).ok())
        else {
            continue;
        };
        let Some(path) = fields
            .iter()
            .find(|field| field.code == 1)
            .map(|field| std::path::PathBuf::from(field.value))
        else {
            continue;
        };
        let Some(page) = fields
            .iter()
            .find(|field| field.code == 2)
            .and_then(|field| field.value.parse::<u32>().ok())
            .filter(|value| *value > 0)
        else {
            continue;
        };
        definitions.insert(handle, (path, page));
    }
    definitions
}

fn dynamic_parse_type(kind: &str, spec: &str) -> Option<DynamicParameterType> {
    match kind {
        "D" | "A" => {
            let values = spec
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 2 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            let min = values[0];
            let max = values[1];
            if kind == "D" {
                Some(DynamicParameterType::Distance { min, max })
            } else {
                Some(DynamicParameterType::Angle { min, max })
            }
        }
        "P" => {
            let (min, max) = spec.split_once(';')?;
            Some(DynamicParameterType::Point {
                min: {
                    let point = dynamic_point_parse(min)?;
                    [point.x(), point.y(), point.z()]
                },
                max: {
                    let point = dynamic_point_parse(max)?;
                    [point.x(), point.y(), point.z()]
                },
            })
        }
        "X" => {
            let values = spec
                .split([',', ';'])
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 4 {
                return None;
            }
            Some(DynamicParameterType::XY {
                min: [values[0], values[1]],
                max: [values[2], values[3]],
            })
        }
        "O" => {
            let values = spec
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 4 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(DynamicParameterType::Polar {
                distance_min: values[0],
                distance_max: values[1],
                angle_min: values[2],
                angle_max: values[3],
            })
        }
        "L" => {
            let mut parts = spec.splitn(3, ';');
            let reverse = parts.next()?;
            let input_parameters = parts
                .next()?
                .split(',')
                .filter(|name| !name.is_empty())
                .map(dynamic_decode)
                .collect::<Option<Vec<_>>>()?;
            let rows = parts.next()?;
            let reverse = match reverse {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            let rows = rows
                .split('~')
                .map(|row| {
                    let mut parts = row.splitn(3, ':');
                    let input = parts.next()?;
                    let label = parts.next()?;
                    let assignments = parts.next()?;
                    let input = input.parse::<f64>().ok()?;
                    let label = dynamic_decode(label)?;
                    let assignments = assignments
                        .split(',')
                        .filter(|assignment| !assignment.is_empty())
                        .map(|assignment| {
                            let (name, value) = assignment.split_once('=')?;
                            Some(LookupAssignment::new(
                                dynamic_decode(name)?,
                                dynamic_value_parse(&dynamic_decode(value)?)?,
                            ))
                        })
                        .collect::<Option<Vec<_>>>()?;
                    LookupRow::new(input, label)
                        .ok()?
                        .with_assignments(assignments)
                        .ok()
                })
                .collect::<Option<Vec<_>>>()?;
            Some(DynamicParameterType::Lookup {
                rows,
                reverse,
                input_parameters,
            })
        }
        "G" => {
            let values = spec
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 8 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(DynamicParameterType::Alignment {
                point_min: [values[0], values[1], values[2]],
                point_max: [values[3], values[4], values[5]],
                angle_min: values[6],
                angle_max: values[7],
            })
        }
        "Q" => {
            let values = spec
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 6 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(DynamicParameterType::BasePoint {
                min: [values[0], values[1], values[2]],
                max: [values[3], values[4], values[5]],
            })
        }
        "F" => Some(DynamicParameterType::Flip),
        "V" => Some(DynamicParameterType::Visibility {
            states: spec
                .split(',')
                .map(dynamic_decode)
                .collect::<Option<Vec<_>>>()?,
        }),
        _ => None,
    }
}

fn dynamic_parse_action(kind: &str, fields: &[&str]) -> Option<DynamicAction> {
    let decode_parameter = |value: &str| dynamic_decode(value);
    match kind {
        "MOVE" => Some(DynamicAction::Move {
            parameter: decode_parameter(fields.first()?)?,
            origin: dynamic_point_parse(fields.get(1)?)?,
            direction: dynamic_point_parse(fields.get(2)?)?,
        }),
        "SCALE" => Some(DynamicAction::Scale {
            parameter: decode_parameter(fields.first()?)?,
            center: dynamic_point_parse(fields.get(1)?)?,
        }),
        "ROTATE" => Some(DynamicAction::Rotate {
            parameter: decode_parameter(fields.first()?)?,
            center: dynamic_point_parse(fields.get(1)?)?,
        }),
        "FLIP" => Some(DynamicAction::Flip {
            parameter: decode_parameter(fields.first()?)?,
            first: dynamic_point_parse(fields.get(1)?)?,
            second: dynamic_point_parse(fields.get(2)?)?,
        }),
        "STRETCH" => Some(DynamicAction::Stretch {
            parameter: decode_parameter(fields.first()?)?,
            min: dynamic_point_parse(fields.get(1)?)?,
            max: dynamic_point_parse(fields.get(2)?)?,
            direction: dynamic_point_parse(fields.get(3)?)?,
        }),
        "POLARSTRETCH" if fields.len() == 5 => Some(DynamicAction::PolarStretch {
            parameter: decode_parameter(fields.first()?)?,
            center: dynamic_point_parse(fields.get(1)?)?,
            min: dynamic_point_parse(fields.get(2)?)?,
            max: dynamic_point_parse(fields.get(3)?)?,
            rotate_only_targets: dynamic_ids_parse(fields.get(4)?)?,
        }),
        "ARRAY" => Some(DynamicAction::Array {
            parameter: match *fields.first()? {
                "" => None,
                value => Some(decode_parameter(value)?),
            },
            rows: fields.get(1)?.parse().ok()?,
            columns: fields.get(2)?.parse().ok()?,
            row_spacing: fields.get(3)?.parse().ok()?,
            column_spacing: fields.get(4)?.parse().ok()?,
        }),
        _ => None,
    }
}

fn dynamic_error() -> DxfError {
    DxfError {
        line: 0,
        kind: DxfErrorKind::UnsupportedProperty(1000),
    }
}

#[derive(Clone, Debug)]
enum DynamicOverrideRecord {
    Entity {
        handle: u64,
        value: Option<(String, DynamicValue)>,
        visibility: Option<String>,
    },
    Block {
        block: String,
        entity_id: u64,
        value: Option<(String, DynamicValue)>,
        visibility: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct DynamicValueSetRecord {
    block: String,
    parameter: String,
    component: DynamicValueComponent,
    value_set: DynamicValueSet,
}

#[derive(Clone, Debug)]
struct DynamicActionOptionsRecord {
    block: String,
    action_index: usize,
    options: DynamicActionOptions,
}

fn collect_dynamic_payloads(input: &str) -> Result<Vec<String>, DxfError> {
    let mut groups = Groups::new(input);
    let mut in_objects = false;
    let mut payloads = Vec::new();
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
        if marker.code != 0 || marker.value != "XRECORD" {
            continue;
        }
        for field in record_fields(&mut groups)? {
            if let Some(payload) = field.value.strip_prefix("OpenCadData|v1|DYNAMIC|") {
                payloads.push(payload.to_owned());
            }
        }
    }
    Ok(payloads)
}

fn read_dynamic_metadata(
    input: &str,
    blocks: &mut [BlockDefinition],
    entities: &mut [Entity],
    records: &[Option<EntityRecord>],
) -> Result<(), DxfError> {
    let payloads = collect_dynamic_payloads(input)?;
    let mut definitions: HashMap<String, DynamicBlockDefinition> = HashMap::new();
    let mut entity_ids: HashMap<String, Vec<u64>> = HashMap::new();
    let mut overrides: Vec<DynamicOverrideRecord> = Vec::new();
    let mut value_sets: Vec<DynamicValueSetRecord> = Vec::new();
    let mut action_options: Vec<DynamicActionOptionsRecord> = Vec::new();
    for payload in payloads {
        parse_dynamic_payload(
            payload.as_str(),
            &mut definitions,
            &mut entity_ids,
            &mut overrides,
            &mut value_sets,
            &mut action_options,
        )?;
    }
    resolve_dynamic_definitions(blocks, &mut definitions, &mut entity_ids)?;
    apply_dynamic_value_sets(blocks, value_sets)?;
    apply_dynamic_action_options(blocks, action_options)?;
    apply_dynamic_overrides(input, blocks, entities, records, overrides)
}

fn resolve_dynamic_definitions(
    blocks: &mut [BlockDefinition],
    definitions: &mut HashMap<String, DynamicBlockDefinition>,
    entity_ids: &mut HashMap<String, Vec<u64>>,
) -> Result<(), DxfError> {
    for block in blocks.iter_mut() {
        if let Some(ids) = entity_ids.remove(block.name()) {
            if ids.len() != block.entities().len()
                || ids.contains(&0)
                || ids
                    .iter()
                    .enumerate()
                    .any(|(index, id)| ids[..index].contains(id))
            {
                return Err(dynamic_error());
            }
            let entities = block.entities().to_vec();
            block
                .set_entities_with_ids(entities, ids)
                .map_err(|_| dynamic_error())?;
        }
        if let Some(definition) = definitions.remove(block.name()) {
            definition
                .validate(block.entity_ids())
                .map_err(|_| dynamic_error())?;
            block.set_dynamic_definition(Some(definition));
        }
    }
    if !definitions.is_empty() || !entity_ids.is_empty() {
        return Err(dynamic_error());
    }
    Ok(())
}

fn parse_dynamic_value_set(fields: &[&str]) -> Result<DynamicValueSetRecord, DxfError> {
    if fields.len() != 6 {
        return Err(dynamic_error());
    }
    let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
    let parameter = dynamic_decode(fields[2]).ok_or_else(dynamic_error)?;
    let component = match fields[3] {
        "D" => DynamicValueComponent::Distance,
        "A" => DynamicValueComponent::Angle,
        "X" => DynamicValueComponent::X,
        "Y" => DynamicValueComponent::Y,
        _ => return Err(dynamic_error()),
    };
    let values = fields[5]
        .split(',')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| dynamic_error())?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(dynamic_error());
    }
    let value_set = match fields[4] {
        "I" if values.len() == 2 => DynamicValueSet::Increment {
            origin: values[0],
            step: values[1],
        },
        "L" if !values.is_empty() => DynamicValueSet::List(values),
        _ => return Err(dynamic_error()),
    };
    Ok(DynamicValueSetRecord {
        block,
        parameter,
        component,
        value_set,
    })
}

fn parse_dynamic_action_options(fields: &[&str]) -> Result<DynamicActionOptionsRecord, DxfError> {
    if fields.len() != 6 {
        return Err(dynamic_error());
    }
    let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
    let action_index = fields[2].parse::<usize>().map_err(|_| dynamic_error())?;
    let distance_multiplier = fields[3].parse::<f64>().map_err(|_| dynamic_error())?;
    let angle_offset = fields[4].parse::<f64>().map_err(|_| dynamic_error())?;
    if !distance_multiplier.is_finite() || !angle_offset.is_finite() {
        return Err(dynamic_error());
    }
    let axis = match fields[5] {
        "X" => DynamicActionAxis::X,
        "Y" => DynamicActionAxis::Y,
        "XY" => DynamicActionAxis::XY,
        _ => return Err(dynamic_error()),
    };
    Ok(DynamicActionOptionsRecord {
        block,
        action_index,
        options: DynamicActionOptions {
            distance_multiplier,
            angle_offset,
            axis,
        },
    })
}

fn apply_dynamic_value_sets(
    blocks: &mut [BlockDefinition],
    records: Vec<DynamicValueSetRecord>,
) -> Result<(), DxfError> {
    let mut grouped: HashMap<String, Vec<DynamicValueSetRecord>> = HashMap::new();
    for record in records {
        grouped
            .entry(record.block.clone())
            .or_default()
            .push(record);
    }
    for (block_name, records) in grouped {
        let block_index = blocks
            .iter()
            .position(|block| block.name() == block_name)
            .ok_or_else(dynamic_error)?;
        let Some(definition) = blocks[block_index].dynamic_definition().cloned() else {
            return Err(dynamic_error());
        };
        let mut replacement = definition.clone();
        for record in records {
            replacement
                .set_parameter_value_set(&record.parameter, record.component, record.value_set)
                .map_err(|_| dynamic_error())?;
        }
        replacement
            .validate(blocks[block_index].entity_ids())
            .map_err(|_| dynamic_error())?;
        blocks[block_index].set_dynamic_definition(Some(replacement));
    }
    Ok(())
}

fn apply_dynamic_action_options(
    blocks: &mut [BlockDefinition],
    records: Vec<DynamicActionOptionsRecord>,
) -> Result<(), DxfError> {
    let mut grouped: HashMap<String, Vec<DynamicActionOptionsRecord>> = HashMap::new();
    for record in records {
        grouped
            .entry(record.block.clone())
            .or_default()
            .push(record);
    }
    for (block_name, records) in grouped {
        let block_index = blocks
            .iter()
            .position(|block| block.name() == block_name)
            .ok_or_else(dynamic_error)?;
        let Some(definition) = blocks[block_index].dynamic_definition().cloned() else {
            return Err(dynamic_error());
        };
        let mut replacement = definition;
        for record in records {
            replacement
                .set_action_options(record.action_index, record.options)
                .map_err(|_| dynamic_error())?;
        }
        replacement
            .validate(blocks[block_index].entity_ids())
            .map_err(|_| dynamic_error())?;
        blocks[block_index].set_dynamic_definition(Some(replacement));
    }
    Ok(())
}

fn parse_dynamic_payload(
    payload: &str,
    definitions: &mut HashMap<String, DynamicBlockDefinition>,
    entity_ids: &mut HashMap<String, Vec<u64>>,
    overrides: &mut Vec<DynamicOverrideRecord>,
    value_sets: &mut Vec<DynamicValueSetRecord>,
    action_options: &mut Vec<DynamicActionOptionsRecord>,
) -> Result<(), DxfError> {
    let fields: Vec<_> = payload.split('|').collect();
    let Some(kind) = fields.first() else {
        return Err(dynamic_error());
    };
    match *kind {
        "IDS" if fields.len() == 3 => {
            let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
            let ids = dynamic_ids_parse(fields[2]).ok_or_else(dynamic_error)?;
            if entity_ids.insert(block, ids).is_some() {
                return Err(dynamic_error());
            }
        }
        "PARAM" => {
            if fields.len() != 6 {
                return Err(dynamic_error());
            }
            let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
            let name = dynamic_decode(fields[2]).ok_or_else(dynamic_error)?;
            let parameter_type =
                dynamic_parse_type(fields[3], fields[4]).ok_or_else(dynamic_error)?;
            let default = dynamic_value_parse(fields[5]).ok_or_else(dynamic_error)?;
            let parameter = DynamicParameter::new(name, parameter_type, default)
                .map_err(|_| dynamic_error())?;
            definitions
                .entry(block)
                .or_default()
                .add_parameter(parameter)
                .map_err(|_| dynamic_error())?;
        }
        "VALUESET" => {
            let record = parse_dynamic_value_set(&fields)?;
            if value_sets.iter().any(|existing| {
                existing.block == record.block
                    && existing.parameter == record.parameter
                    && existing.component == record.component
            }) {
                return Err(dynamic_error());
            }
            value_sets.push(record);
        }
        "ACTION" => {
            let expected_len = match fields.get(2).copied() {
                Some("MOVE") | Some("FLIP") => 7,
                Some("SCALE") | Some("ROTATE") => 6,
                Some("STRETCH") => 8,
                Some("ARRAY") => 9,
                Some("POLARSTRETCH") => 9,
                _ => return Err(dynamic_error()),
            };
            if fields.len() != expected_len {
                return Err(dynamic_error());
            }
            let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
            let action =
                dynamic_parse_action(fields[2], &fields[3..fields.len().saturating_sub(1)])
                    .ok_or_else(dynamic_error)?;
            let targets = dynamic_ids_parse(fields.last().copied().unwrap_or(""))
                .ok_or_else(dynamic_error)?;
            let definition = definitions.entry(block).or_default();
            definition.add_action(action).map_err(|_| dynamic_error())?;
            definition
                .set_action_targets(definition.actions().len() - 1, targets)
                .map_err(|_| dynamic_error())?;
        }
        "ACTIONOPTIONS" => {
            let record = parse_dynamic_action_options(&fields)?;
            if action_options.iter().any(|existing| {
                existing.block == record.block && existing.action_index == record.action_index
            }) {
                return Err(dynamic_error());
            }
            action_options.push(record);
        }
        "VIS" if fields.len() == 4 => {
            let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
            let name = dynamic_decode(fields[2]).ok_or_else(dynamic_error)?;
            let ids = dynamic_ids_parse(fields[3]).ok_or_else(dynamic_error)?;
            definitions
                .entry(block)
                .or_default()
                .add_visibility_state(
                    DynamicVisibilityState::new(name, ids).map_err(|_| dynamic_error())?,
                )
                .map_err(|_| dynamic_error())?;
        }
        "OVERRIDE" if fields.len() == 4 => {
            let handle = u64::from_str_radix(fields[1], 16).map_err(|_| dynamic_error())?;
            if handle == 0 {
                return Err(dynamic_error());
            }
            let name = dynamic_decode(fields[2]).ok_or_else(dynamic_error)?;
            let value = dynamic_value_parse(fields[3]).ok_or_else(dynamic_error)?;
            overrides.push(DynamicOverrideRecord::Entity {
                handle,
                value: Some((name, value)),
                visibility: None,
            });
        }
        "OVIS" if fields.len() == 3 => {
            let handle = u64::from_str_radix(fields[1], 16).map_err(|_| dynamic_error())?;
            if handle == 0 {
                return Err(dynamic_error());
            }
            let name = dynamic_decode(fields[2]).ok_or_else(dynamic_error)?;
            overrides.push(DynamicOverrideRecord::Entity {
                handle,
                value: None,
                visibility: Some(name),
            });
        }
        "BOVERRIDE" if fields.len() == 5 => {
            let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
            let entity_id = fields[2].parse().map_err(|_| dynamic_error())?;
            if entity_id == 0 {
                return Err(dynamic_error());
            }
            let name = dynamic_decode(fields[3]).ok_or_else(dynamic_error)?;
            let value = dynamic_value_parse(fields[4]).ok_or_else(dynamic_error)?;
            overrides.push(DynamicOverrideRecord::Block {
                block,
                entity_id,
                value: Some((name, value)),
                visibility: None,
            });
        }
        "BOVIS" if fields.len() == 4 => {
            let block = dynamic_decode(fields[1]).ok_or_else(dynamic_error)?;
            let entity_id = fields[2].parse().map_err(|_| dynamic_error())?;
            if entity_id == 0 {
                return Err(dynamic_error());
            }
            let name = dynamic_decode(fields[3]).ok_or_else(dynamic_error)?;
            overrides.push(DynamicOverrideRecord::Block {
                block,
                entity_id,
                value: None,
                visibility: Some(name),
            });
        }
        _ => return Err(dynamic_error()),
    }
    Ok(())
}

fn apply_dynamic_overrides(
    input: &str,
    blocks: &mut [BlockDefinition],
    entities: &mut [Entity],
    records: &[Option<EntityRecord>],
    overrides: Vec<DynamicOverrideRecord>,
) -> Result<(), DxfError> {
    let aggregated = collect_dynamic_overrides(overrides)?;
    apply_entity_dynamic_overrides(
        input,
        blocks,
        entities,
        records,
        aggregated.entity_overrides,
    )?;
    apply_block_dynamic_overrides(blocks, aggregated.block_overrides)
}

struct AggregatedDynamicOverrides {
    entity_overrides: HashMap<u64, DynamicBlockOverrides>,
    block_overrides: HashMap<(String, u64), DynamicBlockOverrides>,
}

fn collect_dynamic_overrides(
    overrides: Vec<DynamicOverrideRecord>,
) -> Result<AggregatedDynamicOverrides, DxfError> {
    let mut entity_overrides: HashMap<u64, DynamicBlockOverrides> = HashMap::new();
    let mut block_overrides: HashMap<(String, u64), DynamicBlockOverrides> = HashMap::new();
    for record in overrides {
        match record {
            DynamicOverrideRecord::Entity {
                handle,
                value,
                visibility,
            } => {
                let target = entity_overrides.entry(handle).or_default();
                merge_dynamic_override(target, value, visibility)?;
            }
            DynamicOverrideRecord::Block {
                block,
                entity_id,
                value,
                visibility,
            } => {
                let target = block_overrides.entry((block, entity_id)).or_default();
                merge_dynamic_override(target, value, visibility)?;
            }
        }
    }
    Ok(AggregatedDynamicOverrides {
        entity_overrides,
        block_overrides,
    })
}

fn apply_entity_dynamic_overrides(
    input: &str,
    blocks: &[BlockDefinition],
    entities: &mut [Entity],
    records: &[Option<EntityRecord>],
    overrides: HashMap<u64, DynamicBlockOverrides>,
) -> Result<(), DxfError> {
    for (handle, override_values) in overrides {
        let index = records
            .iter()
            .position(|record| {
                record
                    .as_ref()
                    .and_then(|record| record_handle_at(input, record.start))
                    .and_then(|value| u64::from_str_radix(value.trim(), 16).ok())
                    == Some(handle)
            })
            .ok_or_else(dynamic_error)?;
        let Entity::Insert(insert) = &mut entities[index] else {
            return Err(dynamic_error());
        };
        let mut dynamic = insert.dynamic_overrides().clone();
        merge_dynamic_values(&mut dynamic, &override_values);
        let block_name = insert.block_name().to_owned();
        let definition = blocks
            .iter()
            .find(|block| block.name() == block_name)
            .and_then(BlockDefinition::dynamic_definition)
            .ok_or_else(dynamic_error)?;
        definition
            .validate_overrides(&dynamic)
            .map_err(|_| dynamic_error())?;
        insert.set_dynamic_overrides(dynamic);
    }
    Ok(())
}

fn apply_block_dynamic_overrides(
    blocks: &mut [BlockDefinition],
    overrides: HashMap<(String, u64), DynamicBlockOverrides>,
) -> Result<(), DxfError> {
    for ((block, entity_id), override_values) in overrides {
        let target_index = blocks
            .iter()
            .find(|item| item.name() == block)
            .and_then(|item| item.entity_ids().iter().position(|id| *id == entity_id))
            .ok_or_else(dynamic_error)?;
        let block_index = blocks
            .iter()
            .position(|item| item.name() == block)
            .ok_or_else(dynamic_error)?;
        let mut entities = blocks[block_index].entities().to_vec();
        let Entity::Insert(insert) = &mut entities[target_index] else {
            return Err(dynamic_error());
        };
        let mut dynamic = insert.dynamic_overrides().clone();
        merge_dynamic_values(&mut dynamic, &override_values);
        let nested_definition = blocks
            .iter()
            .find(|item| item.name() == insert.block_name())
            .and_then(BlockDefinition::dynamic_definition)
            .ok_or_else(dynamic_error)?;
        nested_definition
            .validate_overrides(&dynamic)
            .map_err(|_| dynamic_error())?;
        insert.set_dynamic_overrides(dynamic);
        let ids = blocks[block_index].entity_ids().to_vec();
        blocks[block_index]
            .set_entities_with_ids(entities, ids)
            .map_err(|_| dynamic_error())?;
    }
    Ok(())
}

fn merge_dynamic_values(target: &mut DynamicBlockOverrides, source: &DynamicBlockOverrides) {
    for (name, value) in source.values() {
        target.set(name.clone(), value.clone());
    }
    if let Some(name) = source.visibility() {
        target.set_visibility(name);
    }
}

fn merge_dynamic_override(
    target: &mut DynamicBlockOverrides,
    value: Option<(String, DynamicValue)>,
    visibility: Option<String>,
) -> Result<(), DxfError> {
    if let Some((name, value)) = value {
        if target.values().contains_key(&name) {
            return Err(dynamic_error());
        }
        target.set(name, value);
    }
    if let Some(name) = visibility {
        if target.visibility().is_some() {
            return Err(dynamic_error());
        }
        target.set_visibility(name);
    }
    Ok(())
}

fn dynamic_record(handle: u64, payload: &str) -> String {
    format!("0\nXRECORD\n5\n{handle:X}\n100\nAcDbXrecord\n1000\nOpenCadData|v1|DYNAMIC|{payload}\n")
}

fn dynamic_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

fn dynamic_decode(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = char::from(pair[0]).to_digit(16)? as u8;
        let low = char::from(pair[1]).to_digit(16)? as u8;
        bytes.push((high << 4) | low);
    }
    String::from_utf8(bytes).ok()
}

fn dynamic_point(point: Point3) -> String {
    format!("{},{},{}", point.x(), point.y(), point.z())
}

fn dynamic_point_parse(value: &str) -> Option<Point3> {
    let values = value
        .split(',')
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() != 3 {
        return None;
    }
    Point3::new(values[0], values[1], values[2]).ok()
}

fn dynamic_value(value: &DynamicValue) -> String {
    match value {
        DynamicValue::Number(value) => format!("N:{value}"),
        DynamicValue::Point(point) => format!("P:{}", dynamic_point(*point)),
        DynamicValue::XY { x, y } => format!("X:{x},{y}"),
        DynamicValue::Polar { distance, angle } => format!("R:{distance},{angle}"),
        DynamicValue::Lookup(value) => format!("L:{}", dynamic_encode(value)),
        DynamicValue::Alignment { point, angle } => {
            format!("A:{},{},{},{}", point.x(), point.y(), point.z(), angle)
        }
        DynamicValue::Flip(value) => format!("B:{}", u8::from(*value)),
        DynamicValue::Visibility(name) => format!("S:{}", dynamic_encode(name)),
    }
}

fn dynamic_value_parse(value: &str) -> Option<DynamicValue> {
    let (kind, payload) = value.split_once(':')?;
    match kind {
        "N" => {
            let value: f64 = payload.parse().ok()?;
            value.is_finite().then_some(DynamicValue::Number(value))
        }
        "P" => Some(DynamicValue::Point(dynamic_point_parse(payload)?)),
        "X" => {
            let values = payload
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 2 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(DynamicValue::XY {
                x: values[0],
                y: values[1],
            })
        }
        "R" => {
            let values = payload
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 2 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(DynamicValue::Polar {
                distance: values[0],
                angle: values[1],
            })
        }
        "L" => Some(DynamicValue::Lookup(dynamic_decode(payload)?)),
        "A" => {
            let values = payload
                .split(',')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            if values.len() != 4 || values.iter().any(|value| !value.is_finite()) {
                return None;
            }
            Some(DynamicValue::Alignment {
                point: Point3::new(values[0], values[1], values[2]).ok()?,
                angle: values[3],
            })
        }
        "B" => Some(DynamicValue::Flip(match payload {
            "0" => false,
            "1" => true,
            _ => return None,
        })),
        "S" => Some(DynamicValue::Visibility(dynamic_decode(payload)?)),
        _ => None,
    }
}

fn dynamic_ids(ids: &[u64]) -> String {
    ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
}

fn dynamic_ids_parse(value: &str) -> Option<Vec<u64>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    value
        .split(',')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn dynamic_parameter_type(value: &DynamicParameterType) -> (String, String) {
    match value {
        DynamicParameterType::Distance { min, max } => ("D".into(), format!("{min},{max}")),
        DynamicParameterType::Angle { min, max } => ("A".into(), format!("{min},{max}")),
        DynamicParameterType::Point { min, max } => (
            "P".into(),
            format!(
                "{},{},{};{},{},{}",
                min[0], min[1], min[2], max[0], max[1], max[2]
            ),
        ),
        DynamicParameterType::XY { min, max } => (
            "X".into(),
            format!("{},{};{},{}", min[0], min[1], max[0], max[1]),
        ),
        DynamicParameterType::Polar {
            distance_min,
            distance_max,
            angle_min,
            angle_max,
        } => (
            "O".into(),
            format!("{distance_min},{distance_max},{angle_min},{angle_max}"),
        ),
        DynamicParameterType::Lookup {
            rows,
            reverse,
            input_parameters,
        } => (
            "L".into(),
            format!(
                "{};{};{}",
                u8::from(*reverse),
                input_parameters
                    .iter()
                    .map(|name| dynamic_encode(name))
                    .collect::<Vec<_>>()
                    .join(","),
                rows.iter()
                    .map(|row| {
                        let assignments = row
                            .assignments()
                            .iter()
                            .map(|assignment| {
                                format!(
                                    "{}={}",
                                    dynamic_encode(assignment.parameter()),
                                    dynamic_encode(&dynamic_value(assignment.value()))
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        format!(
                            "{}:{}:{}",
                            row.input(),
                            dynamic_encode(row.label()),
                            assignments
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("~")
            ),
        ),
        DynamicParameterType::Alignment {
            point_min,
            point_max,
            angle_min,
            angle_max,
        } => (
            "G".into(),
            format!(
                "{},{},{},{},{},{},{angle_min},{angle_max}",
                point_min[0], point_min[1], point_min[2], point_max[0], point_max[1], point_max[2]
            ),
        ),
        DynamicParameterType::BasePoint { min, max } => (
            "Q".into(),
            format!(
                "{},{},{},{},{},{}",
                min[0], min[1], min[2], max[0], max[1], max[2]
            ),
        ),
        DynamicParameterType::Flip => ("F".into(), String::new()),
        DynamicParameterType::Visibility { states } => (
            "V".into(),
            states
                .iter()
                .map(|state| dynamic_encode(state))
                .collect::<Vec<_>>()
                .join(","),
        ),
    }
}

fn dynamic_value_set_fields(
    component: DynamicValueComponent,
    value_set: &DynamicValueSet,
) -> (String, String) {
    let component = match component {
        DynamicValueComponent::Distance => "D",
        DynamicValueComponent::Angle => "A",
        DynamicValueComponent::X => "X",
        DynamicValueComponent::Y => "Y",
    };
    let (kind, values) = match value_set {
        DynamicValueSet::Increment { origin, step } => ("I", format!("{origin},{step}")),
        DynamicValueSet::List(values) => (
            "L",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
    };
    (component.into(), format!("{kind}|{values}"))
}

fn dynamic_action_axis(axis: DynamicActionAxis) -> &'static str {
    match axis {
        DynamicActionAxis::X => "X",
        DynamicActionAxis::Y => "Y",
        DynamicActionAxis::XY => "XY",
    }
}

fn dynamic_action_fields(action: &DynamicAction) -> (String, Vec<String>) {
    match action {
        DynamicAction::Move {
            parameter,
            origin,
            direction,
        } => (
            "MOVE".into(),
            vec![
                dynamic_encode(parameter),
                dynamic_point(*origin),
                dynamic_point(*direction),
            ],
        ),
        DynamicAction::Scale { parameter, center } => (
            "SCALE".into(),
            vec![dynamic_encode(parameter), dynamic_point(*center)],
        ),
        DynamicAction::Rotate { parameter, center } => (
            "ROTATE".into(),
            vec![dynamic_encode(parameter), dynamic_point(*center)],
        ),
        DynamicAction::Flip {
            parameter,
            first,
            second,
        } => (
            "FLIP".into(),
            vec![
                dynamic_encode(parameter),
                dynamic_point(*first),
                dynamic_point(*second),
            ],
        ),
        DynamicAction::Stretch {
            parameter,
            min,
            max,
            direction,
        } => (
            "STRETCH".into(),
            vec![
                dynamic_encode(parameter),
                dynamic_point(*min),
                dynamic_point(*max),
                dynamic_point(*direction),
            ],
        ),
        DynamicAction::PolarStretch {
            parameter,
            center,
            min,
            max,
            rotate_only_targets,
        } => (
            "POLARSTRETCH".into(),
            vec![
                dynamic_encode(parameter),
                dynamic_point(*center),
                dynamic_point(*min),
                dynamic_point(*max),
                dynamic_ids(rotate_only_targets),
            ],
        ),
        DynamicAction::Array {
            parameter,
            rows,
            columns,
            row_spacing,
            column_spacing,
        } => (
            "ARRAY".into(),
            vec![
                parameter.as_deref().map(dynamic_encode).unwrap_or_default(),
                rows.to_string(),
                columns.to_string(),
                row_spacing.to_string(),
                column_spacing.to_string(),
            ],
        ),
    }
}

fn serialize_dynamic_block(block: &BlockDefinition, output: &mut String, handle: &mut u64) {
    let has_nested_overrides = block.entities().iter().any(|entity| {
        let Entity::Insert(insert) = entity else {
            return false;
        };
        !insert.dynamic_overrides().values().is_empty()
            || insert.dynamic_overrides().visibility().is_some()
    });
    let Some(dynamic) = block.dynamic_definition() else {
        if !has_nested_overrides {
            return;
        }
        let block_name = dynamic_encode(block.name());
        let ids_payload = format!("IDS|{block_name}|{}", dynamic_ids(block.entity_ids()));
        output.push_str(&dynamic_record(*handle, &ids_payload));
        *handle = handle.saturating_add(1);
        serialize_nested_dynamic_overrides(block, &block_name, output, handle);
        return;
    };
    let block_name = dynamic_encode(block.name());
    let ids_payload = format!("IDS|{block_name}|{}", dynamic_ids(block.entity_ids()));
    output.push_str(&dynamic_record(*handle, &ids_payload));
    *handle = handle.saturating_add(1);
    for parameter in dynamic.parameters() {
        let (kind, spec) = dynamic_parameter_type(parameter.parameter_type());
        let payload = format!(
            "PARAM|{block_name}|{}|{kind}|{}|{}",
            dynamic_encode(parameter.name()),
            spec,
            dynamic_value(parameter.default())
        );
        output.push_str(&dynamic_record(*handle, &payload));
        *handle = handle.saturating_add(1);
        for (component, value_set) in parameter.value_sets() {
            let (component, values) = dynamic_value_set_fields(*component, value_set);
            let payload = format!(
                "VALUESET|{block_name}|{}|{component}|{values}",
                dynamic_encode(parameter.name())
            );
            output.push_str(&dynamic_record(*handle, &payload));
            *handle = handle.saturating_add(1);
        }
    }
    for (index, action) in dynamic.actions().iter().enumerate() {
        let (kind, fields) = dynamic_action_fields(action);
        let targets = dynamic
            .action_targets(index)
            .map(dynamic_ids)
            .unwrap_or_default();
        let payload = format!(
            "ACTION|{block_name}|{kind}|{}|{}",
            fields.join("|"),
            targets
        );
        output.push_str(&dynamic_record(*handle, &payload));
        *handle = handle.saturating_add(1);
        if let Some(options) = dynamic.action_options(index)
            && options != DynamicActionOptions::default()
        {
            let payload = format!(
                "ACTIONOPTIONS|{block_name}|{index}|{}|{}|{}",
                options.distance_multiplier,
                options.angle_offset,
                dynamic_action_axis(options.axis)
            );
            output.push_str(&dynamic_record(*handle, &payload));
            *handle = handle.saturating_add(1);
        }
    }
    for state in dynamic.visibility_states() {
        let payload = format!(
            "VIS|{block_name}|{}|{}",
            dynamic_encode(state.name()),
            dynamic_ids(state.entity_ids())
        );
        output.push_str(&dynamic_record(*handle, &payload));
        *handle = handle.saturating_add(1);
    }
    serialize_nested_dynamic_overrides(block, &block_name, output, handle);
}

fn serialize_dynamic_instances(
    entities: &[Entity],
    entity_handles: &[u64],
    output: &mut String,
    handle: &mut u64,
) {
    for (index, entity) in entities.iter().enumerate() {
        let Entity::Insert(insert) = entity else {
            continue;
        };
        let Some(handle_value) = entity_handles.get(index) else {
            continue;
        };
        let mut values = insert
            .dynamic_overrides()
            .values()
            .iter()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.0.cmp(right.0));
        for (name, value) in values {
            let payload = format!(
                "OVERRIDE|{handle_value:X}|{}|{}",
                dynamic_encode(name),
                dynamic_value(value)
            );
            output.push_str(&dynamic_record(*handle, &payload));
            *handle = handle.saturating_add(1);
        }
        if let Some(name) = insert.dynamic_overrides().visibility() {
            let payload = format!("OVIS|{handle_value:X}|{}", dynamic_encode(name));
            output.push_str(&dynamic_record(*handle, &payload));
            *handle = handle.saturating_add(1);
        }
    }
}

fn serialize_dynamic_metadata(
    blocks: &[BlockDefinition],
    entities: &[Entity],
    entity_handles: &[u64],
    handle: &mut u64,
) -> String {
    let mut output = String::new();
    for block in blocks {
        serialize_dynamic_block(block, &mut output, handle);
    }
    serialize_dynamic_instances(entities, entity_handles, &mut output, handle);
    output
}

fn serialize_nested_dynamic_overrides(
    block: &BlockDefinition,
    block_name: &str,
    output: &mut String,
    handle: &mut u64,
) {
    for (index, entity) in block.entities().iter().enumerate() {
        let Entity::Insert(insert) = entity else {
            continue;
        };
        let Some(entity_id) = block.entity_ids().get(index) else {
            continue;
        };
        let mut values = insert
            .dynamic_overrides()
            .values()
            .iter()
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.0.cmp(right.0));
        for (name, value) in values {
            let payload = format!(
                "BOVERRIDE|{block_name}|{entity_id}|{}|{}",
                dynamic_encode(name),
                dynamic_value(value)
            );
            output.push_str(&dynamic_record(*handle, &payload));
            *handle = handle.saturating_add(1);
        }
        if let Some(name) = insert.dynamic_overrides().visibility() {
            let payload = format!("BOVIS|{block_name}|{entity_id}|{}", dynamic_encode(name));
            output.push_str(&dynamic_record(*handle, &payload));
            *handle = handle.saturating_add(1);
        }
    }
}

fn dynamic_metadata_spans(input: &str) -> Vec<(usize, usize)> {
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
            .any(|field| field.code == 1000 && field.value.starts_with("OpenCadData|v1|DYNAMIC|"))
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

#[derive(Clone, Copy)]
struct PdfMetadata {
    page_size: [f64; 2],
    clip_enabled: bool,
    display_enabled: bool,
    frame_mode: u8,
    monochrome: bool,
    fade: f64,
    contrast: f64,
}

fn apply_pdf_metadata(
    underlay: &mut PdfUnderlay,
    metadata: PdfMetadata,
    line: usize,
) -> Result<(), DxfError> {
    underlay
        .set_page_size(metadata.page_size)
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::InvalidCoordinate(1),
        })?;
    underlay
        .set_frame_mode(metadata.frame_mode)
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(280),
        })?;
    underlay.set_clip_enabled(metadata.clip_enabled);
    underlay.set_display_enabled(metadata.display_enabled);
    underlay.set_monochrome(metadata.monochrome);
    underlay.set_fade(metadata.fade).map_err(|_| DxfError {
        line,
        kind: DxfErrorKind::UnsupportedProperty(282),
    })?;
    underlay
        .set_contrast(metadata.contrast)
        .map_err(|_| DxfError {
            line,
            kind: DxfErrorKind::UnsupportedProperty(281),
        })?;
    Ok(())
}

fn serialize_pdf_metadata(underlays: &[(u64, &PdfUnderlay)], handle: &mut u64) -> String {
    if underlays.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for (entity_handle, underlay) in underlays {
        let Ok(size) = underlay.resolved_page_size() else {
            continue;
        };
        output.push_str(&format!(
            "0\nXRECORD\n5\n{:X}\n100\nAcDbXrecord\n1000\nOpenCadData|v2|PDF|{:X}|{}|{}|{}|{}|{}|{}|{}|{}\n",
            *handle,
            entity_handle,
            size[0],
            size[1],
            u8::from(underlay.clip_enabled()),
            u8::from(underlay.display_enabled()),
            underlay.frame_mode(),
            u8::from(underlay.monochrome()),
            underlay.fade(),
            underlay.contrast()
        ));
        *handle = handle.saturating_add(1);
    }
    output
}

fn pdf_metadata_spans(input: &str) -> Vec<(usize, usize)> {
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
            .any(|field| field.code == 1000 && field.value.starts_with("OpenCadData|v2|PDF|"))
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

fn read_pdf_metadata(input: &str) -> HashMap<u64, PdfMetadata> {
    let mut metadata = HashMap::new();
    let mut groups = Groups::new(input);
    let mut objects = false;
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
        for field in fields.iter().filter(|field| field.code == 1000) {
            let Some(payload) = field.value.strip_prefix("OpenCadData|v2|PDF|") else {
                continue;
            };
            let values: Vec<_> = payload.split('|').collect();
            if values.len() != 9 {
                continue;
            }
            let (
                Ok(handle),
                Ok(width),
                Ok(height),
                Ok(clip),
                Ok(display),
                Ok(frame),
                Ok(mono),
                Ok(fade),
                Ok(contrast),
            ) = (
                u64::from_str_radix(values[0], 16),
                values[1].parse::<f64>(),
                values[2].parse::<f64>(),
                values[3].parse::<u8>(),
                values[4].parse::<u8>(),
                values[5].parse::<u8>(),
                values[6].parse::<u8>(),
                values[7].parse::<f64>(),
                values[8].parse::<f64>(),
            )
            else {
                continue;
            };
            if width.is_finite()
                && height.is_finite()
                && width > 0.0
                && height > 0.0
                && clip <= 1
                && display <= 1
                && frame <= 2
                && mono <= 1
                && fade.is_finite()
                && contrast.is_finite()
            {
                metadata.insert(
                    handle,
                    PdfMetadata {
                        page_size: [width, height],
                        clip_enabled: clip != 0,
                        display_enabled: display != 0,
                        frame_mode: frame,
                        monochrome: mono != 0,
                        fade,
                        contrast,
                    },
                );
            }
        }
    }
    metadata
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
