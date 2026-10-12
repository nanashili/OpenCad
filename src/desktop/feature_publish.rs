use super::*;
use crate::{DxfDocument, Layout, NamedPageSetup};
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
            "PUBLISH" => self.publish_command(tokens),
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
        if tokens
            .first()
            .is_some_and(|token| token.eq_ignore_ascii_case("LIST"))
        {
            if tokens.len() != 1 {
                return Err("PUBLISH LIST takes no arguments.".into());
            }
            self.message(self.layouts.names().collect::<Vec<_>>().join(", "), false);
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
        let request = parse_publish_request(tokens, &self.layouts)?;
        let layouts = self.prepare_publish_layouts(&request)?;
        let bytes = super::feature_layouts::vector_pdf_layouts(
            self.editor.document(),
            &layouts,
            self.path.as_deref(),
        )?;
        write_published_pdf(request.output(), &bytes)
    }

    fn prepare_publish_layouts(&self, request: &PublishRequest) -> Result<Vec<Layout>, String> {
        let override_setup = request
            .setup_name()
            .map(|name| {
                self.layouts
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
                let mut layout = self
                    .layouts
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
