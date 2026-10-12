//! Interchange model for Autodesk DSD publish lists.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishListError {
    Invalid { line: usize, message: String },
    UnsupportedTarget(String),
}

impl fmt::Display for PublishListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { line, message } => write!(f, "DSD line {line}: {message}"),
            Self::UnsupportedTarget(target) => write!(f, "unsupported DSD target: {target}"),
        }
    }
}

impl std::error::Error for PublishListError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishTarget {
    Pdf,
    Unsupported(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishDestination {
    pub target: PathBuf,
    pub kind: PublishTarget,
    pub output_directory: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishSheet {
    pub drawing_path: PathBuf,
    pub layout: String,
    pub title: String,
    pub named_page_setup: Option<String>,
    pub setup_source: Option<PathBuf>,
    pub original_sheet_path: Option<PathBuf>,
    pub unknown: BTreeMap<String, String>,
}

impl PublishDestination {
    pub fn pdf(target: impl Into<PathBuf>) -> Self {
        Self {
            target: target.into(),
            kind: PublishTarget::Pdf,
            output_directory: None,
        }
    }

    pub fn with_output_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.output_directory = Some(directory.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishList {
    pub title: Option<String>,
    pub destination: PublishDestination,
    pub sheets: Vec<PublishSheet>,
    pub unknown_sections: Vec<PublishSection>,
    pub unknown_target: BTreeMap<String, String>,
    pub unknown_publish: BTreeMap<String, String>,
    source_path: Option<PathBuf>,
    newline: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishSection {
    pub name: String,
    pub keys: BTreeMap<String, String>,
}

impl PublishSheet {
    pub fn new(
        drawing_path: impl Into<PathBuf>,
        layout: impl Into<String>,
        title: impl Into<String>,
    ) -> Result<Self, PublishListError> {
        let sheet = Self {
            drawing_path: drawing_path.into(),
            layout: layout.into(),
            title: title.into(),
            named_page_setup: None,
            setup_source: None,
            original_sheet_path: None,
            unknown: BTreeMap::new(),
        };
        if sheet.drawing_path.as_os_str().is_empty()
            || sheet.layout.trim().is_empty()
            || sheet.title.trim().is_empty()
        {
            return Err(invalid(0, "sheet drawing path and layout are required"));
        }
        Ok(sheet)
    }
}

impl PublishList {
    pub fn new(destination: PublishDestination) -> Self {
        Self {
            title: None,
            destination,
            sheets: Vec::new(),
            unknown_sections: Vec::new(),
            unknown_target: BTreeMap::new(),
            unknown_publish: BTreeMap::new(),
            source_path: None,
            newline: "\n".to_owned(),
        }
    }

    pub fn with_source_path(mut self, source_path: impl Into<PathBuf>) -> Self {
        self.set_source_path(source_path);
        self
    }

    pub fn set_source_path(&mut self, source_path: impl Into<PathBuf>) {
        self.source_path = Some(source_path.into());
    }

    pub fn parse(input: &str) -> Result<Self, PublishListError> {
        Self::parse_with_path(input, None::<PathBuf>)
    }

    pub fn parse_with_path(
        input: &str,
        source_path: Option<PathBuf>,
    ) -> Result<Self, PublishListError> {
        let newline = if input.contains("\r\n") { "\r\n" } else { "\n" }.to_owned();
        let sections = parse_sections(input)?;
        let mut parsed = ParsedPublishList::default();
        for section in sections {
            parsed.consume(section)?;
        }
        parsed.finish(source_path, newline)
    }

    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        resolve_path(self.source_path.as_deref().and_then(Path::parent), path)
    }

    pub fn resolved_destination(&self) -> PathBuf {
        let target = &self.destination.target;
        let target_is_foreign_absolute = is_foreign_absolute(target);
        if target.is_absolute() || target_is_foreign_absolute {
            return target.to_owned();
        }

        if let Some(output_directory) = &self.destination.output_directory {
            let resolved_directory = self.resolve_path(output_directory);
            return resolve_path(Some(&resolved_directory), target);
        }

        self.resolve_path(target)
    }

    pub fn resolved_output_directory(&self) -> Option<PathBuf> {
        self.destination
            .output_directory
            .as_deref()
            .map(|path| self.resolve_path(path))
    }

    pub fn reorder(&mut self, from: usize, to: usize) -> Result<(), PublishListError> {
        if from >= self.sheets.len() || to >= self.sheets.len() {
            return Err(invalid(0, "sheet reorder index out of range"));
        }
        let sheet = self.sheets.remove(from);
        self.sheets.insert(to, sheet);
        Ok(())
    }

    pub fn add_sheet(&mut self, sheet: PublishSheet) -> Result<(), PublishListError> {
        if self
            .sheets
            .iter()
            .any(|existing| existing.title.eq_ignore_ascii_case(&sheet.title))
        {
            return Err(invalid(0, "duplicate sheet title"));
        }
        self.sheets.push(sheet);
        Ok(())
    }

    pub fn remove_sheet(&mut self, index: usize) -> Option<PublishSheet> {
        (index < self.sheets.len()).then(|| self.sheets.remove(index))
    }

    pub fn validate_pdf(&self) -> Result<(), PublishListError> {
        match self.destination.kind {
            PublishTarget::Pdf => Ok(()),
            PublishTarget::Unsupported(ref target) => {
                Err(PublishListError::UnsupportedTarget(target.clone()))
            }
        }
    }

    pub fn to_dsd(&self) -> Result<String, PublishListError> {
        self.validate_for_write()?;
        let nl = &self.newline;
        let mut out = self.serialize_version_section();
        out.push_str("[Target]");
        out.push_str(nl);
        out.push_str(&format!(
            "Type=6{nl}DWF={}{nl}",
            quote(&self.destination.target)
        ));
        if let Some(output_directory) = &self.destination.output_directory {
            out.push_str(&format!("OUT={}{}", quote(output_directory), nl));
        }
        for (key, value) in &self.unknown_target {
            out.push_str(&format!("{key}={value}{nl}"));
        }
        if self.title.is_some() || !self.unknown_publish.is_empty() {
            out.push_str(&format!("[Publish]{nl}"));
            if let Some(title) = &self.title {
                out.push_str(&format!("Title={}{nl}", quote(title)));
            }
            for (key, value) in &self.unknown_publish {
                out.push_str(&format!("{key}={value}{nl}"));
            }
        }
        for sheet in &self.sheets {
            out.push_str(&format!("[DWF6Sheet:{}]{}", sheet.title, nl));
            out.push_str(&format!(
                "DWG={}{}Layout={}{}Title={}{}",
                quote(&sheet.drawing_path),
                nl,
                quote(&sheet.layout),
                nl,
                quote(&sheet.title),
                nl
            ));
            if let Some(setup) = &sheet.named_page_setup {
                let setup_value = match &sheet.setup_source {
                    Some(source) => format!("{}|{}", setup, source.to_string_lossy()),
                    None => setup.clone(),
                };
                out.push_str(&format!("Setup={}{}", quote(setup_value), nl));
            }
            if let Some(original_sheet_path) = &sheet.original_sheet_path {
                out.push_str(&format!(
                    "OriginalSheetPath={}{}",
                    quote(original_sheet_path),
                    nl
                ));
            }
            for (key, value) in &sheet.unknown {
                out.push_str(&format!("{key}={value}{nl}"));
            }
        }
        for section in &self.unknown_sections {
            if section.name.eq_ignore_ascii_case("DWF6Version") {
                continue;
            }
            out.push_str(&format!("[{}]{nl}", section.name));
            for (key, value) in &section.keys {
                out.push_str(&format!("{key}={value}{nl}"));
            }
        }
        Ok(out)
    }

    fn validate_for_write(&self) -> Result<(), PublishListError> {
        self.validate_pdf()?;
        if self.sheets.is_empty() {
            return Err(invalid(0, "DSD contains no sheets"));
        }
        if let Some(title) = &self.title {
            validate_value(title)?;
        }
        validate_path_value(&self.destination.target)?;
        if let Some(output_directory) = &self.destination.output_directory {
            validate_path_value(output_directory)?;
        }
        let mut titles = std::collections::HashSet::new();
        for sheet in &self.sheets {
            if sheet.drawing_path.as_os_str().is_empty()
                || sheet.layout.trim().is_empty()
                || sheet.title.trim().is_empty()
            {
                return Err(invalid(0, "sheet drawing path and layout are required"));
            }
            if !titles.insert(sheet.title.to_ascii_lowercase()) {
                return Err(invalid(0, "duplicate sheet title"));
            }
            validate_path_value(&sheet.drawing_path)?;
            validate_value(&sheet.layout)?;
            validate_value(&sheet.title)?;
            if let Some(setup) = &sheet.named_page_setup {
                validate_value(setup)?;
            }
            if let Some(source) = &sheet.setup_source {
                if sheet.named_page_setup.is_none() {
                    return Err(invalid(0, "setup source requires a named page setup"));
                }
                validate_path_value(source)?;
            }
            if let Some(original_sheet_path) = &sheet.original_sheet_path {
                validate_path_value(original_sheet_path)?;
            }
            validate_pairs(&sheet.unknown)?;
            validate_unknown_keys(
                &sheet.unknown,
                &["dwg", "layout", "title", "setup", "originalsheetpath"],
            )?;
        }
        validate_pairs(&self.unknown_target)?;
        validate_unknown_keys(&self.unknown_target, &["dwf", "type", "out"])?;
        validate_pairs(&self.unknown_publish)?;
        validate_unknown_keys(&self.unknown_publish, &["title"])?;
        for section in &self.unknown_sections {
            validate_key(&section.name)?;
            validate_pairs(&section.keys)?;
        }
        Ok(())
    }

    fn serialize_version_section(&self) -> String {
        let nl = &self.newline;
        let mut out = format!("[DWF6Version]{nl}");
        if let Some(section) = self
            .unknown_sections
            .iter()
            .find(|section| section.name.eq_ignore_ascii_case("DWF6Version"))
        {
            for (key, value) in &section.keys {
                out.push_str(&format!("{key}={value}{nl}"));
            }
        } else {
            out.push_str(&format!("Ver=1{nl}"));
        }
        out
    }
}

#[derive(Default)]
struct ParsedPublishList {
    title: Option<String>,
    destination: Option<PublishDestination>,
    sheets: Vec<PublishSheet>,
    unknown_sections: Vec<PublishSection>,
    unknown_target: BTreeMap<String, String>,
    unknown_publish: BTreeMap<String, String>,
    target_seen: bool,
}

impl ParsedPublishList {
    fn consume(&mut self, section: ParsedSection) -> Result<(), PublishListError> {
        if section.name.eq_ignore_ascii_case("Target") {
            if self.target_seen {
                return Err(invalid(section.line, "duplicate [Target] section"));
            }
            self.target_seen = true;
            let path = required_key(&section, "DWF", section.line)?;
            let kind = match key(&section, "Type") {
                Some(value) if value == "6" || value.eq_ignore_ascii_case("pdf") => {
                    PublishTarget::Pdf
                }
                Some(value) => PublishTarget::Unsupported(value),
                None if path.to_ascii_lowercase().ends_with(".pdf") => PublishTarget::Pdf,
                None => PublishTarget::Unsupported("unknown".to_owned()),
            };
            self.destination = Some(PublishDestination {
                target: PathBuf::from(path),
                kind,
                output_directory: key(&section, "OUT").map(PathBuf::from),
            });
            collect_unknown(&section, &["dwf", "type", "out"], &mut self.unknown_target);
        } else if section.name.eq_ignore_ascii_case("Publish") {
            self.title = key(&section, "Title");
            collect_unknown(&section, &["title"], &mut self.unknown_publish);
        } else if section.name.to_ascii_lowercase().starts_with("sheet:")
            || section.name.to_ascii_lowercase().starts_with("dwf6sheet:")
        {
            self.sheets.push(parse_sheet(&section)?);
        } else {
            self.unknown_sections.push(PublishSection {
                name: section.name,
                keys: section.keys,
            });
        }
        Ok(())
    }

    fn finish(
        self,
        source_path: Option<PathBuf>,
        newline: String,
    ) -> Result<PublishList, PublishListError> {
        let destination = self
            .destination
            .ok_or_else(|| invalid(1, "missing [Target] section"))?;
        let mut titles = std::collections::HashSet::new();
        for sheet in &self.sheets {
            if !titles.insert(sheet.title.to_ascii_lowercase()) {
                return Err(invalid(1, "duplicate sheet title"));
            }
        }
        if self.sheets.is_empty() {
            return Err(invalid(1, "DSD contains no sheets"));
        }
        Ok(PublishList {
            title: self.title,
            destination,
            sheets: self.sheets,
            unknown_sections: self.unknown_sections,
            unknown_target: self.unknown_target,
            unknown_publish: self.unknown_publish,
            source_path,
            newline,
        })
    }
}

#[derive(Clone, Debug)]
struct ParsedSection {
    name: String,
    keys: BTreeMap<String, String>,
    line: usize,
}

fn parse_sections(input: &str) -> Result<Vec<ParsedSection>, PublishListError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut sections = Vec::new();
    let mut current: Option<ParsedSection> = None;
    for (index, raw) in input.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with(';') {
            continue;
        }
        if text.starts_with('[') && text.ends_with(']') {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(ParsedSection {
                name: text[1..text.len() - 1].trim().to_owned(),
                keys: BTreeMap::new(),
                line,
            });
            continue;
        }
        let Some((key, value)) = text.split_once('=') else {
            return Err(invalid(line, "expected key=value"));
        };
        let Some(section) = current.as_mut() else {
            return Err(invalid(line, "key before section"));
        };
        if key.trim().is_empty() {
            return Err(invalid(line, "empty key"));
        }
        if value.trim().starts_with('"') != value.trim().ends_with('"') {
            return Err(invalid(line, "unmatched quoted value"));
        }
        if section
            .keys
            .keys()
            .any(|existing| existing.eq_ignore_ascii_case(key.trim()))
        {
            return Err(invalid(line, "duplicate key"));
        }
        section
            .keys
            .insert(key.trim().to_owned(), value.trim().to_owned());
    }
    if let Some(section) = current {
        sections.push(section);
    }
    Ok(sections)
}

fn collect_unknown(section: &ParsedSection, known: &[&str], target: &mut BTreeMap<String, String>) {
    for (key, value) in &section.keys {
        if !known.iter().any(|known| key.eq_ignore_ascii_case(known)) {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn parse_sheet(section: &ParsedSection) -> Result<PublishSheet, PublishListError> {
    let drawing = required_key(section, "DWG", section.line)?;
    let layout = required_key(section, "Layout", section.line)?;
    let default_title = section
        .name
        .split_once(':')
        .map_or_else(|| section.name.clone(), |(_, title)| unquote(title));
    let title = key(section, "Title").unwrap_or(default_title);
    let setup = key(section, "Setup").filter(|value| !value.trim().is_empty());
    let (named_page_setup, setup_source) = setup
        .map(|value| match value.split_once('|') {
            Some((name, source)) => (Some(name.to_owned()), Some(PathBuf::from(source))),
            None => (Some(value), None),
        })
        .unwrap_or((None, None));
    let mut unknown = BTreeMap::new();
    for (key_name, value) in &section.keys {
        if !["dwg", "layout", "title", "setup", "originalsheetpath"]
            .contains(&key_name.to_ascii_lowercase().as_str())
        {
            unknown.insert(key_name.clone(), value.clone());
        }
    }
    Ok(PublishSheet {
        drawing_path: PathBuf::from(drawing),
        layout,
        title,
        named_page_setup,
        setup_source,
        original_sheet_path: key(section, "OriginalSheetPath").map(PathBuf::from),
        unknown,
    })
}

fn key(section: &ParsedSection, wanted: &str) -> Option<String> {
    section
        .keys
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(wanted))
        .map(|(_, value)| unquote(value))
}
fn required_key(
    section: &ParsedSection,
    wanted: &str,
    line: usize,
) -> Result<String, PublishListError> {
    let value = key(section, wanted).ok_or_else(|| invalid(line, format!("missing {wanted}")))?;
    if value.trim().is_empty() {
        return Err(invalid(line, format!("empty {wanted}")));
    }
    Ok(value)
}
fn resolve_path(base: Option<&Path>, path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let foreign_absolute = is_foreign_absolute(path);
    if path.is_absolute() || foreign_absolute {
        path.to_owned()
    } else {
        #[cfg(unix)]
        let normalized = PathBuf::from(text.replace('\\', "/"));
        #[cfg(not(unix))]
        let normalized = path.to_owned();
        base.map_or(normalized.clone(), |base| base.join(normalized))
    }
}

fn is_foreign_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("\\\\") || text.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}
fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].replace("\\\"", "\"")
    } else {
        value.to_owned()
    }
}
fn quote(value: impl AsRef<Path>) -> String {
    let value = value.as_ref().to_string_lossy();
    if value.contains([' ', '\t', '=']) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.into_owned()
    }
}
fn validate_value(value: &str) -> Result<(), PublishListError> {
    if value.contains(['\r', '\n', '[', ']']) {
        Err(invalid(0, "DSD value contains structural characters"))
    } else {
        Ok(())
    }
}

fn validate_key(key: &str) -> Result<(), PublishListError> {
    if key.trim().is_empty() || key.contains(['\r', '\n', '[', ']', '=']) {
        Err(invalid(0, "DSD key contains structural characters"))
    } else {
        Ok(())
    }
}

fn validate_path_value(value: &Path) -> Result<(), PublishListError> {
    validate_value(&value.to_string_lossy())
}
fn validate_pairs(values: &BTreeMap<String, String>) -> Result<(), PublishListError> {
    for (key, value) in values {
        validate_key(key)?;
        validate_value(value)?;
    }
    Ok(())
}

fn validate_unknown_keys(
    values: &BTreeMap<String, String>,
    reserved: &[&str],
) -> Result<(), PublishListError> {
    for key in values.keys() {
        if reserved
            .iter()
            .any(|reserved| key.eq_ignore_ascii_case(reserved))
        {
            return Err(invalid(0, format!("duplicate reserved DSD key: {key}")));
        }
    }
    Ok(())
}
fn invalid(line: usize, message: impl Into<String>) -> PublishListError {
    PublishListError::Invalid {
        line,
        message: message.into(),
    }
}
