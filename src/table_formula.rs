//! Bounded A1-style table formula parsing and evaluation.
use crate::Table;
use std::fmt;

const MAX_FORMULA_BYTES: usize = 16 * 1024;
const MAX_WORK: usize = 100_000;
const MAX_DEPTH: usize = 128;
const MAX_CELLS: usize = 10_000;

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Blank,
    Text(String),
    Number(f64),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaError {
    EmptyFormula,
    TooLarge,
    InvalidSyntax,
    InvalidReference,
    OutOfRange,
    Cycle,
    DivisionByZero,
    DomainError,
    Nonnumeric,
    EmptyAggregate,
    TooComplex,
    NonFinite,
}
impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FormulaError {}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedTable {
    values: Vec<Vec<Result<CellValue, FormulaError>>>,
}
impl EvaluatedTable {
    pub fn values(&self) -> &[Vec<Result<CellValue, FormulaError>>] {
        &self.values
    }
    pub fn cell(&self, row: usize, column: usize) -> Option<&Result<CellValue, FormulaError>> {
        self.values.get(row).and_then(|r| r.get(column))
    }
    pub fn display_cells(&self) -> Vec<Vec<String>> {
        self.values
            .iter()
            .map(|row| row.iter().map(display_value).collect())
            .collect()
    }
}
fn display_value(value: &Result<CellValue, FormulaError>) -> String {
    match value {
        Ok(CellValue::Blank) => String::new(),
        Ok(CellValue::Text(text)) => text.clone(),
        Ok(CellValue::Number(number)) => number.to_string(),
        Err(error) => format!("#ERR:{error}"),
    }
}

#[derive(Clone, Debug)]
enum Expr {
    Number(f64),
    Ref(usize, usize),
    Range(usize, usize, usize, usize),
    Unary(char, Box<Expr>),
    Binary(char, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}
struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    nodes: usize,
    nesting: usize,
}
impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
            nodes: 0,
            nesting: 0,
        }
    }
    fn parse(mut self) -> Result<Expr, FormulaError> {
        self.skip_space();
        if self.peek() != Some(b'=') {
            return Err(FormulaError::InvalidSyntax);
        }
        self.position += 1;
        let expr = self.expression()?;
        self.skip_space();
        if self.position != self.input.len() {
            return Err(FormulaError::InvalidSyntax);
        }
        Ok(expr)
    }
    fn expression(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.term()?;
        loop {
            self.skip_space();
            let Some(op) = self.peek() else {
                break;
            };
            if op != b'+' && op != b'-' {
                break;
            }
            self.position += 1;
            self.node()?;
            left = Expr::Binary(op as char, Box::new(left), Box::new(self.term()?));
        }
        Ok(left)
    }
    fn term(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.power()?;
        loop {
            self.skip_space();
            let Some(op) = self.peek() else {
                break;
            };
            if op != b'*' && op != b'/' {
                break;
            }
            self.position += 1;
            self.node()?;
            left = Expr::Binary(op as char, Box::new(left), Box::new(self.power()?));
        }
        Ok(left)
    }
    fn power(&mut self) -> Result<Expr, FormulaError> {
        let left = self.unary()?;
        self.skip_space();
        if self.peek() == Some(b'^') {
            self.position += 1;
            self.node()?;
            self.enter()?;
            let right = self.power();
            self.leave();
            Ok(Expr::Binary('^', Box::new(left), Box::new(right?)))
        } else {
            Ok(left)
        }
    }
    fn unary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_space();
        let mut operators = Vec::new();
        while matches!(self.peek(), Some(b'+') | Some(b'-')) {
            let op = self.input[self.position] as char;
            self.position += 1;
            operators.push(op);
        }
        let mut value = self.primary()?;
        for op in operators.into_iter().rev() {
            self.node()?;
            value = Expr::Unary(op, Box::new(value));
        }
        Ok(value)
    }
    fn primary(&mut self) -> Result<Expr, FormulaError> {
        self.node()?;
        self.skip_space();
        if self.peek() == Some(b'(') {
            self.position += 1;
            self.enter()?;
            let value = self.expression();
            self.leave();
            let value = value?;
            self.skip_space();
            if self.take() != Some(b')') {
                return Err(FormulaError::InvalidSyntax);
            }
            return Ok(value);
        }
        if self
            .peek()
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return self.number();
        }
        let start = self.position;
        if self.peek() == Some(b'$') {
            self.position += 1;
        }
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            self.position += 1;
        }
        let name_start = if self.input.get(start) == Some(&b'$') {
            start + 1
        } else {
            start
        };
        let mut name = std::str::from_utf8(&self.input[name_start..self.position])
            .map_err(|_| FormulaError::InvalidSyntax)?;
        if name_start != start {
            name = std::str::from_utf8(&self.input[start..self.position])
                .map_err(|_| FormulaError::InvalidSyntax)?;
        }
        if name.is_empty() {
            return Err(FormulaError::InvalidSyntax);
        }
        self.skip_space();
        if self.peek() == Some(b'(') {
            self.position += 1;
            let mut args = Vec::new();
            self.skip_space();
            if self.peek() != Some(b')') {
                loop {
                    self.enter()?;
                    let expression = self.expression();
                    self.leave();
                    args.push(expression?);
                    self.skip_space();
                    if self.peek() != Some(b',') {
                        break;
                    }
                    self.position += 1;
                }
            }
            if self.take() != Some(b')') {
                return Err(FormulaError::InvalidSyntax);
            }
            return Ok(Expr::Call(name.to_ascii_uppercase(), args));
        }
        self.reference(name)
    }
    fn reference(&mut self, prefix: &str) -> Result<Expr, FormulaError> {
        let (column, _) = column_name(prefix)?;
        if self.peek() == Some(b'$') {
            self.position += 1;
        }
        let row_start = self.position;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        let row_text = std::str::from_utf8(&self.input[row_start..self.position])
            .map_err(|_| FormulaError::InvalidReference)?;
        let row = row_text
            .parse::<usize>()
            .map_err(|_| FormulaError::InvalidReference)?
            .checked_sub(1)
            .ok_or(FormulaError::InvalidReference)?;
        let first = (row, column);
        self.skip_space();
        if self.peek() == Some(b':') {
            self.position += 1;
            self.skip_space();
            let second = self.cell_reference()?;
            return Ok(Expr::Range(first.0, first.1, second.0, second.1));
        }
        if !row_text.is_empty() {
            Ok(Expr::Ref(row, column))
        } else {
            Err(FormulaError::InvalidReference)
        }
    }
    fn cell_reference(&mut self) -> Result<(usize, usize), FormulaError> {
        if self.peek() == Some(b'$') {
            self.position += 1;
        }
        let start = self.position;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.position += 1;
        }
        let name = std::str::from_utf8(&self.input[start..self.position])
            .map_err(|_| FormulaError::InvalidReference)?;
        let (column, _) = column_name(name)?;
        if self.peek() == Some(b'$') {
            self.position += 1;
        }
        let row_start = self.position;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        let row = std::str::from_utf8(&self.input[row_start..self.position])
            .map_err(|_| FormulaError::InvalidReference)?
            .parse::<usize>()
            .ok()
            .and_then(|v| v.checked_sub(1))
            .ok_or(FormulaError::InvalidReference)?;
        Ok((row, column))
    }
    fn number(&mut self) -> Result<Expr, FormulaError> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            self.position += 1;
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.position += 1;
            }
            let exponent_start = self.position;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
            if exponent_start == self.position {
                return Err(FormulaError::InvalidSyntax);
            }
        }
        let value = std::str::from_utf8(&self.input[start..self.position])
            .map_err(|_| FormulaError::InvalidSyntax)?
            .parse::<f64>()
            .map_err(|_| FormulaError::InvalidSyntax)?;
        if !value.is_finite() {
            return Err(FormulaError::NonFinite);
        }
        Ok(Expr::Number(value))
    }
    fn skip_space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }
    fn take(&mut self) -> Option<u8> {
        let value = self.peek();
        self.position += usize::from(value.is_some());
        value
    }
    fn node(&mut self) -> Result<(), FormulaError> {
        self.nodes += 1;
        if self.nodes > MAX_DEPTH * 32 {
            Err(FormulaError::TooComplex)
        } else {
            Ok(())
        }
    }
    fn enter(&mut self) -> Result<(), FormulaError> {
        self.nesting += 1;
        if self.nesting > MAX_DEPTH {
            Err(FormulaError::TooComplex)
        } else {
            Ok(())
        }
    }
    fn leave(&mut self) {
        self.nesting = self.nesting.saturating_sub(1);
    }
}
fn column_name(name: &str) -> Result<(usize, bool), FormulaError> {
    let mut locked = false;
    let name = name.strip_prefix('$').map_or(name, |value| {
        locked = true;
        value
    });
    if name.is_empty() {
        return Err(FormulaError::InvalidReference);
    }
    let mut value = 0usize;
    for byte in name.bytes() {
        if !byte.is_ascii_alphabetic() {
            return Err(FormulaError::InvalidReference);
        }
        value = value
            .checked_mul(26)
            .and_then(|v| v.checked_add(usize::from(byte.to_ascii_uppercase() - b'A' + 1)))
            .ok_or(FormulaError::TooComplex)?;
    }
    Ok((value - 1, locked))
}

pub fn evaluate_table(table: &Table) -> EvaluatedTable {
    let rows = table.cells().len();
    let columns = table.cells().first().map_or(0, Vec::len);
    if rows.saturating_mul(columns) > MAX_CELLS {
        return EvaluatedTable {
            values: vec![vec![Err(FormulaError::TooComplex); columns]; rows],
        };
    }
    let mut context = Context {
        table,
        values: vec![vec![None; columns]; rows],
        states: vec![vec![State::Fresh; columns]; rows],
        work: 0,
    };
    for row in 0..rows {
        for column in 0..columns {
            let _ = context.evaluate_cell(row, column, 0);
        }
    }
    EvaluatedTable {
        values: context
            .values
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|v| v.unwrap_or(Err(FormulaError::TooComplex)))
                    .collect()
            })
            .collect(),
    }
}
struct Context<'a> {
    table: &'a Table,
    values: Vec<Vec<Option<Result<CellValue, FormulaError>>>>,
    states: Vec<Vec<State>>,
    work: usize,
}
#[derive(Clone, Copy)]
enum State {
    Fresh,
    Active,
    Done,
}
impl<'a> Context<'a> {
    fn evaluate_cell(
        &mut self,
        row: usize,
        column: usize,
        depth: usize,
    ) -> Result<CellValue, FormulaError> {
        if row >= self.values.len() || column >= self.values.first().map_or(0, Vec::len) {
            return Err(FormulaError::OutOfRange);
        }
        if depth > MAX_DEPTH {
            return Err(FormulaError::TooComplex);
        }
        if matches!(self.states[row][column], State::Active) {
            return Err(FormulaError::Cycle);
        }
        if matches!(self.states[row][column], State::Done) {
            return self.values[row][column]
                .clone()
                .unwrap_or(Err(FormulaError::TooComplex));
        }
        self.work = self.work.checked_add(1).ok_or(FormulaError::TooComplex)?;
        if self.work > MAX_WORK
            || self
                .values
                .len()
                .saturating_mul(self.values.first().map_or(0, Vec::len))
                > MAX_CELLS
        {
            return Err(FormulaError::TooComplex);
        }
        self.states[row][column] = State::Active;
        let raw = &self.table.cells()[row][column];
        let trimmed = raw.trim();
        let result = if trimmed.is_empty() {
            Ok(CellValue::Blank)
        } else if trimmed.starts_with('=') {
            self.evaluate_formula(raw, depth + 1)
        } else if let Ok(number) = trimmed.parse::<f64>() {
            if number.is_finite() {
                Ok(CellValue::Number(number))
            } else {
                Err(FormulaError::NonFinite)
            }
        } else {
            Ok(CellValue::Text(raw.clone()))
        };
        self.states[row][column] = State::Done;
        self.values[row][column] = Some(result.clone());
        result
    }
    fn evaluate_formula(&mut self, raw: &str, depth: usize) -> Result<CellValue, FormulaError> {
        if raw.len() > MAX_FORMULA_BYTES {
            return Err(FormulaError::TooLarge);
        }
        let expression = Parser::new(raw).parse()?;
        Ok(CellValue::Number(self.number(&expression, depth)?))
    }
    fn number(&mut self, expression: &Expr, depth: usize) -> Result<f64, FormulaError> {
        if depth > MAX_DEPTH {
            return Err(FormulaError::TooComplex);
        }
        match expression {
            Expr::Number(value) => Ok(*value),
            Expr::Ref(row, column) => self.numeric_cell(*row, *column, depth + 1),
            Expr::Range(..) => Err(FormulaError::Nonnumeric),
            Expr::Unary(op, value) => {
                let n = self.number(value, depth + 1)?;
                if *op == '-' { Ok(-n) } else { Ok(n) }
            }
            Expr::Binary(op, left, right) => {
                let a = self.number(left, depth + 1)?;
                let b = self.number(right, depth + 1)?;
                let value = match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' if b == 0.0 => return Err(FormulaError::DivisionByZero),
                    '/' => a / b,
                    '^' => {
                        let value = a.powf(b);
                        if value.is_nan() {
                            return Err(FormulaError::DomainError);
                        }
                        value
                    }
                    _ => return Err(FormulaError::InvalidSyntax),
                };
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(FormulaError::NonFinite)
                }
            }
            Expr::Call(name, args) => self.call(name, args, depth + 1),
        }
    }
    fn numeric_cell(
        &mut self,
        row: usize,
        column: usize,
        depth: usize,
    ) -> Result<f64, FormulaError> {
        match self.evaluate_cell(row, column, depth)? {
            CellValue::Number(value) => Ok(value),
            CellValue::Blank | CellValue::Text(_) => Err(FormulaError::Nonnumeric),
        }
    }
    fn call(&mut self, name: &str, args: &[Expr], depth: usize) -> Result<f64, FormulaError> {
        if !matches!(name, "SUM" | "AVERAGE" | "COUNT") {
            return Err(FormulaError::InvalidSyntax);
        }
        let mut numbers = Vec::new();
        for arg in args {
            match arg {
                Expr::Range(a, b, c, d) => {
                    let row_start = (*a).min(*c);
                    let row_end = (*a).max(*c);
                    let column_start = (*b).min(*d);
                    let column_end = (*b).max(*d);
                    let span = row_end
                        .checked_sub(row_start)
                        .and_then(|value| value.checked_add(1))
                        .and_then(|height| {
                            column_end
                                .checked_sub(column_start)
                                .and_then(|width| width.checked_add(1))
                                .and_then(|width| height.checked_mul(width))
                        })
                        .ok_or(FormulaError::TooComplex)?;
                    if span > MAX_CELLS {
                        return Err(FormulaError::TooComplex);
                    }
                    for row in row_start..=row_end {
                        for column in column_start..=column_end {
                            self.work = self.work.checked_add(1).ok_or(FormulaError::TooComplex)?;
                            if self.work > MAX_WORK {
                                return Err(FormulaError::TooComplex);
                            }
                            match self.numeric_cell(row, column, depth + 1) {
                                Ok(value) => numbers.push(value),
                                Err(FormulaError::Nonnumeric) => {}
                                Err(error) => return Err(error),
                            }
                        }
                    }
                }
                expression => match self.number(expression, depth + 1) {
                    Ok(value) => numbers.push(value),
                    Err(FormulaError::Nonnumeric) => {}
                    Err(error) => return Err(error),
                },
            }
        }
        let sum = numbers.iter().sum::<f64>();
        if !sum.is_finite() {
            return Err(FormulaError::NonFinite);
        }
        match name {
            "SUM" => Ok(sum),
            "COUNT" => Ok(numbers.len() as f64),
            "AVERAGE" if numbers.is_empty() => Err(FormulaError::EmptyAggregate),
            "AVERAGE" => {
                let average = sum / numbers.len() as f64;
                if average.is_finite() {
                    Ok(average)
                } else {
                    Err(FormulaError::NonFinite)
                }
            }
            _ => Err(FormulaError::InvalidSyntax),
        }
    }
}

pub fn copy_formula(
    formula: &str,
    row_delta: isize,
    column_delta: isize,
) -> Result<String, FormulaError> {
    if formula.len() > MAX_FORMULA_BYTES || !formula.trim_start().starts_with('=') {
        return Err(FormulaError::InvalidSyntax);
    }
    let chars = formula.chars().collect::<Vec<_>>();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || (!chars[i - 1].is_ascii_alphanumeric() && chars[i - 1] != '_');
        if boundary && (chars[i] == '$' || chars[i].is_ascii_alphabetic()) {
            let start = i;
            let mut lock_col = false;
            if chars[i] == '$' {
                lock_col = true;
                i += 1;
            }
            let col_start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i == col_start {
                out.push(chars[start]);
                i = start + 1;
                continue;
            }
            let mut lock_row = false;
            if i < chars.len() && chars[i] == '$' {
                lock_row = true;
                i += 1;
            }
            let row_start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i == row_start {
                for character in &chars[start..i] {
                    out.push(*character);
                }
                continue;
            }
            let col_text = chars[col_start..if lock_row { row_start - 1 } else { row_start }]
                .iter()
                .collect::<String>();
            let row_text = chars[row_start..i].iter().collect::<String>();
            let (col, _) = column_name(&col_text)?;
            let row = row_text
                .parse::<isize>()
                .map_err(|_| FormulaError::InvalidReference)?;
            let next_col = if lock_col {
                col as isize
            } else {
                (col as isize)
                    .checked_add(column_delta)
                    .ok_or(FormulaError::OutOfRange)?
            };
            let next_row = if lock_row {
                row
            } else {
                row.checked_add(row_delta).ok_or(FormulaError::OutOfRange)?
            };
            if next_col < 0 || next_row < 1 {
                return Err(FormulaError::OutOfRange);
            }
            if lock_col {
                out.push('$');
            }
            out.push_str(&column_text(next_col as usize)?);
            if lock_row {
                out.push('$');
            }
            out.push_str(&next_row.to_string());
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Ok(out)
}
fn column_text(mut column: usize) -> Result<String, FormulaError> {
    let mut out = String::new();
    loop {
        out.push((b'A' + (column % 26) as u8) as char);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    Ok(out.chars().rev().collect())
}
