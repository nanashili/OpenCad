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
            "EXTRUDE" => self.solid_extrude(tokens),
            "REVOLVE" => self.solid_revolve(tokens),
            "ROTATE3D" => self.solid_rotate3d(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn add_mesh(&mut self, mesh: Mesh3) -> Result<(), String> {
        self.add_entities(mesh.into_faces().into_iter().map(Entity::Face3).collect())
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
}
