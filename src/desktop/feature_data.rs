use super::*;
use crate::drawing_data::{
    CsvTable, DataColumn, DataLink, FieldExpression, NamedField, extract_csv, table_from_csv,
    update_linked_table, update_named_fields,
};
use crate::table_formula::evaluate_table;
use std::fs;
use std::path::Path;

impl OpenCadApp {
    pub(super) fn feature_data_command(
        &mut self,
        command: &str,
        rest: &str,
    ) -> Option<Result<String, String>> {
        let tokens = match super::commands::command_tokens(rest) {
            Ok(tokens) => tokens,
            Err(error) => return Some(Err(error)),
        };
        let result = match command.to_ascii_uppercase().as_str() {
            "DATAEXTRACTION" => self.data_extraction(&tokens),
            "DATALINK" => self.data_link(&tokens),
            "DATALINKUPDATE" => self.data_link_update(&tokens),
            "FIELD" => self.field_command(&tokens),
            "UPDATEFIELD" => self.update_field(&tokens),
            _ => return None,
        };
        Some(result)
    }

    fn data_extraction(&mut self, tokens: &[String]) -> Result<String, String> {
        let mut columns = Vec::new();
        let mut type_filter = None;
        let mut layer_filter = None;
        let mut insert_at = None;
        let mut output_path = None;
        for token in tokens {
            if let Some(value) = token.strip_prefix("type=") {
                type_filter = Some(value);
            } else if let Some(value) = token.strip_prefix("layer=") {
                layer_filter = Some(value);
            } else if let Some(value) = token.strip_prefix("output=") {
                output_path = Some(value.to_owned());
            } else if let Some(value) = token.strip_prefix("at=") {
                insert_at = Some(super::commands::parse_point(
                    value,
                    self.drawing_elevation(),
                )?);
            } else {
                let (name, expression) = token
                    .split_once('=')
                    .ok_or("DATAEXTRACTION columns use Name=Property.")?;
                columns.push(
                    DataColumn::new(
                        name,
                        FieldExpression::parse(expression).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?,
                );
            }
        }
        if columns.is_empty() {
            columns.push(
                DataColumn::new("Handle", FieldExpression::EntityHandle)
                    .map_err(|e| e.to_string())?,
            );
            columns
                .push(DataColumn::new("Type", FieldExpression::Type).map_err(|e| e.to_string())?);
            columns
                .push(DataColumn::new("Layer", FieldExpression::Layer).map_err(|e| e.to_string())?);
        }
        let table = extract_csv(self.editor.document(), &columns, type_filter, layer_filter)
            .map_err(|e| e.to_string())?;
        if let Some(anchor) = insert_at {
            let table_entity =
                table_from_csv(&table, anchor, true, 2.5, 12.0).map_err(|e| e.to_string())?;
            self.editor
                .add(vec![Entity::Table(table_entity)])
                .map_err(|e| e.to_string())?;
        }
        let csv = table.to_csv();
        if let Some(path) = output_path {
            super::atomic_write(Path::new(&path), csv.as_bytes())
                .map_err(|e| format!("Cannot write extraction {path}: {e}"))?;
            Ok(format!("Data extracted to {path}."))
        } else {
            Ok(csv)
        }
    }

    fn data_link(&mut self, tokens: &[String]) -> Result<String, String> {
        let action = tokens
            .first()
            .map(String::as_str)
            .unwrap_or("LIST")
            .to_ascii_uppercase();
        match action.as_str() {
            "LIST" => Ok(self
                .editor
                .document()
                .data_links()
                .iter()
                .map(|link| link.name().to_owned())
                .collect::<Vec<_>>()
                .join(", ")),
            "CREATE" => {
                if tokens.len() < 3 {
                    return Err("DATALINK CREATE name path [entity].".into());
                }
                let target = tokens
                    .get(3)
                    .map(|value| self.parse_handle(value))
                    .transpose()?;
                if let Some(handle) = target {
                    let index = self
                        .editor
                        .document()
                        .entity_index_by_handle(handle)
                        .ok_or("DATALINK target entity not found.")?;
                    if !matches!(
                        self.editor.document().entities().get(index),
                        Some(Entity::Table(_))
                    ) {
                        return Err("DATALINK target must be a TABLE entity.".into());
                    }
                }
                let link = DataLink::new(tokens[1].clone(), tokens[2].clone(), target, true, ',')
                    .map_err(|e| e.to_string())?;
                let mut candidate = self.editor.document().clone();
                let mut links = candidate.data_links().to_vec();
                if links
                    .iter()
                    .any(|item| item.name().eq_ignore_ascii_case(link.name()))
                {
                    return Err("Data link name already exists.".into());
                }
                links.push(link);
                candidate.set_data_links(links);
                self.editor
                    .commit_document(candidate)
                    .map_err(|e| e.to_string())?;
                Ok("Data link created.".into())
            }
            "DELETE" => {
                let name = tokens.get(1).ok_or("DATALINK DELETE name.")?;
                let mut candidate = self.editor.document().clone();
                let mut links = candidate.data_links().to_vec();
                let before = links.len();
                links.retain(|link| !link.name().eq_ignore_ascii_case(name));
                if links.len() == before {
                    return Err("Data link not found.".into());
                }
                candidate.set_data_links(links);
                self.editor
                    .commit_document(candidate)
                    .map_err(|e| e.to_string())?;
                Ok("Data link deleted.".into())
            }
            _ => Err("DATALINK syntax: LIST, CREATE name path [entity], or DELETE name.".into()),
        }
    }

    fn data_link_update(&mut self, tokens: &[String]) -> Result<String, String> {
        if tokens.len() < 2 {
            return Err("DATALINKUPDATE PULL|PUSH name.".into());
        }
        let direction = tokens[0].to_ascii_uppercase();
        let link = self
            .editor
            .document()
            .data_links()
            .iter()
            .find(|link| link.name().eq_ignore_ascii_case(&tokens[1]))
            .cloned()
            .ok_or("Data link not found.")?;
        let path = self.resolve_data_path(link.path());
        match direction.as_str() {
            "PULL" | "DOWNLOAD" => {
                if tokens.len() != 2 {
                    return Err("DATALINKUPDATE PULL name.".into());
                }
                let csv = CsvTable::parse_delimited(
                    &fs::read_to_string(path).map_err(|e| e.to_string())?,
                    link.delimiter(),
                )
                .map_err(|e| e.to_string())?;
                let mut candidate = self.editor.document().clone();
                update_linked_table(&mut candidate, &link, &csv).map_err(|e| e.to_string())?;
                self.editor
                    .commit_document(candidate)
                    .map_err(|e| e.to_string())?;
                Ok("Data link updated from CSV.".into())
            }
            "PUSH" | "UPLOAD" => {
                if tokens.len() > 3 {
                    return Err("DATALINKUPDATE PUSH name [RAW|EVALUATED].".into());
                }
                let evaluated = match tokens.get(2).map(String::as_str) {
                    None => false,
                    Some(value) if value.eq_ignore_ascii_case("RAW") => false,
                    Some(value) if value.eq_ignore_ascii_case("EVALUATED") => true,
                    Some(_) => return Err("DATALINKUPDATE PUSH name [RAW|EVALUATED].".into()),
                };
                let handle = link
                    .target_entity_handle()
                    .ok_or("Data link has no table target.")?;
                let index = self
                    .editor
                    .document()
                    .entity_index_by_handle(handle)
                    .ok_or("Data link target not found.")?;
                let Entity::Table(table) = &self.editor.document().entities()[index] else {
                    return Err("Data link target is not a table.".into());
                };
                let temp = path.with_extension("opencad.tmp");
                fs::write(
                    &temp,
                    self.table_csv(table, link.has_header(), link.delimiter(), evaluated)?,
                )
                .map_err(|e| e.to_string())?;
                fs::rename(&temp, path).map_err(|e| e.to_string())?;
                Ok("Data link exported to CSV.".into())
            }
            _ => Err("DATALINKUPDATE syntax: PULL|PUSH name [RAW|EVALUATED].".into()),
        }
    }

    fn field_command(&mut self, tokens: &[String]) -> Result<String, String> {
        match tokens
            .first()
            .map(String::as_str)
            .unwrap_or("LIST")
            .to_ascii_uppercase()
            .as_str()
        {
            "LIST" => Ok(self
                .editor
                .document()
                .named_fields()
                .iter()
                .map(|field| field.name().to_owned())
                .collect::<Vec<_>>()
                .join(", ")),
            "CREATE" => {
                if tokens.len() < 3 {
                    return Err("FIELD CREATE name expression [source] [target].".into());
                }
                let source = tokens.get(3).map(|v| self.parse_handle(v)).transpose()?;
                let target = tokens.get(4).map(|v| self.parse_handle(v)).transpose()?;
                let table_cell = match (tokens.get(5), tokens.get(6)) {
                    (Some(row), Some(column)) => Some((
                        row.parse().map_err(|_| "Invalid field row.")?,
                        column.parse().map_err(|_| "Invalid field column.")?,
                    )),
                    (None, None) => None,
                    _ => {
                        return Err("FIELD CREATE row and column must be supplied together.".into());
                    }
                };
                let precision = tokens
                    .get(7)
                    .map_or(Ok(3), |v| v.parse().map_err(|_| "Invalid field precision."))?;
                let scale = tokens
                    .get(8)
                    .map_or(Ok(1.0), |v| v.parse().map_err(|_| "Invalid field scale."))?;
                let expression = FieldExpression::parse(&tokens[2]).map_err(|e| e.to_string())?;
                if let Some(handle) = source {
                    self.editor
                        .document()
                        .entity_index_by_handle(handle)
                        .ok_or("FIELD source entity not found.")?;
                }
                if let Some(handle) = target {
                    let index = self
                        .editor
                        .document()
                        .entity_index_by_handle(handle)
                        .ok_or("FIELD target entity not found.")?;
                    if !matches!(
                        self.editor.document().entities().get(index),
                        Some(Entity::Text(_) | Entity::MText(_) | Entity::Table(_))
                    ) {
                        return Err("FIELD target must be TEXT, MTEXT, or TABLE.".into());
                    }
                }
                let field = NamedField::new(
                    tokens[1].clone(),
                    source,
                    target,
                    table_cell,
                    expression,
                    precision,
                    scale,
                )
                .map_err(|e| e.to_string())?;
                if let Some(handle) = target
                    && let Some((row, column)) = field.table_cell()
                {
                    let index = self
                        .editor
                        .document()
                        .entity_index_by_handle(handle)
                        .ok_or("FIELD target entity not found.")?;
                    let Entity::Table(table) = &self.editor.document().entities()[index] else {
                        return Err("FIELD row/column requires a TABLE target.".into());
                    };
                    if row >= table.cells().len()
                        || column >= table.cells().first().map_or(0, Vec::len)
                    {
                        return Err("FIELD table cell is out of range.".into());
                    }
                }
                let mut candidate = self.editor.document().clone();
                if source.is_some() {
                    field.evaluate(&candidate).map_err(|e| e.to_string())?;
                }
                let mut fields = candidate.named_fields().to_vec();
                if fields
                    .iter()
                    .any(|item| item.name().eq_ignore_ascii_case(field.name()))
                {
                    return Err("Field name already exists.".into());
                }
                fields.push(field);
                candidate.set_named_fields(fields);
                self.editor
                    .commit_document(candidate)
                    .map_err(|e| e.to_string())?;
                Ok("Field created.".into())
            }
            "DELETE" => {
                let name = tokens.get(1).ok_or("FIELD DELETE name.")?;
                let mut candidate = self.editor.document().clone();
                let mut fields = candidate.named_fields().to_vec();
                let before = fields.len();
                fields.retain(|field| !field.name().eq_ignore_ascii_case(name));
                if fields.len() == before {
                    return Err("Field not found.".into());
                }
                candidate.set_named_fields(fields);
                self.editor
                    .commit_document(candidate)
                    .map_err(|e| e.to_string())?;
                Ok("Field deleted.".into())
            }
            _ => Err(
                "FIELD syntax: LIST, CREATE name expression [source] [target], or DELETE name."
                    .into(),
            ),
        }
    }

    fn parse_handle(&self, value: &str) -> Result<u64, String> {
        let value = value.trim();
        if let Some(hex) = value.strip_prefix('@') {
            return u64::from_str_radix(hex.trim_start_matches("0x"), 16)
                .map_err(|_| "Invalid @HEXHANDLE reference.".to_owned());
        }
        let index = value
            .parse::<usize>()
            .map_err(|_| "Entity reference must be a one-based index or @HEXHANDLE.")?;
        let index = index.checked_sub(1).ok_or("Entity indices start at 1.")?;
        self.editor
            .document()
            .entity_handle(index)
            .ok_or_else(|| "Entity reference is out of range.".into())
    }

    fn resolve_data_path(&self, path: &str) -> std::path::PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_owned()
        } else {
            self.path
                .as_ref()
                .and_then(|drawing| drawing.parent())
                .unwrap_or_else(|| Path::new("."))
                .join(path)
        }
    }

    fn table_csv(
        &self,
        table: &crate::Table,
        has_header: bool,
        delimiter: char,
        evaluated: bool,
    ) -> Result<String, String> {
        let evaluated_cells;
        let cells = if evaluated {
            evaluated_cells = evaluate_table(table).display_cells();
            evaluated_cells.as_slice()
        } else {
            table.cells()
        };
        if cells.is_empty() || cells[0].is_empty() {
            return Err("TABLE has no cells to export.".into());
        }
        let (headers, rows) = if has_header {
            (cells[0].clone(), cells[1..].to_vec())
        } else {
            (
                (0..cells[0].len())
                    .map(|index| format!("Column{}", index + 1))
                    .collect(),
                cells.to_vec(),
            )
        };
        CsvTable::new(headers, rows)
            .map(|csv| csv.to_delimited(delimiter))
            .map_err(|error| error.to_string())
    }

    fn update_field(&mut self, tokens: &[String]) -> Result<String, String> {
        let mut candidate = self.editor.document().clone();
        let fields = candidate.named_fields().to_vec();
        let selected = tokens
            .first()
            .filter(|value| !value.eq_ignore_ascii_case("ALL"));
        let selected_fields = fields
            .into_iter()
            .filter(|field| selected.is_none_or(|name| field.name().eq_ignore_ascii_case(name)))
            .collect::<Vec<_>>();
        if selected_fields.is_empty() {
            return Err("No matching fields.".into());
        }
        update_named_fields(&mut candidate, &selected_fields).map_err(|e| e.to_string())?;
        self.editor
            .commit_document(candidate)
            .map_err(|e| e.to_string())?;
        Ok("Fields updated.".into())
    }
}
