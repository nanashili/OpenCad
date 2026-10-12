//! Drawing data extraction, CSV interchange, and field expressions.
//!
//! This module intentionally stays independent of document persistence.  Entity indices are
//! used as stable handles for a document snapshot; a future persistence layer can map them to
//! native DXF handles without changing table or expression APIs.

use crate::{DxfDocument, Entity, Face3};
use std::{f64::consts::PI, fmt, str::FromStr};

pub type EntityHandle = u64;
const MAX_CSV_BYTES: usize = 16 * 1024 * 1024;
const MAX_CSV_ROWS: usize = 100_000;
const MAX_CSV_COLUMNS: usize = 256;
const MAX_CELL_BYTES: usize = 1024 * 1024;
const MAX_METADATA_BYTES: usize = 16 * 1024;
const MAX_NAME_BYTES: usize = 256;
const MAX_PATH_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CsvError {
    Empty,
    InvalidDelimiter,
    InvalidHeader,
    DuplicateHeader(String),
    TooManyRows,
    TooManyColumns,
    CellTooLarge,
    UnterminatedQuote,
    InvalidRowWidth,
}
impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CsvError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}
impl CsvTable {
    pub fn new(headers: Vec<String>, rows: Vec<Vec<String>>) -> Result<Self, CsvError> {
        validate_headers(&headers)?;
        if rows.len() > MAX_CSV_ROWS {
            return Err(CsvError::TooManyRows);
        }
        for row in &rows {
            if row.len() != headers.len() {
                return Err(CsvError::InvalidRowWidth);
            }
            for cell in row {
                validate_cell(cell)?;
            }
        }
        Ok(Self { headers, rows })
    }
    pub fn headers(&self) -> &[String] {
        &self.headers
    }
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }
    pub fn parse(input: &str) -> Result<Self, CsvError> {
        Self::parse_delimited(input, ',')
    }
    pub fn parse_delimited(input: &str, delimiter: char) -> Result<Self, CsvError> {
        if input.len() > MAX_CSV_BYTES {
            return Err(CsvError::CellTooLarge);
        }
        validate_delimiter(delimiter)?;
        let mut records: Vec<Vec<String>> = Vec::new();
        let mut row = Vec::new();
        let mut cell = String::new();
        let mut quoted = false;
        let mut in_quotes = false;
        let mut chars = input.trim_start_matches('\u{feff}').chars().peekable();
        while let Some(ch) = chars.next() {
            if in_quotes {
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        cell.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else {
                    cell.push(ch);
                }
            } else if ch == '"' && cell.is_empty() {
                in_quotes = true;
                quoted = true;
            } else if ch == delimiter {
                row.push(cell);
                cell = String::new();
            } else if ch == '\n' || ch == '\r' {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(cell);
                cell = String::new();
                if !(row.len() == 1
                    && row[0].is_empty()
                    && records.is_empty()
                    && input.trim().is_empty())
                {
                    records.push(row);
                }
                row = Vec::new();
                quoted = false;
            } else {
                cell.push(ch);
            }
            if cell.len() > MAX_CELL_BYTES {
                return Err(CsvError::CellTooLarge);
            }
        }
        if in_quotes {
            return Err(CsvError::UnterminatedQuote);
        }
        if !cell.is_empty() || !row.is_empty() || quoted {
            row.push(cell);
            records.push(row);
        }
        if records.is_empty() {
            return Err(CsvError::Empty);
        }
        let headers = records.remove(0);
        CsvTable::new(headers, records)
    }
    pub fn to_csv(&self) -> String {
        self.to_delimited(',')
    }
    pub fn to_delimited(&self, delimiter: char) -> String {
        let mut out = String::new();
        for (ri, row) in std::iter::once(&self.headers)
            .chain(self.rows.iter())
            .enumerate()
        {
            if ri != 0 {
                out.push('\r');
                out.push('\n');
            }
            for (ci, value) in row.iter().enumerate() {
                if ci != 0 {
                    out.push(delimiter);
                }
                write_csv_cell(&mut out, value, delimiter);
            }
        }
        out
    }
}
fn validate_delimiter(d: char) -> Result<(), CsvError> {
    if d == '"' || d == '\r' || d == '\n' || d == '\0' {
        Err(CsvError::InvalidDelimiter)
    } else {
        Ok(())
    }
}
fn validate_cell(s: &str) -> Result<(), CsvError> {
    if s.len() > MAX_CELL_BYTES {
        Err(CsvError::CellTooLarge)
    } else {
        Ok(())
    }
}
fn validate_headers(headers: &[String]) -> Result<(), CsvError> {
    if headers.is_empty() || headers.len() > MAX_CSV_COLUMNS {
        return Err(if headers.is_empty() {
            CsvError::InvalidHeader
        } else {
            CsvError::TooManyColumns
        });
    }
    for (i, h) in headers.iter().enumerate() {
        if h.trim().is_empty() {
            return Err(CsvError::InvalidHeader);
        }
        validate_cell(h)?;
        if headers[..i].iter().any(|x| x.eq_ignore_ascii_case(h)) {
            return Err(CsvError::DuplicateHeader(h.clone()));
        }
    }
    Ok(())
}
fn write_csv_cell(out: &mut String, value: &str, delimiter: char) {
    if value.contains(delimiter)
        || value.contains('"')
        || value.contains('\r')
        || value.contains('\n')
    {
        out.push('"');
        out.push_str(&value.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(value);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataLinkError {
    InvalidName,
    InvalidPath,
    InvalidDelimiter,
    InvalidHandle,
}
impl fmt::Display for DataLinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DataLinkError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataLink {
    name: String,
    path: String,
    target_entity_handle: Option<EntityHandle>,
    has_header: bool,
    delimiter: char,
}
impl DataLink {
    pub fn new(
        name: impl Into<String>,
        path: impl Into<String>,
        target: Option<EntityHandle>,
        has_header: bool,
        delimiter: char,
    ) -> Result<Self, DataLinkError> {
        let name = name.into();
        let path = path.into();
        if name.trim().is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.contains('\0')
            || name.contains('\t')
            || name.contains('\r')
            || name.contains('\n')
        {
            return Err(DataLinkError::InvalidName);
        }
        if path.trim().is_empty()
            || path.len() > MAX_PATH_BYTES
            || path.contains('\0')
            || path.contains('\t')
            || path.contains('\r')
            || path.contains('\n')
        {
            return Err(DataLinkError::InvalidPath);
        }
        if delimiter == '"' || delimiter == '\r' || delimiter == '\n' || delimiter == '\0' {
            return Err(DataLinkError::InvalidDelimiter);
        }
        Ok(Self {
            name,
            path,
            target_entity_handle: target,
            has_header,
            delimiter,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn target_entity_handle(&self) -> Option<EntityHandle> {
        self.target_entity_handle
    }
    pub fn has_header(&self) -> bool {
        self.has_header
    }
    pub fn delimiter(&self) -> char {
        self.delimiter
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldExpression {
    EntityHandle,
    Type,
    Layer,
    Color,
    Linetype,
    Length,
    Area,
    Volume,
    BlockAttribute(String),
    DrawingEntityCount,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldExpressionError {
    Empty,
    Unknown(String),
    InvalidAttribute,
}
impl fmt::Display for FieldExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FieldExpressionError {}
impl FromStr for FieldExpression {
    type Err = FieldExpressionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}
impl FieldExpression {
    pub fn parse(input: &str) -> Result<Self, FieldExpressionError> {
        let s = input.trim();
        if s.is_empty() {
            return Err(FieldExpressionError::Empty);
        }
        let lower = s.to_ascii_lowercase();
        Ok(match lower.as_str() {
            "handle" | "entityhandle" => Self::EntityHandle,
            "type" | "entitytype" => Self::Type,
            "layer" => Self::Layer,
            "color" | "colorindex" => Self::Color,
            "linetype" => Self::Linetype,
            "length" => Self::Length,
            "area" => Self::Area,
            "volume" => Self::Volume,
            "entitycount" | "drawingentitycount" => Self::DrawingEntityCount,
            _ if lower.starts_with("blockattr/") => {
                let tag = s[10..].trim();
                if tag.is_empty()
                    || tag.len() > MAX_NAME_BYTES
                    || tag.contains('\0')
                    || tag.contains('\t')
                    || tag.contains('\r')
                    || tag.contains('\n')
                {
                    return Err(FieldExpressionError::InvalidAttribute);
                }
                Self::BlockAttribute(tag.to_owned())
            }
            _ if lower.starts_with("blockattr(") && s.ends_with(')') => {
                let tag = s[10..s.len() - 1].trim();
                if tag.is_empty()
                    || tag.len() > MAX_NAME_BYTES
                    || tag.contains('\0')
                    || tag.contains('\t')
                    || tag.contains('\r')
                    || tag.contains('\n')
                {
                    return Err(FieldExpressionError::InvalidAttribute);
                }
                Self::BlockAttribute(tag.to_owned())
            }
            _ => return Err(FieldExpressionError::Unknown(s.to_owned())),
        })
    }
    /// Stable application metadata spelling for persistence in XRECORD-like data.
    pub fn to_metadata(&self) -> String {
        match self {
            Self::EntityHandle => "handle".into(),
            Self::Type => "type".into(),
            Self::Layer => "layer".into(),
            Self::Color => "color".into(),
            Self::Linetype => "linetype".into(),
            Self::Length => "length".into(),
            Self::Area => "area".into(),
            Self::Volume => "volume".into(),
            Self::DrawingEntityCount => "entitycount".into(),
            Self::BlockAttribute(tag) => format!("blockattr/{}", encode_meta(tag)),
        }
    }
    pub fn from_metadata(value: &str) -> Result<Self, FieldExpressionError> {
        let value = if let Some(tag) = value.strip_prefix("blockattr/") {
            format!(
                "blockattr/{}",
                decode_meta(tag).map_err(|_| FieldExpressionError::InvalidAttribute)?
            )
        } else {
            value.to_owned()
        };
        Self::parse(&value)
    }

    pub fn evaluate(
        &self,
        doc: &DxfDocument,
        handle: Option<EntityHandle>,
    ) -> Result<Option<FieldValue>, FieldEvaluationError> {
        if matches!(self, Self::DrawingEntityCount) {
            return Ok(Some(FieldValue::Number(doc.entities().len() as f64)));
        }
        let stable_handle = handle.ok_or(FieldEvaluationError::MissingEntity)?;
        let index = doc
            .entity_index_by_handle(stable_handle)
            .ok_or(FieldEvaluationError::InvalidEntity(stable_handle))?;
        let entity = doc
            .entities()
            .get(index)
            .ok_or(FieldEvaluationError::InvalidEntity(stable_handle))?;
        Ok(match self {
            Self::EntityHandle => Some(FieldValue::Text(stable_handle.to_string())),
            Self::Type => Some(FieldValue::Text(entity.name().to_owned())),
            Self::Layer => doc
                .entity_layer(index)
                .map(|v| FieldValue::Text(v.to_owned())),
            Self::Color => doc
                .entity_style(index)
                .map(|v| FieldValue::Number(v.color_index() as f64)),
            Self::Linetype => doc
                .entity_style(index)
                .map(|v| FieldValue::Text(v.linetype().to_owned())),
            Self::Length => measure_length(entity).map(FieldValue::Number),
            Self::Area => measure_area(entity).map(FieldValue::Number),
            Self::Volume => measure_volume(entity).map(FieldValue::Number),
            Self::BlockAttribute(tag) => match entity {
                Entity::Insert(insert) => insert
                    .attributes()
                    .iter()
                    .find(|a| a.tag().eq_ignore_ascii_case(tag))
                    .map(|a| FieldValue::Text(a.value().to_owned())),
                _ => None,
            },
            Self::DrawingEntityCount => unreachable!(),
        })
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Text(String),
    Number(f64),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldEvaluationError {
    MissingEntity,
    InvalidEntity(EntityHandle),
    Geometry,
}
impl fmt::Display for FieldEvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FieldEvaluationError {}

fn measure_length(entity: &Entity) -> Option<f64> {
    match entity {
        Entity::Line(v) => v.length().ok(),
        Entity::Arc(v) => v.length().ok(),
        Entity::Circle(v) => Some(2.0 * PI * v.radius()),
        Entity::Ellipse(v) => v.length().ok(),
        Entity::Polyline(v) => v.length().ok(),
        Entity::Polyline3(v) => v.length().ok(),
        _ => None,
    }
}
fn triangle_area(a: crate::Point3, b: crate::Point3, c: crate::Point3) -> f64 {
    let ux = b.x() - a.x();
    let uy = b.y() - a.y();
    let uz = b.z() - a.z();
    let vx = c.x() - a.x();
    let vy = c.y() - a.y();
    let vz = c.z() - a.z();
    let cx = uy * vz - uz * vy;
    let cy = uz * vx - ux * vz;
    let cz = ux * vy - uy * vx;
    0.5 * (cx * cx + cy * cy + cz * cz).sqrt()
}
fn face_area(face: Face3) -> f64 {
    let (v, n) = face.vertices();
    triangle_area(v[0], v[1], v[2])
        + if n == 4 {
            triangle_area(v[0], v[2], v[3])
        } else {
            0.0
        }
}
fn measure_area(entity: &Entity) -> Option<f64> {
    match entity {
        Entity::Circle(v) => Some(PI * v.radius() * v.radius()),
        Entity::Face3(v) => Some(face_area(*v)),
        Entity::Mesh(v) => crate::mesh_area(v).ok(),
        _ => None,
    }
}
fn measure_volume(entity: &Entity) -> Option<f64> {
    match entity {
        Entity::Mesh(v) => crate::mesh_volume(v).ok(),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldMetadataError {
    TooLarge,
    InvalidFormat,
    InvalidHandle,
    InvalidExpression,
}
impl fmt::Display for FieldMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FieldMetadataError {}
fn parse_optional_handle(value: &str) -> Result<Option<EntityHandle>, FieldMetadataError> {
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse()
            .map(Some)
            .map_err(|_| FieldMetadataError::InvalidHandle)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedField {
    name: String,
    source_handle: Option<EntityHandle>,
    target_handle: Option<EntityHandle>,
    table_cell: Option<(usize, usize)>,
    expression: FieldExpression,
    precision: u8,
    scale: f64,
}
impl NamedField {
    pub fn new(
        name: impl Into<String>,
        source_handle: Option<EntityHandle>,
        target_handle: Option<EntityHandle>,
        table_cell: Option<(usize, usize)>,
        expression: FieldExpression,
        precision: u8,
        scale: f64,
    ) -> Result<Self, FieldEvaluationError> {
        let name = name.into();
        if name.trim().is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.contains('\0')
            || name.contains('\t')
            || name.contains('\r')
            || name.contains('\n')
            || precision > 15
            || !scale.is_finite()
        {
            return Err(FieldEvaluationError::Geometry);
        }
        Ok(Self {
            name,
            source_handle,
            target_handle,
            table_cell,
            expression,
            precision,
            scale,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn expression(&self) -> &FieldExpression {
        &self.expression
    }
    pub fn source_handle(&self) -> Option<EntityHandle> {
        self.source_handle
    }
    pub fn target_handle(&self) -> Option<EntityHandle> {
        self.target_handle
    }
    pub fn precision(&self) -> u8 {
        self.precision
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn table_cell(&self) -> Option<(usize, usize)> {
        self.table_cell
    }
    /// Stable tab-separated metadata for application-owned field persistence.
    pub fn to_metadata(&self) -> String {
        let source = self
            .source_handle
            .map_or_else(String::new, |v| v.to_string());
        let target = self
            .target_handle
            .map_or_else(String::new, |v| v.to_string());
        let cell = self
            .table_cell
            .map_or_else(String::new, |(r, c)| format!("{r},{c}"));
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            encode_meta(&self.name),
            source,
            target,
            cell,
            encode_meta(&self.expression.to_metadata()),
            self.precision,
            self.scale
        )
    }
    pub fn from_metadata(value: &str) -> Result<Self, FieldMetadataError> {
        if value.len() > MAX_METADATA_BYTES {
            return Err(FieldMetadataError::TooLarge);
        }
        let fields: Vec<_> = value.split('\t').collect();
        if fields.len() != 7 {
            return Err(FieldMetadataError::InvalidFormat);
        }
        let name = decode_meta(fields[0]).map_err(|_| FieldMetadataError::InvalidFormat)?;
        let source = parse_optional_handle(fields[1])?;
        let target = parse_optional_handle(fields[2])?;
        let cell = if fields[3].is_empty() {
            None
        } else {
            let mut it = fields[3].split(',');
            let r = it
                .next()
                .ok_or(FieldMetadataError::InvalidFormat)?
                .parse()
                .map_err(|_| FieldMetadataError::InvalidFormat)?;
            let c = it
                .next()
                .ok_or(FieldMetadataError::InvalidFormat)?
                .parse()
                .map_err(|_| FieldMetadataError::InvalidFormat)?;
            if it.next().is_some() {
                return Err(FieldMetadataError::InvalidFormat);
            }
            Some((r, c))
        };
        let expression = FieldExpression::from_metadata(
            &decode_meta(fields[4]).map_err(|_| FieldMetadataError::InvalidFormat)?,
        )
        .map_err(|_| FieldMetadataError::InvalidExpression)?;
        let precision = fields[5]
            .parse()
            .map_err(|_| FieldMetadataError::InvalidFormat)?;
        let scale = fields[6]
            .parse()
            .map_err(|_| FieldMetadataError::InvalidFormat)?;
        Self::new(name, source, target, cell, expression, precision, scale)
            .map_err(|_| FieldMetadataError::InvalidFormat)
    }

    pub fn evaluate(&self, doc: &DxfDocument) -> Result<Option<String>, FieldEvaluationError> {
        Ok(self
            .expression
            .evaluate(doc, self.source_handle)?
            .map(|v| match v {
                FieldValue::Text(s) => s,
                FieldValue::Number(n) => format!("{:.*}", self.precision as usize, n * self.scale),
            }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataColumn {
    name: String,
    expression: FieldExpression,
}
impl DataColumn {
    pub fn new(
        name: impl Into<String>,
        expression: FieldExpression,
    ) -> Result<Self, FieldExpressionError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(FieldExpressionError::Empty);
        }
        Ok(Self { name, expression })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn expression(&self) -> &FieldExpression {
        &self.expression
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExtractionError {
    NoColumns,
    Field(FieldEvaluationError),
    Csv(CsvError),
}
impl fmt::Display for ExtractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ExtractionError {}
pub fn extract_csv(
    doc: &DxfDocument,
    columns: &[DataColumn],
    type_filter: Option<&str>,
    layer_filter: Option<&str>,
) -> Result<CsvTable, ExtractionError> {
    if columns.is_empty() || columns.len() > MAX_CSV_COLUMNS {
        return Err(ExtractionError::NoColumns);
    }
    let headers = columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    let mut rows = Vec::new();
    for (index, entity) in doc.entities().iter().enumerate() {
        if type_filter.is_some_and(|v| !entity.name().eq_ignore_ascii_case(v))
            || layer_filter.is_some_and(|v| {
                !doc.entity_layer(index)
                    .is_some_and(|l| l.eq_ignore_ascii_case(v))
            })
        {
            continue;
        }
        let mut row = Vec::with_capacity(columns.len());
        for column in columns {
            let handle = doc.entity_handle(index).ok_or(ExtractionError::Field(
                FieldEvaluationError::InvalidEntity(index as u64),
            ))?;
            let value = column
                .expression
                .evaluate(doc, Some(handle))
                .map_err(ExtractionError::Field)?;
            row.push(
                value
                    .map(|v| match v {
                        FieldValue::Text(s) => s,
                        FieldValue::Number(n) => n.to_string(),
                    })
                    .unwrap_or_default(),
            );
        }
        rows.push(row);
    }
    CsvTable::new(headers, rows).map_err(ExtractionError::Csv)
}

/// Errors raised while applying a field or linked table update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldUpdateError {
    MissingTarget,
    InvalidTarget(EntityHandle),
    InvalidCell,
    UnsupportedTarget(&'static str),
    Evaluation(FieldEvaluationError),
    Geometry,
}
impl fmt::Display for FieldUpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FieldUpdateError {}

/// Applies one field atomically: evaluation and target validation complete before mutation.
pub fn update_named_field(
    doc: &mut DxfDocument,
    field: &NamedField,
) -> Result<(), FieldUpdateError> {
    let value = field
        .evaluate(doc)
        .map_err(FieldUpdateError::Evaluation)?
        .ok_or(FieldUpdateError::Evaluation(
            FieldEvaluationError::MissingEntity,
        ))?;
    let target = field.target_handle.ok_or(FieldUpdateError::MissingTarget)?;
    let index = doc
        .entity_index_by_handle(target)
        .ok_or(FieldUpdateError::InvalidTarget(target))?;
    let mut entity = doc
        .entities()
        .get(index)
        .cloned()
        .ok_or(FieldUpdateError::InvalidTarget(target))?;
    match (&mut entity, field.table_cell) {
        (Entity::Text(text), None) => text.set_content(value),
        (Entity::MText(text), None) => text.set_content(value),
        (Entity::Table(table), Some((row, column))) => table
            .set_cell(row, column, value)
            .map_err(|_| FieldUpdateError::InvalidCell)?,
        (Entity::Table(_), None) => return Err(FieldUpdateError::InvalidCell),
        (other, _) => return Err(FieldUpdateError::UnsupportedTarget(other.name())),
    }
    doc.replace_entity(index, entity)
        .ok_or(FieldUpdateError::InvalidTarget(target))?;
    Ok(())
}

/// Applies a batch only after every source, target, and table cell has validated.
pub fn update_named_fields(
    doc: &mut DxfDocument,
    fields: &[NamedField],
) -> Result<(), FieldUpdateError> {
    let mut changes = Vec::with_capacity(fields.len());
    for field in fields {
        let value = field
            .evaluate(doc)
            .map_err(FieldUpdateError::Evaluation)?
            .ok_or(FieldUpdateError::Evaluation(
                FieldEvaluationError::MissingEntity,
            ))?;
        let target = field.target_handle.ok_or(FieldUpdateError::MissingTarget)?;
        let index = doc
            .entity_index_by_handle(target)
            .ok_or(FieldUpdateError::InvalidTarget(target))?;
        let mut entity = doc
            .entities()
            .get(index)
            .cloned()
            .ok_or(FieldUpdateError::InvalidTarget(target))?;
        match (&mut entity, field.table_cell) {
            (Entity::Text(text), None) => text.set_content(value),
            (Entity::MText(text), None) => text.set_content(value),
            (Entity::Table(table), Some((row, column))) => table
                .set_cell(row, column, value)
                .map_err(|_| FieldUpdateError::InvalidCell)?,
            (Entity::Table(_), None) => return Err(FieldUpdateError::InvalidCell),
            (other, _) => return Err(FieldUpdateError::UnsupportedTarget(other.name())),
        }
        changes.push((index, entity));
    }
    for (index, entity) in changes {
        doc.replace_entity(index, entity)
            .ok_or(FieldUpdateError::InvalidTarget(index as u64))?;
    }
    Ok(())
}

/// Builds a table from CSV data. When `has_header` is true, headers become the first row.
pub fn table_from_csv(
    table: &CsvTable,
    anchor: crate::Point3,
    has_header: bool,
    row_height: f64,
    column_width: f64,
) -> Result<crate::Table, FieldUpdateError> {
    if !row_height.is_finite()
        || row_height <= 0.0
        || !column_width.is_finite()
        || column_width <= 0.0
    {
        return Err(FieldUpdateError::Geometry);
    }
    let mut cells = Vec::with_capacity(table.rows.len() + usize::from(has_header));
    if has_header {
        cells.push(table.headers.clone());
    }
    cells.extend(table.rows.clone());
    if cells.is_empty() {
        cells.push(vec![String::new(); table.headers.len()]);
    }
    let widths = vec![column_width; table.headers.len()];
    let heights = vec![row_height; cells.len()];
    crate::Table::new(anchor, heights, widths, cells).map_err(|_| FieldUpdateError::Geometry)
}

/// Replaces a linked table while preserving its anchor and existing row/column dimensions.
pub fn update_linked_table(
    doc: &mut DxfDocument,
    link: &DataLink,
    table: &CsvTable,
) -> Result<(), FieldUpdateError> {
    let target = link
        .target_entity_handle
        .ok_or(FieldUpdateError::MissingTarget)?;
    let index = doc
        .entity_index_by_handle(target)
        .ok_or(FieldUpdateError::InvalidTarget(target))?;
    let old = match doc.entities().get(index) {
        Some(Entity::Table(t)) => t,
        Some(entity) => return Err(FieldUpdateError::UnsupportedTarget(entity.name())),
        None => return Err(FieldUpdateError::InvalidTarget(target)),
    };
    let mut cells = Vec::with_capacity(table.rows.len() + usize::from(link.has_header));
    if link.has_header {
        cells.push(table.headers.clone());
    }
    cells.extend(table.rows.clone());
    if cells.is_empty() {
        cells.push(vec![String::new(); table.headers.len()]);
    }
    let columns = table.headers.len();
    let widths = (0..columns)
        .map(|i| old.column_widths().get(i).copied().unwrap_or(1.0))
        .collect();
    let fallback_height = old.row_heights().last().copied().unwrap_or(1.0);
    let heights = (0..cells.len())
        .map(|i| old.row_heights().get(i).copied().unwrap_or(fallback_height))
        .collect();
    let replacement = crate::Table::new(old.anchor(), heights, widths, cells)
        .map_err(|_| FieldUpdateError::Geometry)?;
    doc.replace_entity(index, Entity::Table(replacement))
        .ok_or(FieldUpdateError::InvalidTarget(target))?;
    Ok(())
}

impl DataLink {
    /// Stable compact metadata for an application-owned XRECORD namespace.
    pub fn to_metadata(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            encode_meta(&self.name),
            encode_meta(&self.path),
            self.target_entity_handle
                .map_or_else(String::new, |v| v.to_string()),
            if self.has_header { "1" } else { "0" },
            self.delimiter as u32
        )
    }
    pub fn from_metadata(value: &str) -> Result<Self, DataLinkError> {
        if value.len() > MAX_METADATA_BYTES {
            return Err(DataLinkError::InvalidPath);
        }
        let fields: Vec<_> = value.split('\t').collect();
        if fields.len() != 5 {
            return Err(DataLinkError::InvalidPath);
        }
        let target = if fields[2].is_empty() {
            None
        } else {
            Some(
                fields[2]
                    .parse()
                    .map_err(|_| DataLinkError::InvalidHandle)?,
            )
        };
        let delimiter = char::from_u32(
            fields[4]
                .parse()
                .map_err(|_| DataLinkError::InvalidDelimiter)?,
        )
        .ok_or(DataLinkError::InvalidDelimiter)?;
        Self::new(
            decode_meta(fields[0])?,
            decode_meta(fields[1])?,
            target,
            fields[3] == "1",
            delimiter,
        )
    }
}
fn encode_meta(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\t', "%09")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}
fn decode_meta(value: &str) -> Result<String, DataLinkError> {
    let mut out = String::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(DataLinkError::InvalidPath);
            }
            let code = u8::from_str_radix(&value[i + 1..i + 3], 16)
                .map_err(|_| DataLinkError::InvalidPath)?;
            out.push(code as char);
            i += 3;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}
