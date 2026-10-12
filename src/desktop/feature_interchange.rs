use super::OpenCadApp;
use crate::{Entity, Mesh3, read_obj, read_stl, write_obj, write_stl_ascii, write_stl_binary};
use std::fs;
use std::path::{Path, PathBuf};

impl OpenCadApp {
    pub(super) fn feature_interchange_command(
        &mut self,
        verb: &str,
        rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb {
            "IMPORT" | "IMPORTOBJ" | "IMPORTSTL" => self.import_mesh_command(verb, rest, tokens),
            "EXPORT" | "EXPORTOBJ" | "EXPORTSTL" => self.export_mesh_command(verb, rest, tokens),
            "SCRIPT" | "SCRIPTCALL" => self.script_command(rest, tokens),
            _ => return None,
        };
        Some(result)
    }

    fn import_mesh_command(
        &mut self,
        verb: &str,
        rest: &str,
        tokens: &[&str],
    ) -> Result<(), String> {
        let (path, explicit_format) = if verb == "IMPORT"
            && tokens
                .first()
                .is_some_and(|t| t.eq_ignore_ascii_case("OBJ") || t.eq_ignore_ascii_case("STL"))
        {
            (
                tokens.get(1).copied().unwrap_or(""),
                tokens.first().copied(),
            )
        } else {
            (
                tokens
                    .first()
                    .copied()
                    .filter(|p| !p.is_empty())
                    .unwrap_or(rest.trim()),
                None,
            )
        };
        if path.is_empty() {
            return Err("Usage: IMPORT [OBJ|STL] path, or IMPORTOBJ/IMPORTSTL path".into());
        }
        let format = if let Some(explicit) = explicit_format {
            explicit
        } else if verb == "IMPORT" {
            tokens.get(1).copied().unwrap_or_else(|| {
                Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
            })
        } else {
            verb.trim_start_matches("IMPORT")
        };
        let bytes = fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
        let meshes = match format.to_ascii_uppercase().as_str() {
            "OBJ" => {
                let input =
                    String::from_utf8(bytes).map_err(|_| "OBJ is not valid UTF-8".to_owned())?;
                read_obj(&input).map_err(|e| format!("OBJ import failed: {e}"))?
            }
            "STL" => vec![read_stl(&bytes).map_err(|e| format!("STL import failed: {e}"))?],
            other => return Err(format!("Unsupported mesh format: {other}")),
        };
        let count = meshes.len();
        self.add_entities(meshes.into_iter().map(Entity::Mesh).collect())?;
        self.message(format!("Imported {count} mesh object(s)."), false);
        Ok(())
    }

    fn export_mesh_command(
        &mut self,
        verb: &str,
        rest: &str,
        tokens: &[&str],
    ) -> Result<(), String> {
        let (path, format) = if verb == "EXPORT"
            && tokens
                .first()
                .is_some_and(|t| t.eq_ignore_ascii_case("OBJ") || t.eq_ignore_ascii_case("STL"))
        {
            (
                tokens.get(1).copied().unwrap_or(""),
                tokens.first().copied().unwrap_or(""),
            )
        } else {
            (
                tokens
                    .first()
                    .copied()
                    .filter(|p| !p.is_empty())
                    .unwrap_or(rest.trim()),
                verb.trim_start_matches("EXPORT"),
            )
        };
        if path.is_empty() {
            return Err(format!("Usage: {verb} path"));
        }
        let meshes = self
            .editor
            .entities()
            .iter()
            .filter_map(|entity| match entity {
                Entity::Mesh(mesh) => Some(mesh.clone()),
                _ => None,
            })
            .collect::<Vec<Mesh3>>();
        if meshes.is_empty() {
            return Err("No mesh entities to export.".into());
        }
        match format.to_ascii_uppercase().as_str() {
            "OBJ" => fs::write(path, write_obj(&meshes).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Cannot write {path}: {e}"))?,
            "STL" => {
                let binary = Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("stlb"));
                if binary {
                    fs::write(path, write_stl_binary(&meshes).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("Cannot write {path}: {e}"))?;
                } else {
                    fs::write(path, write_stl_ascii(&meshes).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("Cannot write {path}: {e}"))?;
                }
            }
            _ => return Err(format!("Unsupported mesh format: {format}")),
        }
        self.message(format!("Exported {} mesh object(s).", meshes.len()), false);
        Ok(())
    }

    fn script_command(&mut self, rest: &str, tokens: &[&str]) -> Result<(), String> {
        let path = tokens
            .first()
            .copied()
            .filter(|p| !p.is_empty())
            .unwrap_or(rest.trim());
        if path.is_empty() {
            return Err("Usage: SCRIPT path".into());
        }
        let path_buf = PathBuf::from(path);
        let input = fs::read_to_string(&path_buf)
            .map_err(|e| format!("Cannot read script {}: {e}", path_buf.display()))?;
        let base = path_buf
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let runner = crate::script::ScriptRunner::default();
        let count = runner
            .run(
                &input,
                path_buf.to_string_lossy().into_owned(),
                |including, include| {
                    let parent = Path::new(including)
                        .parent()
                        .filter(|path| !path.as_os_str().is_empty())
                        .unwrap_or(&base);
                    let candidate = parent.join(include);
                    fs::read_to_string(&candidate).map_err(|e| {
                        format!("Cannot read included script {}: {e}", candidate.display())
                    })
                },
                |command| {
                    if command.raw.is_empty() {
                        return Ok(());
                    }
                    self.command_result(&command.raw)
                },
            )
            .map_err(|e| e.to_string())?;
        self.message(format!("Executed {count} script command(s)."), false);
        Ok(())
    }
}
