use super::commands::{number, parse_point};
use super::*;
use crate::{Face3, Mesh3};

/// Command handlers for tessellated surface features.  The solid model is a
/// mesh of faces, so these commands deliberately create honest polygonal
/// surfaces that remain editable and exportable.
impl OpenCadApp {
    pub(super) fn feature_solid_command(
        &mut self,
        verb: &str,
        _rest: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "BOX" => self.solid_box(tokens),
            "WEDGE" => self.solid_wedge(tokens),
            "PYRAMID" => self.solid_pyramid(tokens),
            "SPHERE" => self.solid_sphere(tokens),
            "CYLINDER" => self.solid_cylinder(tokens),
            "CONE" => self.solid_cone(tokens),
            "TORUS" => self.solid_torus(tokens),
            "3DFACE" => self.solid_face(tokens),
            "3DPOLY" => self.solid_polyline3(tokens),
            "EXTRUDE" => self.solid_extrude(tokens),
            "REVOLVE" => self.solid_revolve(tokens),
            "ROTATE3D" => self.solid_rotate3d(tokens),
            "SWEEP" => self.solid_sweep(tokens),
            "LOFT" => self.solid_loft(tokens),
            "SLICE" => self.solid_slice(tokens),
            "SECTION" => self.solid_section(tokens),
            "HELIX" => self.solid_helix(tokens),
            "3DARRAY" => self.solid_array3d(tokens),
            "MIRROR3D" => self.solid_mirror3d(tokens),
            "SCALE3D" => self.solid_scale3d(tokens),
            "MESHREFINE" => self.mesh_refine(tokens),
            "MESHTRIANGULATE" => self.mesh_triangulate(tokens),
            "MESHINFO" => self.mesh_info(tokens),
            "MESHEDGES" => self.mesh_edges(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn add_mesh(&mut self, mesh: Mesh3) -> Result<(), String> {
        self.add_entities(vec![Entity::Mesh(mesh)])
    }
    fn parse_solid_point(&self, value: &str) -> Result<crate::Point3, String> {
        parse_point(value, self.drawing_elevation())
    }
    fn dimensions(tokens: &[&str], label: &str) -> Result<(crate::Point3, Vec<f64>), String> {
        if tokens.len() < 2 {
            return Err(format!(
                "{label} syntax requires an insertion point and dimensions."
            ));
        }
        let p = parse_point(tokens[0], 0.0)?;
        let values = tokens[1..]
            .iter()
            .map(|v| number(v))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((p, values))
    }
    fn solid_box(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 2 {
            return Err("BOX syntax: BOX minimum maximum.".into());
        }
        self.add_mesh(
            Mesh3::box_mesh(
                self.parse_solid_point(tokens[0])?,
                self.parse_solid_point(tokens[1])?,
            )
            .map_err(|e| e.to_string())?,
        )
    }
    fn solid_wedge(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (p, v) = Self::dimensions(tokens, "WEDGE")?;
        if v.len() != 3 {
            return Err("WEDGE requires width depth height.".into());
        };
        self.add_mesh(
            Mesh3::wedge(v[0], v[1], v[2])
                .map_err(|e| e.to_string())?
                .translated(p.x(), p.y(), p.z())
                .map_err(|e| e.to_string())?,
        )
    }
    fn solid_pyramid(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (p, v) = Self::dimensions(tokens, "PYRAMID")?;
        if v.len() != 3 {
            return Err("PYRAMID requires width depth height.".into());
        };
        self.add_mesh(
            Mesh3::pyramid(v[0], v[1], v[2])
                .map_err(|e| e.to_string())?
                .translated(p.x(), p.y(), p.z())
                .map_err(|e| e.to_string())?,
        )
    }
    fn solid_sphere(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (p, v) = Self::dimensions(tokens, "SPHERE")?;
        if v.len() != 1 {
            return Err("SPHERE requires radius.".into());
        };
        self.add_mesh(Mesh3::sphere(p, v[0], 32, 16).map_err(|e| e.to_string())?)
    }
    fn solid_cylinder(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (p, v) = Self::dimensions(tokens, "CYLINDER")?;
        if v.len() != 2 {
            return Err("CYLINDER requires radius height.".into());
        };
        self.add_mesh(Mesh3::cylinder(p, v[0], v[1], 32).map_err(|e| e.to_string())?)
    }
    fn solid_cone(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (p, v) = Self::dimensions(tokens, "CONE")?;
        if v.len() != 2 {
            return Err("CONE requires radius height.".into());
        };
        self.add_mesh(Mesh3::cone(p, v[0], v[1], 32).map_err(|e| e.to_string())?)
    }
    fn solid_torus(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (p, v) = Self::dimensions(tokens, "TORUS")?;
        if v.len() != 2 {
            return Err("TORUS requires major-radius minor-radius.".into());
        };
        self.add_mesh(Mesh3::torus(p, v[0], v[1], 32, 16).map_err(|e| e.to_string())?)
    }
    fn solid_face(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !(3..=4).contains(&tokens.len()) {
            return Err("3DFACE syntax: three or four points.".into());
        }
        let p = tokens
            .iter()
            .map(|v| self.parse_solid_point(v))
            .collect::<Result<Vec<_>, _>>()?;
        let face = if p.len() == 3 {
            Face3::triangle(p[0], p[1], p[2])
        } else {
            Face3::quad(p[0], p[1], p[2], p[3])
        }
        .map_err(|e| e.to_string())?;
        self.add_entities(vec![Entity::Face3(face)])
    }
    fn solid_extrude(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 4 {
            return Err("EXTRUDE syntax: point point point [point ...] height.".into());
        };
        let height = number(tokens[tokens.len() - 1])?;
        let points = tokens[..tokens.len() - 1]
            .iter()
            .map(|v| self.parse_solid_point(v))
            .collect::<Result<Vec<_>, _>>()?;
        self.add_mesh(Mesh3::extrude_polygon(&points, height).map_err(|e| e.to_string())?)
    }
    fn solid_revolve(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() < 5 {
            return Err("REVOLVE syntax: profile-points axis-origin axis-vector angle.".into());
        };
        let n = tokens.len();
        let angle = number(tokens[n - 1])?.to_radians();
        let origin = self.parse_solid_point(tokens[n - 3])?;
        let axis = self.parse_solid_point(tokens[n - 2])?;
        let points = tokens[..n - 3]
            .iter()
            .map(|v| self.parse_solid_point(v))
            .collect::<Result<Vec<_>, _>>()?;
        self.add_mesh(
            Mesh3::revolve_profile_angle(&points, origin, axis, 32, angle)
                .map_err(|e| e.to_string())?,
        )
    }
    fn solid_rotate3d(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 4 {
            return Err("ROTATE3D syntax: entity-number axis-origin axis-vector angle.".into());
        };
        let index = tokens[0]
            .parse::<usize>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or("Entity number must be positive.")?
            - 1;
        let origin = self.parse_solid_point(tokens[1])?;
        let axis = self.parse_solid_point(tokens[2])?;
        let angle = number(tokens[3])?.to_radians();
        let entity = self
            .editor
            .entities()
            .get(index)
            .ok_or("Entity number is outside this drawing.")?;
        if !matches!(
            entity,
            Entity::Face3(_) | Entity::Line(_) | Entity::Point(_)
        ) {
            return Err("ROTATE3D supports 3DFACE, LINE, and POINT entities.".into());
        }
        self.editor.select(vec![index]);
        self.editor
            .map_selection_with(false, move |entity| match entity {
                Entity::Face3(face) => Ok(Entity::Face3(face.transformed(origin, axis, angle)?)),
                Entity::Line(line) => Ok(Entity::Line(Line3::new(
                    crate::solid::rotate_point(line.start(), origin, axis, angle)?,
                    crate::solid::rotate_point(line.end(), origin, axis, angle)?,
                ))),
                Entity::Point(point) => Ok(Entity::Point(crate::PointEntity::new(
                    crate::solid::rotate_point(point.position(), origin, axis, angle)?,
                ))),
                _ => Err(crate::GeometryError::UnsupportedPolylineConversion),
            })
            .map_err(|e| e.to_string())
            .map(|_| ())
    }

    fn point_groups(
        &self,
        tokens: &[&str],
        marker: &str,
    ) -> Result<Vec<Vec<crate::Point3>>, String> {
        let mut groups = vec![Vec::new()];
        for token in tokens {
            if token.eq_ignore_ascii_case(marker) {
                groups.push(Vec::new());
            } else {
                groups
                    .last_mut()
                    .unwrap()
                    .push(self.parse_solid_point(token)?);
            }
        }
        if groups.iter().any(|group| group.len() < 2) {
            return Err("Each point group needs at least two points.".into());
        }
        Ok(groups)
    }

    fn solid_sweep(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (tokens, closed, cap) = solid_surface_options(tokens);
        let groups = self.point_groups(&tokens, "|")?;
        if groups.len() != 2 {
            return Err("SWEEP syntax: profile points | path points.".into());
        }
        let mesh =
            crate::sweep_profile(&groups[0], &groups[1], closed, cap).map_err(|e| e.to_string())?;
        self.add_mesh(mesh)
    }

    fn solid_loft(&mut self, tokens: &[&str]) -> Result<(), String> {
        let (tokens, closed, cap) = solid_surface_options(tokens);
        let profiles = self.point_groups(&tokens, "|")?;
        if profiles.len() < 2 {
            return Err("LOFT syntax: profile points | profile points [...].".into());
        }
        self.add_mesh(crate::loft_profiles(&profiles, closed, cap).map_err(|e| e.to_string())?)
    }

    fn mesh_index(&self, token: &str) -> Result<usize, String> {
        token
            .parse::<usize>()
            .ok()
            .and_then(|value| value.checked_sub(1))
            .filter(|index| matches!(self.editor.entities().get(*index), Some(Entity::Mesh(_))))
            .ok_or_else(|| "Entity number must refer to a MESH.".into())
    }

    fn solid_slice(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !(3..=5).contains(&tokens.len()) {
            return Err("SLICE syntax: mesh origin normal [POSITIVE|NEGATIVE|BOTH].".into());
        }
        let index = self.mesh_index(tokens[0])?;
        let origin = self.parse_solid_point(tokens[1])?;
        let normal = self.parse_solid_point(tokens[2])?;
        let side = tokens
            .get(3)
            .map(|value| value.to_ascii_uppercase())
            .unwrap_or_else(|| "BOTH".into());
        if !matches!(side.as_str(), "POSITIVE" | "NEGATIVE" | "BOTH") {
            return Err("SLICE side must be POSITIVE, NEGATIVE, or BOTH.".into());
        }
        let cap = match tokens.get(4) {
            None => true,
            Some(value) if value.eq_ignore_ascii_case("SURFACE") => false,
            Some(_) => return Err("SLICE option must be SURFACE.".into()),
        };
        let mesh = match self.editor.entities().get(index) {
            Some(Entity::Mesh(mesh)) => mesh.clone(),
            _ => unreachable!(),
        };
        let (positive, negative) =
            crate::slice_mesh(&mesh, origin, normal, cap).map_err(|e| e.to_string())?;
        let replacement: Vec<Mesh3> = match side.as_str() {
            "POSITIVE" => positive.into_iter().collect(),
            "NEGATIVE" => negative.into_iter().collect(),
            _ => [positive, negative].into_iter().flatten().collect(),
        };
        self.editor.select(vec![index]);
        self.editor
            .replace_selection_with_entities(replacement.into_iter().map(Entity::Mesh).collect())
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn solid_section(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 3 {
            return Err("SECTION syntax: mesh origin normal.".into());
        }
        let index = self.mesh_index(tokens[0])?;
        let origin = self.parse_solid_point(tokens[1])?;
        let normal = self.parse_solid_point(tokens[2])?;
        let mesh = match self.editor.entities().get(index) {
            Some(Entity::Mesh(mesh)) => mesh.clone(),
            _ => unreachable!(),
        };
        let loops = crate::section_mesh(&mesh, origin, normal).map_err(|e| e.to_string())?;
        let mut entities = vec![Entity::Mesh(mesh)];
        for mut loop_points in loops {
            let closed = loop_points.len() > 1 && loop_points.first() == loop_points.last();
            if closed {
                loop_points.pop();
            }
            let path = crate::Polyline3::new(loop_points, closed).map_err(|e| e.to_string())?;
            entities.push(Entity::Polyline3(path));
        }
        if entities.len() == 1 {
            return Err("SECTION produced no valid intersection paths.".into());
        }
        self.editor.select(vec![index]);
        self.editor
            .replace_selection_with_entities(entities)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn solid_helix(&mut self, tokens: &[&str]) -> Result<(), String> {
        if !(5..=6).contains(&tokens.len()) {
            return Err(
                "HELIX syntax: center base-radius top-radius height turns [segments].".into(),
            );
        }
        let center = self.parse_solid_point(tokens[0])?;
        let radius = number(tokens[1])?;
        let top_radius = number(tokens[2])?;
        let height = number(tokens[3])?;
        let turns = number(tokens[4])?;
        let segments = tokens.get(5).map_or(Ok(128), |value| {
            value
                .parse::<usize>()
                .map_err(|_| "Helix segments must be an integer.".to_string())
        })?;
        let points = crate::helix_points(center, radius, top_radius, height, turns, segments)
            .map_err(|e| e.to_string())?;
        let polyline = crate::Polyline3::new(points, false).map_err(|e| e.to_string())?;
        self.add_entities(vec![Entity::Polyline3(polyline)])
    }

    fn solid_polyline3(&mut self, tokens: &[&str]) -> Result<(), String> {
        let closed = tokens
            .last()
            .is_some_and(|value| value.eq_ignore_ascii_case("CLOSE"));
        let end = tokens.len() - usize::from(closed);
        if end < 2 {
            return Err("3DPOLY syntax: point point [... ] [CLOSE].".into());
        }
        let points = tokens[..end]
            .iter()
            .map(|token| self.parse_solid_point(token))
            .collect::<Result<Vec<_>, _>>()?;
        let polyline = crate::Polyline3::new(points, closed).map_err(|e| e.to_string())?;
        self.add_entities(vec![Entity::Polyline3(polyline)])
    }

    fn selected_mesh(&self, token: &str) -> Result<(usize, Mesh3), String> {
        let index = self.mesh_index(token)?;
        let mesh = match self.editor.entities().get(index) {
            Some(Entity::Mesh(mesh)) => mesh.clone(),
            _ => unreachable!(),
        };
        Ok((index, mesh))
    }

    fn solid_array3d(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 5 {
            return Err("3DARRAY syntax: mesh count dx dy dz.".into());
        }
        let (_, mesh) = self.selected_mesh(tokens[0])?;
        let count = tokens[1]
            .parse::<usize>()
            .map_err(|_| "Array count must be an integer.")?;
        if !(1..=1024).contains(&count) {
            return Err("Array count must be between 1 and 1024.".into());
        }
        let dx = number(tokens[2])?;
        let dy = number(tokens[3])?;
        let dz = number(tokens[4])?;
        let copies = (1..count)
            .map(|i| {
                mesh.translated(dx * i as f64, dy * i as f64, dz * i as f64)
                    .map(Entity::Mesh)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        self.add_entities(copies)
    }

    fn solid_mirror3d(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 3 {
            return Err("MIRROR3D syntax: mesh origin axis.".into());
        }
        let (index, mesh) = self.selected_mesh(tokens[0])?;
        let origin = self.parse_solid_point(tokens[1])?;
        let axis = self.parse_solid_point(tokens[2])?;
        let reflected = mesh.mirrored(origin, axis).map_err(|e| e.to_string())?;
        self.editor.select(vec![index]);
        self.editor
            .replace_selection_with_entities(vec![Entity::Mesh(reflected)])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn mesh_refine(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 2 {
            return Err("MESHREFINE syntax: mesh levels.".into());
        }
        let (index, mesh) = self.selected_mesh(tokens[0])?;
        let levels = tokens[1]
            .parse::<usize>()
            .map_err(|_| "Subdivision levels must be an integer.")?;
        let mesh = crate::subdivide_mesh(&mesh, levels).map_err(|e| e.to_string())?;
        self.editor.select(vec![index]);
        self.editor
            .replace_selection_with_entities(vec![Entity::Mesh(mesh)])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn solid_scale3d(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 4 {
            return Err("SCALE3D syntax: mesh sx sy sz.".into());
        }
        let (index, mesh) = self.selected_mesh(tokens[0])?;
        let scale = [number(tokens[1])?, number(tokens[2])?, number(tokens[3])?];
        let scaled = crate::scale_mesh(&mesh, scale).map_err(|e| e.to_string())?;
        self.editor.select(vec![index]);
        self.editor
            .replace_selection_with_entities(vec![Entity::Mesh(scaled)])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn mesh_triangulate(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 1 {
            return Err("MESHTRIANGULATE syntax: mesh.".into());
        }
        let (index, mesh) = self.selected_mesh(tokens[0])?;
        let mesh = crate::triangulate_mesh(&mesh).map_err(|e| e.to_string())?;
        self.editor.select(vec![index]);
        self.editor
            .replace_selection_with_entities(vec![Entity::Mesh(mesh)])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn mesh_info(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 1 {
            return Err("MESHINFO syntax: mesh.".into());
        }
        let (_, mesh) = self.selected_mesh(tokens[0])?;
        let area = crate::mesh_area(&mesh).map_err(|e| e.to_string())?;
        let volume = crate::mesh_volume(&mesh).map_err(|e| e.to_string())?;
        self.message(
            format!(
                "Mesh: {} faces, area {area:.4}, volume {volume:.4}.",
                mesh.faces().len()
            ),
            false,
        );
        Ok(())
    }

    fn mesh_edges(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 1 {
            return Err("MESHEDGES syntax: mesh.".into());
        }
        let (_, mesh) = self.selected_mesh(tokens[0])?;
        let edges = crate::boundary_edges(&mesh).map_err(|e| e.to_string())?;
        let entities = edges
            .into_iter()
            .map(|(a, b)| Entity::Line(Line3::new(a, b)))
            .collect();
        self.add_entities(entities)
    }
}

fn solid_surface_options<'a>(tokens: &'a [&'a str]) -> (Vec<&'a str>, bool, bool) {
    let mut end = tokens.len();
    let mut closed = true;
    let mut cap = true;
    while end > 0 {
        match tokens[end - 1].to_ascii_uppercase().as_str() {
            "OPEN" => {
                closed = false;
                end -= 1;
            }
            "CLOSED" => {
                closed = true;
                end -= 1;
            }
            "SURFACE" => {
                cap = false;
                end -= 1;
            }
            _ => break,
        }
    }
    (tokens[..end].to_vec(), closed, cap)
}
