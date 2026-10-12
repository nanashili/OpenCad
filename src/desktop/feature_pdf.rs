//! PDF underlay command handlers.
//!
//! The handler keeps PDF data external: attaching stores a validated path,
//! page, placement and display settings in `PdfUnderlay`. Rasterisation is a
//! renderer concern, while command validation still reads the referenced PDF.

use super::OpenCadApp;
use super::commands::{command_tokens, parse_point};
use crate::{Entity, PdfUnderlay};
use std::fs;
use std::path::{Path, PathBuf};

impl OpenCadApp {
    pub(super) fn feature_pdf_command(
        &mut self,
        verb: &str,
        rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "PDFATTACH" => self.pdf_attach(rest, tokens),
            "PDFCLIP" => self.pdf_clip(tokens),
            "PDFADJUST" => self.pdf_adjust(tokens),
            "PDFFRAME" => self.pdf_frame(tokens),
            "PDFRELOAD" => self.pdf_reload(tokens),
            "PDFDETACH" => self.pdf_detach(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn pdf_attach(&mut self, rest: &str, tokens: &[&str]) -> Result<(), String> {
        let args = self.pdf_attach_args(rest, tokens)?;
        let operation = args.first().map(String::as_str).unwrap_or("");
        if operation.eq_ignore_ascii_case("LIST") || operation.eq_ignore_ascii_case("PAGES") {
            self.pdf_list_pages(&args)?;
            return Ok(());
        }
        let underlay = self.prepare_pdf_underlay(&args)?;
        self.editor
            .add(vec![Entity::PdfUnderlay(underlay)])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn pdf_attach_args(&self, rest: &str, tokens: &[&str]) -> Result<Vec<String>, String> {
        let parsed = command_tokens(rest)?;
        Ok(if parsed.is_empty() {
            tokens.iter().map(|token| (*token).to_owned()).collect()
        } else {
            parsed
        })
    }

    fn pdf_list_pages(&mut self, args: &[String]) -> Result<(), String> {
        let raw_path = args.get(1).ok_or("PDFATTACH LIST path.")?;
        let path = self.resolve_pdf_path(raw_path);
        let info = read_pdf_info(&path)?;
        let first_page = info.pages.first();
        self.message(
            format!(
                "{}: {} page(s), first page {:.6} x {:.6} points.",
                path.display(),
                info.page_count,
                first_page.map_or(0.0, |page| f64::from(page.width_points)),
                first_page.map_or(0.0, |page| f64::from(page.height_points))
            ),
            false,
        );
        Ok(())
    }

    fn prepare_pdf_underlay(&self, args: &[String]) -> Result<PdfUnderlay, String> {
        if args.len() < 3 || args.len() > 5 {
            return Err(
                "PDFATTACH syntax: PDFATTACH path page insertion [scale] [rotation-degrees]."
                    .into(),
            );
        }
        let path = self.resolve_pdf_path(&args[0]);
        let info = read_pdf_info(&path)?;
        let page = args[1]
            .parse::<u32>()
            .map_err(|_| "PDF page must be a positive whole number.")?;
        if page == 0 || page > info.page_count as u32 {
            return Err(format!(
                "PDF page must be between 1 and {}.",
                info.page_count
            ));
        }
        let insertion = parse_point(&args[2], self.drawing_elevation())?;
        let scale = args
            .get(3)
            .map_or(Ok(1.0), |value| parse_finite(value, "PDF scale"))?;
        if scale == 0.0 {
            return Err("PDF scale must be non-zero.".into());
        }
        let rotation = args
            .get(4)
            .map_or(Ok(0.0), |value| parse_finite(value, "PDF rotation"))?;
        let page_info = info.pages[(page - 1) as usize];
        let page_size = [
            f64::from(page_info.width_points),
            f64::from(page_info.height_points),
        ];
        let mut underlay = PdfUnderlay::new(&path, page, page_size, insertion)
            .map_err(|error| error.to_string())?;
        underlay
            .set_scale([scale, scale])
            .map_err(|error| error.to_string())?;
        underlay
            .set_rotation(rotation.to_radians())
            .map_err(|error| error.to_string())?;
        Ok(underlay)
    }

    fn pdf_clip(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (index, args) = self.pdf_entity_args(tokens)?;
        let mut underlay = self.pdf_at(index)?.clone();
        let operation = args.first().copied().unwrap_or("").to_ascii_uppercase();
        match operation.as_str() {
            "ON" | "OFF" => underlay.set_clip_enabled(operation == "ON"),
            "DELETE" | "CLEAR" => underlay.set_clip(None).map_err(|error| error.to_string())?,
            "RECT" => {
                if args.len() != 3 {
                    return Err("PDFCLIP RECT requires two normalized page points.".into());
                }
                let a = parse_clip_point(args[1])?;
                let b = parse_clip_point(args[2])?;
                let min = [a[0].min(b[0]), a[1].min(b[1])];
                let max = [a[0].max(b[0]), a[1].max(b[1])];
                underlay
                    .set_clip(Some(vec![min, [max[0], min[1]], max, [min[0], max[1]]]))
                    .map_err(|error| error.to_string())?;
                underlay.set_clip_enabled(true);
            }
            "POLY" => {
                if args.len() < 4 {
                    return Err("PDFCLIP POLY requires at least three normalized points.".into());
                }
                let points = args[1..]
                    .iter()
                    .map(|token| parse_clip_point(token))
                    .collect::<Result<Vec<_>, _>>()?;
                underlay
                    .set_clip(Some(points))
                    .map_err(|error| error.to_string())?;
                underlay.set_clip_enabled(true);
            }
            _ => {
                return Err(
                    "PDFCLIP syntax: PDFCLIP entity ON|OFF|DELETE|RECT p1 p2|POLY p1 p2 p3..."
                        .into(),
                );
            }
        }
        self.editor
            .update(index, Entity::PdfUnderlay(underlay))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn pdf_adjust(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (index, args) = self.pdf_entity_args(tokens)?;
        let mut underlay = self.pdf_at(index)?.clone();
        if args.len() == 2 && args[0].eq_ignore_ascii_case("DISPLAY") {
            let enabled = parse_on_off(args[1], "PDF display")?;
            underlay.set_display_enabled(enabled);
        } else if args.len() == 2 && args[0].eq_ignore_ascii_case("FRAME") {
            let mode = parse_frame_mode(args[1])?;
            underlay
                .set_frame_mode(mode)
                .map_err(|error| error.to_string())?;
        } else if args.len() == 3 {
            let fade = parse_finite(args[0], "PDF fade")?;
            let contrast = parse_finite(args[1], "PDF contrast")?;
            let monochrome = parse_on_off(args[2], "PDF monochrome")?;
            underlay.set_fade(fade).map_err(|error| error.to_string())?;
            underlay
                .set_contrast(contrast)
                .map_err(|error| error.to_string())?;
            underlay.set_monochrome(monochrome);
        } else {
            return Err(
                "PDFADJUST syntax: PDFADJUST entity fade contrast monochrome(ON|OFF), DISPLAY ON|OFF, or FRAME 0|1|2.".into(),
            );
        }
        self.editor
            .update(index, Entity::PdfUnderlay(underlay))
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn pdf_frame(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.is_empty() {
            let count = self
                .editor
                .entities()
                .iter()
                .filter_map(|entity| match entity {
                    Entity::PdfUnderlay(underlay) => Some(underlay.frame_mode()),
                    _ => None,
                })
                .next()
                .ok_or("No PDF underlays are present.")?;
            self.message(format!("PDF frame mode: {count}."), false);
            return Ok(());
        }
        if tokens.len() != 1 {
            return Err("PDFFRAME syntax: PDFFRAME 0|1|2.".into());
        }
        let mode = parse_frame_mode(tokens[0])?;
        let mut candidate = self.editor.document().clone();
        let mut changed = false;
        for index in 0..candidate.entities().len() {
            let Some(Entity::PdfUnderlay(mut underlay)) = candidate.entities().get(index).cloned()
            else {
                continue;
            };
            underlay
                .set_frame_mode(mode)
                .map_err(|error| error.to_string())?;
            if candidate
                .replace_entity(index, Entity::PdfUnderlay(underlay))
                .is_some()
            {
                changed = true;
            }
        }
        if !changed {
            return Err("No PDF underlays are present.".into());
        }
        self.editor
            .commit_document(candidate)
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn pdf_reload(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (index, _) = self.pdf_entity_args(tokens)?;
        let underlay = self.pdf_at(index)?;
        let path = self.resolve_pdf_path(&underlay.path().to_string_lossy());
        let info = read_pdf_info(&path)?;
        let mut refreshed = underlay.clone();
        let page_info = info
            .pages
            .get(underlay.page().saturating_sub(1) as usize)
            .ok_or_else(|| format!("PDF page {} is no longer available.", underlay.page()))?;
        refreshed
            .set_page_size([
                f64::from(page_info.width_points),
                f64::from(page_info.height_points),
            ])
            .map_err(|error| error.to_string())?;
        self.editor
            .update(index, Entity::PdfUnderlay(refreshed))
            .map_err(|error| error.to_string())?;
        self.camera.clear_pdf_cache();
        self.message(format!("PDF underlay {} reloaded.", path.display()), false);
        Ok(())
    }

    fn pdf_detach(&mut self, tokens: &[&str]) -> Result<(), String> {
        let index = self.pdf_index(tokens)?;
        self.editor.select(vec![index]);
        self.editor
            .delete_selection()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn pdf_entity_args<'a>(&self, tokens: &'a [&'a str]) -> Result<(usize, &'a [&'a str]), String> {
        let index = self.pdf_index(tokens)?;
        let consumed = if tokens
            .first()
            .is_some_and(|token| token.parse::<usize>().is_ok())
        {
            1
        } else {
            0
        };
        Ok((index, &tokens[consumed..]))
    }

    fn pdf_index(&self, tokens: &[&str]) -> Result<usize, String> {
        if let Some(token) = tokens.first()
            && let Ok(number) = token.parse::<usize>()
        {
            let index = number
                .checked_sub(1)
                .ok_or("Entity numbers must be positive whole numbers.")?;
            if matches!(
                self.editor.entities().get(index),
                Some(Entity::PdfUnderlay(_))
            ) {
                return Ok(index);
            }
            return Err(format!("Entity {index} is not a PDF underlay."));
        }
        self.editor
            .selection()
            .iter()
            .copied()
            .find(|index| {
                matches!(
                    self.editor.entities().get(*index),
                    Some(Entity::PdfUnderlay(_))
                )
            })
            .ok_or("Select a PDF underlay or provide its entity index.".into())
    }

    fn pdf_at(&self, index: usize) -> Result<&PdfUnderlay, String> {
        match self.editor.entities().get(index) {
            Some(Entity::PdfUnderlay(underlay)) => Ok(underlay),
            _ => Err(format!("Entity {index} is not a PDF underlay.")),
        }
    }

    fn resolve_pdf_path(&self, raw: &str) -> PathBuf {
        let path = Path::new(raw);
        if path.is_absolute() {
            return path.to_owned();
        }
        self.path
            .as_deref()
            .and_then(Path::parent)
            .map_or_else(|| path.to_owned(), |parent| parent.join(path))
    }
}

fn read_pdf_info(path: &Path) -> Result<crate::PdfInfo, String> {
    let data =
        fs::read(path).map_err(|error| format!("Cannot read PDF {}: {error}.", path.display()))?;
    crate::pdf_info(&data)
        .map_err(|error| format!("Cannot inspect PDF {}: {error}.", path.display()))
}

fn parse_finite(value: &str, label: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("{label} must be a finite number."))?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(format!("{label} must be a finite number."))
    }
}

fn parse_clip_point(value: &str) -> Result<[f64; 2], String> {
    let mut values = value
        .split(',')
        .map(|part| parse_finite(part, "Clip coordinate"));
    let point = [
        values.next().ok_or("Clip point requires x,y.")??,
        values.next().ok_or("Clip point requires x,y.")??,
    ];
    if values.next().is_some() || point.iter().any(|value| !(0.0..=1.0).contains(value)) {
        return Err("Clip points must be normalized x,y values in the range 0..1.".into());
    }
    Ok(point)
}

fn parse_on_off(value: &str, label: &str) -> Result<bool, String> {
    match value.to_ascii_uppercase().as_str() {
        "ON" | "TRUE" | "1" => Ok(true),
        "OFF" | "FALSE" | "0" => Ok(false),
        _ => Err(format!("{label} must be ON or OFF.")),
    }
}

fn parse_frame_mode(value: &str) -> Result<u8, String> {
    let mode = value
        .parse::<u8>()
        .map_err(|_| "PDF frame mode must be 0, 1, or 2.")?;
    if mode <= 2 {
        Ok(mode)
    } else {
        Err("PDF frame mode must be 0, 1, or 2.".into())
    }
}
