use super::commands::number;
use super::*;
use crate::Entity;
use crate::table_formula::{copy_formula, evaluate_table};

struct TableCopyRequest {
    source_row: usize,
    source_column: usize,
    destination_row: usize,
    destination_column: usize,
}

struct TableFillRequest {
    row: usize,
    column: usize,
    count: usize,
    start: f64,
    step: f64,
    row_delta: usize,
    column_delta: usize,
}

fn parse_table_index(value: &str, label: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("{label} must be a whole number."))
}

impl TableCopyRequest {
    fn parse(tokens: &[&str]) -> Result<Self, String> {
        if tokens.len() != 4 {
            return Err(
                "TABLECOPY source-row source-column destination-row destination-column.".into(),
            );
        }
        Ok(Self {
            source_row: parse_table_index(tokens[0], "Source row")?,
            source_column: parse_table_index(tokens[1], "Source column")?,
            destination_row: parse_table_index(tokens[2], "Destination row")?,
            destination_column: parse_table_index(tokens[3], "Destination column")?,
        })
    }

    fn offsets(&self) -> Result<(isize, isize), String> {
        let source_row =
            isize::try_from(self.source_row).map_err(|_| "Source row is too large.")?;
        let source_column =
            isize::try_from(self.source_column).map_err(|_| "Source column is too large.")?;
        let destination_row =
            isize::try_from(self.destination_row).map_err(|_| "Destination row is too large.")?;
        let destination_column = isize::try_from(self.destination_column)
            .map_err(|_| "Destination column is too large.")?;
        Ok((
            destination_row
                .checked_sub(source_row)
                .ok_or("Table row offset is too large.")?,
            destination_column
                .checked_sub(source_column)
                .ok_or("Table column offset is too large.")?,
        ))
    }
}

impl TableFillRequest {
    fn parse(tokens: &[&str]) -> Result<Self, String> {
        if !(5..=6).contains(&tokens.len()) {
            return Err("TABLEFILL row column count start step [direction].".into());
        }
        let count = parse_table_index(tokens[2], "Count")?;
        if count == 0 || count > 10_000 {
            return Err("TABLEFILL count must be between 1 and 10,000.".into());
        }
        let direction = tokens.get(5).copied().unwrap_or("DOWN");
        let (row_delta, column_delta) = match direction.to_ascii_uppercase().as_str() {
            "DOWN" | "ROW" => (1, 0),
            "RIGHT" | "COLUMN" => (0, 1),
            _ => return Err("TABLEFILL direction must be DOWN or RIGHT.".into()),
        };
        Ok(Self {
            row: parse_table_index(tokens[0], "Row")?,
            column: parse_table_index(tokens[1], "Column")?,
            count,
            start: number(tokens[3])?,
            step: number(tokens[4])?,
            row_delta,
            column_delta,
        })
    }

    fn validate_range(&self, table: &crate::Table) -> Result<(), String> {
        let last_offset = self.count - 1;
        let last_row = self
            .row
            .checked_add(
                self.row_delta
                    .checked_mul(last_offset)
                    .ok_or("TABLEFILL range is too large.")?,
            )
            .ok_or("TABLEFILL range is too large.")?;
        let last_column = self
            .column
            .checked_add(
                self.column_delta
                    .checked_mul(last_offset)
                    .ok_or("TABLEFILL range is too large.")?,
            )
            .ok_or("TABLEFILL range is too large.")?;
        if last_row >= table.cells().len()
            || last_column >= table.cells().first().map_or(0, Vec::len)
        {
            return Err("TABLEFILL range is outside the TABLE.".into());
        }
        Ok(())
    }
}

/// Formula and bulk editing commands for the selected TABLE entity.
///
/// The table keeps its original cell strings so formulas remain editable and
/// are evaluated only for display or command output.
impl OpenCadApp {
    pub(super) fn feature_tables_command(
        &mut self,
        verb: &str,
        rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "TABLEFORMULA" => self.table_formula_command(rest, tokens),
            "TABLECOPY" => self.table_copy_command(tokens),
            "TABLEFILL" => self.table_fill_command(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn selected_table(&self) -> Result<crate::Table, String> {
        if self.editor.selection().len() != 1 {
            return Err("Select exactly one TABLE.".into());
        }
        let index = *self
            .editor
            .selection()
            .first()
            .ok_or("Select a TABLE first.")?;
        match self.editor.entities().get(index) {
            Some(Entity::Table(table)) => Ok(table.clone()),
            _ => Err("Selected entity is not a TABLE.".into()),
        }
    }

    fn replace_selected_table<F>(&mut self, edit: F) -> Result<(), String>
    where
        F: Fn(&crate::Table) -> Result<crate::Table, crate::GeometryError>,
    {
        let _ = self.selected_table()?;
        self.editor
            .map_selection_with(false, |entity| match entity {
                Entity::Table(table) => edit(table).map(Entity::Table),
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|error| error.to_string())
            .map(|_| ())
    }

    fn table_formula_command(&mut self, rest: &str, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .copied()
            .unwrap_or("GET")
            .to_ascii_uppercase();
        match operation.as_str() {
            "GET" | "LIST" => {
                let table = self.selected_table()?;
                let evaluated = evaluate_table(&table);
                let output = evaluated
                    .display_cells()
                    .into_iter()
                    .map(|row| row.join(" | "))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.message(output, false);
                Ok(())
            }
            "RECALCULATE" | "REGEN" => {
                let table = self.selected_table()?;
                let errors = evaluate_table(&table)
                    .values()
                    .iter()
                    .flat_map(|row| row.iter())
                    .filter(|value| value.is_err())
                    .count();
                self.message(
                    if errors == 0 {
                        "Table formulas recalculated."
                    } else {
                        "Table formulas recalculated with errors."
                    },
                    errors != 0,
                );
                Ok(())
            }
            "SET" => {
                let _ = self.selected_table()?;
                let formula_tokens =
                    super::commands::command_tokens(rest).map_err(|error| error.to_string())?;
                if formula_tokens.len() < 4 || !formula_tokens[0].eq_ignore_ascii_case("SET") {
                    return Err("TABLEFORMULA SET row column formula.".into());
                }
                let row = &formula_tokens[1];
                let column = &formula_tokens[2];
                let formula = formula_tokens[3..].join(" ");
                if !formula.trim_start().starts_with('=') {
                    return Err("TABLEFORMULA formula must begin with '='.".into());
                }
                let row = parse_table_index(row, "Row")?;
                let column = parse_table_index(column, "Column")?;
                self.replace_selected_table(|table| {
                    let mut next = table.clone();
                    next.set_cell(row, column, formula.to_owned())?;
                    Ok(next)
                })?;
                self.finish();
                self.message("Table formula updated.", false);
                Ok(())
            }
            _ => Err("TABLEFORMULA syntax: SET row column formula | GET | RECALCULATE.".into()),
        }
    }

    fn table_copy_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let request = TableCopyRequest::parse(tokens)?;
        let table = self.selected_table()?;
        let formula = table
            .cells()
            .get(request.source_row)
            .and_then(|row| row.get(request.source_column))
            .ok_or("Source table cell is out of range.")?;
        if !formula.trim_start().starts_with('=') {
            return Err("TABLECOPY source cell must contain a formula.".into());
        }
        let (row_delta, column_delta) = request.offsets()?;
        let copied =
            copy_formula(formula, row_delta, column_delta).map_err(|error| error.to_string())?;
        self.replace_selected_table(|table| {
            let mut next = table.clone();
            next.set_cell(
                request.destination_row,
                request.destination_column,
                copied.clone(),
            )?;
            Ok(next)
        })?;
        self.finish();
        self.message("Table formula copied.", false);
        Ok(())
    }

    fn table_fill_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let request = TableFillRequest::parse(tokens)?;
        let table = self.selected_table()?;
        request.validate_range(&table)?;
        self.replace_selected_table(move |table| {
            let mut next = table.clone();
            for offset in 0..request.count {
                let value = request.start + request.step * offset as f64;
                if !value.is_finite() {
                    return Err(crate::GeometryError::NonFiniteCoordinate);
                }
                next.set_cell(
                    request.row + request.row_delta * offset,
                    request.column + request.column_delta * offset,
                    value.to_string(),
                )?;
            }
            Ok(next)
        })?;
        self.finish();
        self.message("Table cells filled.", false);
        Ok(())
    }
}
