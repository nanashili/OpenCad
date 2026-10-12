use super::*;
use crate::{DxfDocument, Layout, NamedPageSetup, PublishDestination, PublishList, PublishSheet};
use std::{fs, path::Path};

impl OpenCadApp {
    pub(super) fn feature_publish_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "PAGESETUP"
                if tokens.first().is_some_and(|token| {
                    matches!(
                        token.to_ascii_uppercase().as_str(),
                        "LIST" | "SAVE" | "APPLY" | "RENAME" | "DELETE" | "IMPORT"
                    )
                }) =>
            {
                self.page_setup_named_command(tokens)
            }
            "PUBLISHLIST" => self.publish_list_command(tokens),
            "PUBLISH" => self.publish_command(tokens),
            "-PUBLISH" => self.publish_dsd_command(tokens),
            _ => return None,
        };
        let mutates_layouts = verb.eq_ignore_ascii_case("PAGESETUP")
            && tokens.first().is_some_and(|token| {
                matches!(
                    token.to_ascii_uppercase().as_str(),
                    "SAVE" | "APPLY" | "RENAME" | "DELETE" | "IMPORT"
                )
            });
        Some(if mutates_layouts {
            result.map(|()| {
                self.editor.set_layouts(self.layouts.clone());
            })
        } else {
            result
        })
    }

    fn page_setup_named_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .map(|token| token.to_ascii_uppercase())
            .ok_or("PAGESETUP syntax: LIST|SAVE|APPLY|RENAME|DELETE|IMPORT ... .")?;
        match operation.as_str() {
            "LIST" => {
                if tokens.len() != 1 {
                    return Err("PAGESETUP LIST takes no arguments.".into());
                }
                let entries = self
                    .layouts
                    .page_setups()
                    .entries()
                    .iter()
                    .map(|entry| entry.name().to_owned())
                    .collect::<Vec<_>>();
                self.message(
                    if entries.is_empty() {
                        "No named page setups are defined.".into()
                    } else {
                        entries.join(", ")
                    },
                    false,
                );
                Ok(())
            }
            "SAVE" => {
                if tokens.len() != 2 {
                    return Err("PAGESETUP SAVE name.".into());
                }
                let entry = NamedPageSetup::new(tokens[1], self.layouts.active().setup())
                    .map_err(|error| error.to_string())?;
                self.layouts
                    .page_setups_mut()
                    .insert_or_replace(entry)
                    .map_err(|error| error.to_string())
            }
            "APPLY" => {
                if tokens.len() != 2 && tokens.len() != 3 {
                    return Err("PAGESETUP APPLY name [layout].".into());
                }
                let layout = tokens
                    .get(2)
                    .map(|name| (*name).to_owned())
                    .unwrap_or_else(|| self.layouts.active().name().to_owned());
                self.layouts
                    .apply_page_setup(&layout, tokens[1])
                    .map_err(|error| error.to_string())
            }
            "RENAME" => {
                if tokens.len() != 3 {
                    return Err("PAGESETUP RENAME old new.".into());
                }
                self.layouts
                    .rename_page_setup(tokens[1], tokens[2])
                    .map_err(|error| error.to_string())
            }
            "DELETE" => {
                if tokens.len() != 2 {
                    return Err("PAGESETUP DELETE name.".into());
                }
                self.layouts
                    .remove_page_setup(tokens[1])
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
            "IMPORT" => self.import_page_setups(tokens),
            _ => Err("PAGESETUP syntax: LIST|SAVE|APPLY|RENAME|DELETE|IMPORT ... .".into()),
        }
    }

    fn import_page_setups(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 2 {
            return Err("PAGESETUP IMPORT source.dxf [name ...].".into());
        }
        let source = Path::new(tokens[1]);
        let source_entries = load_page_setup_source(source)?;
        let selected = select_page_setups(&source_entries, &tokens[2..])?;
        let mut candidate = self.layouts.clone();
        merge_page_setups(&mut candidate, &selected)?;
        self.layouts = candidate;
        Ok(())
    }

    fn publish_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.first().is_some_and(|token| {
            token.eq_ignore_ascii_case("DSD")
                || Path::new(token)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dsd"))
        }) {
            let path = if tokens[0].eq_ignore_ascii_case("DSD") {
                tokens.get(1).ok_or("PUBLISH DSD path.dsd [output.pdf].")?
            } else {
                tokens[0]
            };
            let output = if tokens[0].eq_ignore_ascii_case("DSD") {
                tokens.get(2).copied()
            } else {
                tokens.get(1).copied()
            };
            if tokens.len()
                > if tokens[0].eq_ignore_ascii_case("DSD") {
                    3
                } else {
                    2
                }
            {
                return Err("PUBLISH DSD path.dsd [output.pdf].".into());
            }
            return self.publish_dsd(Path::new(path), output.map(Path::new));
        }
        if tokens
            .first()
            .is_some_and(|token| token.eq_ignore_ascii_case("LIST"))
        {
            if tokens.len() != 1 {
                return Err("PUBLISH LIST takes no arguments.".into());
            }
            self.publish_list_panel = true;
            self.message("Publish list manager opened.", false);
            return Ok(());
        }
        if tokens.is_empty() {
            self.message(
                "PUBLISH output.pdf [ALL|layout-name ...] [SETUP name] [REVERSE]. Available layouts: "
                    .to_owned()
                    + &self.layouts.names().collect::<Vec<_>>().join(", "),
                false,
            );
            return Ok(());
        }
        let current_layouts = self.layouts_for_persistence()?;
        let request = parse_publish_request(tokens, &current_layouts)?;
        let layouts = self.prepare_publish_layouts(&request, &current_layouts)?;
        let mut document = self.editor.document().clone();
        document.set_layouts(current_layouts);
        let bytes =
            super::feature_layouts::vector_pdf_layouts(&document, &layouts, self.path.as_deref())?;
        write_published_pdf(request.output(), &bytes)
    }

    fn publish_list_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let operation = tokens
            .first()
            .map(|token| token.to_ascii_uppercase())
            .ok_or(
                "PUBLISHLIST NEW|LOAD|SAVE|ADD|REMOVE|MOVE|RENAME|SETUP|DESTINATION|LIST|PUBLISH.",
            )?;
        match operation.as_str() {
            "NEW" => self.new_publish_list(tokens),
            "LOAD" => self.load_publish_list(tokens),
            "SAVE" => self.save_publish_list(tokens),
            "ADD" => self.add_publish_sheet(tokens),
            "REMOVE" => self.remove_publish_sheet(tokens),
            "MOVE" => self.move_publish_sheet(tokens),
            "RENAME" => self.rename_publish_sheet(tokens),
            "SETUP" => self.set_publish_sheet_setup(tokens),
            "DESTINATION" => self.set_publish_destination(tokens),
            "LIST" => self.list_publish_sheets(tokens),
            "PUBLISH" => self.publish_loaded_list(tokens),
            _ => Err("Unknown PUBLISHLIST operation.".into()),
        }
    }

    fn new_publish_list(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() > 2 {
            return Err("PUBLISHLIST NEW [path.dsd].".into());
        }
        let drawing = self
            .path
            .clone()
            .ok_or("Save the current drawing before creating a publish list.")?;
        let dsd_path = tokens.get(1).map(PathBuf::from);
        let drawing = anchor_path(&drawing)?;
        let destination = drawing.with_extension("pdf");
        let mut list = PublishList::new(PublishDestination::pdf(destination));
        if let Some(path) = dsd_path.as_deref() {
            list.set_source_path(anchor_path(path)?);
        }
        list.add_sheet(
            PublishSheet::new(drawing, "Model", "Model").map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        self.publish_list = Some(list);
        self.publish_list_path = dsd_path;
        self.publish_list_panel = true;
        self.message("Publish list created.", false);
        Ok(())
    }

    fn load_publish_list(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 2 {
            return Err("PUBLISHLIST LOAD path.dsd.".into());
        }
        let path = anchor_path(Path::new(tokens[1]))?;
        let input = fs::read_to_string(&path)
            .map_err(|error| format!("Cannot read publish list {}: {error}", path.display()))?;
        let list = PublishList::parse_with_path(&input, Some(path.clone()))
            .map_err(|error| error.to_string())?;
        self.publish_list = Some(list);
        self.publish_list_path = Some(path);
        self.publish_list_panel = true;
        self.message("Publish list loaded.", false);
        Ok(())
    }

    fn save_publish_list(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() > 2 {
            return Err("PUBLISHLIST SAVE [path.dsd].".into());
        }
        let path = tokens
            .get(1)
            .map(PathBuf::from)
            .or_else(|| self.publish_list_path.clone())
            .ok_or("PUBLISHLIST SAVE requires a path.dsd.")?;
        let mut list = rebase_publish_list(self.publish_list()?.clone())?;
        let source_path = anchor_path(&path)?;
        list.set_source_path(&source_path);
        let bytes = list.to_dsd().map_err(|error| error.to_string())?;
        super::atomic_write(&path, bytes.as_bytes())
            .map_err(|error| format!("Cannot save publish list {}: {error}", path.display()))?;
        *self.publish_list_mut()? = list;
        self.publish_list_path = Some(path);
        self.message("Publish list saved.", false);
        Ok(())
    }

    fn add_publish_sheet(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !(3..=4).contains(&tokens.len()) {
            return Err("PUBLISHLIST ADD drawing.dxf layout [title].".into());
        }
        let path = PathBuf::from(tokens[1]);
        let layout = tokens[2].to_owned();
        let title = tokens.get(3).copied().unwrap_or(tokens[2]).to_owned();
        let mut candidate = self.publish_list()?.clone();
        candidate
            .add_sheet(PublishSheet {
                drawing_path: path,
                layout,
                title,
                named_page_setup: None,
                setup_source: None,
                original_sheet_path: None,
                unknown: Default::default(),
            })
            .map_err(|error| error.to_string())?;
        validate_publish_list(&candidate)?;
        *self.publish_list_mut()? = candidate;
        Ok(())
    }

    fn remove_publish_sheet(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 2 {
            return Err("PUBLISHLIST REMOVE sheet-number.".into());
        }
        let index = parse_sheet_index(tokens[1])?;
        let mut candidate = self.publish_list()?.clone();
        candidate
            .remove_sheet(index)
            .ok_or("Publish sheet number is out of range.")?;
        validate_publish_list(&candidate)?;
        *self.publish_list_mut()? = candidate;
        Ok(())
    }

    fn move_publish_sheet(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 3 {
            return Err("PUBLISHLIST MOVE from-number to-number.".into());
        }
        let from = parse_sheet_index(tokens[1])?;
        let to = parse_sheet_index(tokens[2])?;
        let mut candidate = self.publish_list()?.clone();
        candidate
            .reorder(from, to)
            .map_err(|error| error.to_string())?;
        validate_publish_list(&candidate)?;
        *self.publish_list_mut()? = candidate;
        Ok(())
    }

    fn rename_publish_sheet(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 3 {
            return Err("PUBLISHLIST RENAME sheet-number title.".into());
        }
        let index = parse_sheet_index(tokens[1])?;
        let mut candidate = self.publish_list()?.clone();
        let sheet = candidate
            .sheets
            .get_mut(index)
            .ok_or("Publish sheet number is out of range.")?;
        sheet.title = tokens[2].to_owned();
        validate_publish_list(&candidate)?;
        *self.publish_list_mut()? = candidate;
        Ok(())
    }

    fn set_publish_sheet_setup(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !(3..=4).contains(&tokens.len()) {
            return Err("PUBLISHLIST SETUP sheet-number name [source.dxf].".into());
        }
        let index = parse_sheet_index(tokens[1])?;
        let mut candidate = self.publish_list()?.clone();
        let sheet = candidate
            .sheets
            .get_mut(index)
            .ok_or("Publish sheet number is out of range.")?;
        sheet.named_page_setup = Some(tokens[2].to_owned());
        sheet.setup_source = tokens.get(3).map(PathBuf::from);
        validate_publish_list(&candidate)?;
        *self.publish_list_mut()? = candidate;
        Ok(())
    }

    fn set_publish_destination(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 2 {
            return Err("PUBLISHLIST DESTINATION output.pdf.".into());
        }
        let mut candidate = self.publish_list()?.clone();
        candidate.destination = PublishDestination::pdf(tokens[1]);
        validate_publish_list(&candidate)?;
        *self.publish_list_mut()? = candidate;
        Ok(())
    }

    fn list_publish_sheets(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 1 {
            return Err("PUBLISHLIST LIST takes no arguments.".into());
        }
        let list = self
            .publish_list()?
            .sheets
            .iter()
            .enumerate()
            .map(|(index, sheet)| format!("{}:{}:{}", index + 1, sheet.title, sheet.layout))
            .collect::<Vec<_>>();
        self.message(list.join("; "), false);
        self.publish_list_panel = true;
        Ok(())
    }

    fn publish_dsd_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !(1..=2).contains(&tokens.len()) {
            return Err("-PUBLISH path.dsd [output.pdf].".into());
        }
        let dsd = Path::new(tokens[0]);
        let output = tokens.get(1).map(Path::new);
        self.publish_dsd(dsd, output)
    }

    fn publish_loaded_list(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() > 2 {
            return Err("PUBLISHLIST PUBLISH [output.pdf].".into());
        }
        let list = self.publish_list()?.clone();
        let output = tokens.get(1).map(Path::new);
        self.publish_list_to_pdf(&list, output)
    }

    fn publish_dsd(&self, path: &Path, output_override: Option<&Path>) -> Result<(), String> {
        let path = anchor_path(path)?;
        let input = fs::read_to_string(&path)
            .map_err(|error| format!("Cannot read publish list {}: {error}", path.display()))?;
        let list = PublishList::parse_with_path(&input, Some(path.to_path_buf()))
            .map_err(|error| error.to_string())?;
        self.publish_list_to_pdf(&list, output_override)
    }

    fn publish_list_to_pdf(
        &self,
        list: &PublishList,
        output_override: Option<&Path>,
    ) -> Result<(), String> {
        list.validate_pdf().map_err(|error| error.to_string())?;
        let mut prepared = Vec::with_capacity(list.sheets.len());
        for (index, sheet) in list.sheets.iter().enumerate() {
            prepared.push(self.prepare_publish_sheet(list, sheet, index + 1)?);
        }
        let bytes = super::feature_layouts::vector_pdf_documents(&prepared)?;
        let output = output_override
            .map(PathBuf::from)
            .unwrap_or_else(|| list.resolved_destination());
        super::atomic_write(&output, &bytes)
            .map_err(|error| format!("Cannot write published PDF {}: {error}", output.display()))
    }

    fn prepare_publish_sheet(
        &self,
        list: &PublishList,
        sheet: &PublishSheet,
        number: usize,
    ) -> Result<(DxfDocument, Layout, Option<PathBuf>), String> {
        let drawing_path = list.resolve_path(&sheet.drawing_path);
        let (document, source_path) = self.load_publish_document(&drawing_path)?;
        let mut layout = find_publish_layout(&document, &sheet.layout).ok_or_else(|| {
            format!(
                "Publish sheet {number} layout {} was not found in {}.",
                sheet.layout,
                drawing_path.display()
            )
        })?;
        if let Some(setup_name) = sheet.named_page_setup.as_deref() {
            let setup_document = self.load_setup_document(list, sheet, &document)?;
            let setup = setup_document
                .layouts()
                .page_setups()
                .get(setup_name)
                .map_err(|error| {
                    format!(
                        "Named page setup {setup_name} was not found for sheet {number}: {error}"
                    )
                })?;
            layout.set_setup(setup.setup());
        }
        Ok((document, layout, Some(source_path)))
    }

    fn load_setup_document(
        &self,
        list: &PublishList,
        sheet: &PublishSheet,
        fallback: &DxfDocument,
    ) -> Result<DxfDocument, String> {
        let Some(source) = sheet.setup_source.as_deref() else {
            return Ok(fallback.clone());
        };
        let setup_path = list.resolve_path(source);
        ensure_supported_drawing(&setup_path)?;
        if self
            .path
            .as_deref()
            .is_some_and(|current| same_path(current, &setup_path))
        {
            let mut document = self.editor.document().clone();
            document.set_layouts(self.layouts_for_persistence()?);
            return Ok(document);
        }
        let input = fs::read_to_string(&setup_path).map_err(|error| {
            format!(
                "Cannot read page setup source {}: {error}",
                setup_path.display()
            )
        })?;
        DxfDocument::parse(&input).map_err(|error| {
            format!(
                "Cannot parse page setup source {}: {error}",
                setup_path.display()
            )
        })
    }

    fn load_publish_document(&self, path: &Path) -> Result<(DxfDocument, PathBuf), String> {
        ensure_supported_drawing(path)?;
        if self
            .path
            .as_deref()
            .is_some_and(|current| same_path(current, path))
        {
            let mut document = self.editor.document().clone();
            document.set_layouts(self.layouts_for_persistence()?);
            return Ok((document, path.to_path_buf()));
        }
        let input = fs::read_to_string(path)
            .map_err(|error| format!("Cannot read drawing {}: {error}", path.display()))?;
        let document = DxfDocument::parse(&input)
            .map_err(|error| format!("Cannot parse drawing {}: {error}", path.display()))?;
        Ok((document, path.to_path_buf()))
    }

    fn publish_list(&self) -> Result<&PublishList, String> {
        self.publish_list
            .as_ref()
            .ok_or_else(|| "No publish list is open.".to_owned())
    }

    fn publish_list_mut(&mut self) -> Result<&mut PublishList, String> {
        self.publish_list
            .as_mut()
            .ok_or_else(|| "No publish list is open.".to_owned())
    }

    fn prepare_publish_layouts(
        &self,
        request: &PublishRequest,
        source: &crate::LayoutCollection,
    ) -> Result<Vec<Layout>, String> {
        let override_setup = request
            .setup_name()
            .map(|name| {
                source
                    .page_setups()
                    .get(name)
                    .map(|entry| entry.setup())
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        request
            .layout_names()
            .iter()
            .map(|name| {
                let mut layout = source
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("Layout {name} was not found."))?;
                if let Some(setup) = override_setup {
                    layout.set_setup(setup);
                }
                Ok(layout)
            })
            .collect()
    }
}

struct PublishRequest {
    output: String,
    layout_names: Vec<String>,
    setup_name: Option<String>,
}

impl PublishRequest {
    fn output(&self) -> &str {
        &self.output
    }

    fn layout_names(&self) -> &[String] {
        &self.layout_names
    }

    fn setup_name(&self) -> Option<&str> {
        self.setup_name.as_deref()
    }
}

fn parse_publish_request(
    tokens: &[&str],
    layouts: &crate::LayoutCollection,
) -> Result<PublishRequest, String> {
    let output = tokens[0].to_owned();
    let available = layouts.names().map(str::to_owned).collect::<Vec<_>>();
    let mut names = Vec::new();
    let mut setup = None;
    let mut all = false;
    let mut reverse = false;
    let mut cursor = 1;
    while cursor < tokens.len() {
        let raw = tokens[cursor];
        match raw.to_ascii_uppercase().as_str() {
            "ALL" => {
                if all || !names.is_empty() {
                    return Err("PUBLISH ALL cannot be combined with layout names.".into());
                }
                all = true;
                cursor += 1;
            }
            "SETUP" => {
                let value = tokens
                    .get(cursor + 1)
                    .ok_or("PUBLISH SETUP requires a name.")?;
                if setup.replace((*value).to_owned()).is_some() {
                    return Err("PUBLISH accepts only one SETUP override.".into());
                }
                cursor += 2;
            }
            "REVERSE" => {
                if reverse {
                    return Err("PUBLISH accepts REVERSE only once.".into());
                }
                reverse = true;
                cursor += 1;
            }
            _ => {
                if all {
                    return Err("PUBLISH ALL cannot be combined with layout names.".into());
                }
                let canonical = available
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(raw))
                    .ok_or_else(|| format!("Layout {raw} was not found."))?;
                if names
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(canonical))
                {
                    return Err(format!("Layout {raw} was listed more than once."));
                }
                names.push(canonical.clone());
                cursor += 1;
            }
        }
    }
    if all || names.is_empty() {
        names = available;
    }
    if reverse {
        names.reverse();
    }
    Ok(PublishRequest {
        output,
        layout_names: names,
        setup_name: setup,
    })
}

fn write_published_pdf(output: &str, bytes: &[u8]) -> Result<(), String> {
    super::atomic_write(Path::new(output), bytes)
        .map_err(|error| format!("Cannot write published PDF {output}: {error}"))
}

fn parse_sheet_index(token: &str) -> Result<usize, String> {
    let number = token
        .parse::<usize>()
        .map_err(|_| format!("Invalid publish sheet number {token}."))?;
    number
        .checked_sub(1)
        .ok_or_else(|| "Publish sheet numbers start at 1.".to_owned())
}

fn find_publish_layout(document: &DxfDocument, requested: &str) -> Option<Layout> {
    document
        .layouts()
        .names()
        .find(|name| name.eq_ignore_ascii_case(requested))
        .and_then(|name| document.layouts().get(name))
        .cloned()
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn validate_publish_list(list: &PublishList) -> Result<(), String> {
    list.to_dsd().map(|_| ()).map_err(|error| error.to_string())
}

fn rebase_publish_list(mut list: PublishList) -> Result<PublishList, String> {
    let source = list.clone();
    let destination = anchor_path(&source.resolved_destination())?;
    for sheet in &mut list.sheets {
        sheet.drawing_path = anchor_path(&source.resolve_path(&sheet.drawing_path))?;
        sheet.setup_source = sheet
            .setup_source
            .as_deref()
            .map(|path| anchor_path(&source.resolve_path(path)))
            .transpose()?;
        sheet.original_sheet_path = sheet
            .original_sheet_path
            .as_deref()
            .map(|path| anchor_path(&source.resolve_path(path)))
            .transpose()?;
    }
    list.destination.target = destination;
    list.destination.output_directory = None;
    Ok(list)
}

fn anchor_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() || is_foreign_absolute(path) {
        return Ok(path.to_owned());
    }
    std::env::current_dir()
        .map(|directory| directory.join(path))
        .map_err(|error| {
            format!(
                "Cannot resolve relative publish path {}: {error}",
                path.display()
            )
        })
}

fn is_foreign_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.starts_with("\\\\") || text.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

fn ensure_supported_drawing(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dwg"))
    {
        return Err(format!(
            "Drawing {} is a DWG file; publish lists currently support DXF drawings only.",
            path.display()
        ));
    }
    Ok(())
}

fn load_page_setup_source(source: &Path) -> Result<Vec<NamedPageSetup>, String> {
    let input = fs::read_to_string(source).map_err(|error| {
        format!(
            "Cannot read page setup source {}: {error}",
            source.display()
        )
    })?;
    let document = DxfDocument::parse(&input).map_err(|error| error.to_string())?;
    Ok(document.layouts().page_setups().entries().to_vec())
}

fn select_page_setups(
    entries: &[NamedPageSetup],
    requested: &[&str],
) -> Result<Vec<NamedPageSetup>, String> {
    if requested.is_empty() {
        return Ok(entries.to_vec());
    }
    requested
        .iter()
        .map(|name| {
            entries
                .iter()
                .find(|entry| entry.name().eq_ignore_ascii_case(name))
                .cloned()
                .ok_or_else(|| format!("Source page setup {name} was not found."))
        })
        .collect()
}

fn merge_page_setups(
    layouts: &mut crate::LayoutCollection,
    entries: &[NamedPageSetup],
) -> Result<(), String> {
    for entry in entries {
        if layouts.page_setups().get(entry.name()).is_ok() {
            return Err(format!("Named page setup {} already exists.", entry.name()));
        }
    }
    for entry in entries {
        layouts
            .page_setups_mut()
            .add(entry.clone())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
