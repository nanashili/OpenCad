use super::commands::{number, parse_point};
use super::*;
use crate::PlotArea;

impl OpenCadApp {
    pub(super) fn feature_plot_settings_command(
        &mut self,
        verb: &str,
        tokens: &[&str],
    ) -> Option<Result<(), String>> {
        let result = match verb.to_ascii_uppercase().as_str() {
            "PLOTAREA" => self.plot_area_command(tokens),
            "PLOTSCALE" => self.plot_scale_command(tokens),
            "PLOTOFFSET" => self.plot_offset_command(tokens),
            "PLOTCENTER" => self.plot_center_command(tokens),
            "PLOTMARGINS" => self.plot_margins_command(tokens),
            "PLOTSETTINGS" => self.plot_settings_command(tokens),
            _ => return None,
        };
        Some(result)
    }

    fn plot_area_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let area = match tokens {
            [name] if name.eq_ignore_ascii_case("EXTENTS") => PlotArea::Extents,
            [name] if name.eq_ignore_ascii_case("LAYOUT") => PlotArea::Layout,
            [window, first, second] if window.eq_ignore_ascii_case("WINDOW") => {
                let a = parse_point(first, self.drawing_elevation())?;
                let b = parse_point(second, a.z())?;
                PlotArea::Window([a.x(), a.y(), b.x(), b.y()])
            }
            [first, second] => {
                let a = parse_point(first, self.drawing_elevation())?;
                let b = parse_point(second, a.z())?;
                PlotArea::Window([a.x(), a.y(), b.x(), b.y()])
            }
            _ => return Err("PLOTAREA EXTENTS|LAYOUT|WINDOW corner1 corner2.".into()),
        };
        self.update_active_plot_setup(|setup| {
            setup.set_plot_area(area).map_err(|error| error.to_string())
        })
    }

    fn plot_scale_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        let value = tokens.first().ok_or("PLOTSCALE FIT|number|paper:draw.")?;
        if tokens.len() != 1 {
            return Err("PLOTSCALE accepts one scale value.".into());
        }
        if value.eq_ignore_ascii_case("FIT") {
            return self.update_active_plot_setup(|setup| {
                setup.set_fit_to_paper(true);
                Ok(())
            });
        }
        let scale = parse_plot_scale(value)?;
        self.update_active_plot_setup(|setup| {
            setup.set_scale(scale).map_err(|error| error.to_string())
        })
    }

    fn plot_offset_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 1 {
            return Err("PLOTOFFSET x,y.".into());
        }
        let point = parse_point(tokens[0], self.drawing_elevation())?;
        self.update_active_plot_setup(|setup| {
            setup
                .set_plot_offset([point.x(), point.y()])
                .map_err(|error| error.to_string())
        })
    }

    fn plot_center_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 1 {
            return Err("PLOTCENTER ON|OFF.".into());
        }
        let enabled = match tokens[0].to_ascii_uppercase().as_str() {
            "ON" => true,
            "OFF" => false,
            _ => return Err("PLOTCENTER must be ON or OFF.".into()),
        };
        self.update_active_plot_setup(|setup| {
            setup
                .set_centered(enabled)
                .map_err(|error| error.to_string())
        })
    }

    fn plot_margins_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() != 4 {
            return Err("PLOTMARGINS left bottom right top.".into());
        }
        let margins = tokens
            .iter()
            .map(|value| number(value))
            .collect::<Result<Vec<_>, _>>()?;
        self.update_active_plot_setup(|setup| {
            setup
                .set_margins([margins[0], margins[1], margins[2], margins[3]])
                .map_err(|error| error.to_string())
        })
    }

    fn plot_settings_command(&mut self, tokens: &[&str]) -> Result<(), String> {
        if tokens.len() > 1
            || tokens
                .first()
                .is_some_and(|token| !token.eq_ignore_ascii_case("LIST"))
        {
            return Err("PLOTSETTINGS LIST.".into());
        }
        let setup = self.layouts.active().setup();
        self.message(
            format!(
                "paper={:?} orientation={:?} area={:?} fit={} scale={} margins={:?} offset={:?} centered={}",
                setup.dimensions(),
                setup.orientation(),
                setup.plot_area(),
                setup.fit_to_paper(),
                setup.scale(),
                setup.margins(),
                setup.plot_offset(),
                setup.centered()
            ),
            false,
        );
        Ok(())
    }

    fn update_active_plot_setup(
        &mut self,
        update: impl FnOnce(&mut crate::PageSetup) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut setup = self.layouts.active().setup();
        update(&mut setup)?;
        self.layouts.active_mut().set_setup(setup);
        self.editor.set_layouts(self.layouts.clone());
        Ok(())
    }
}

fn parse_plot_scale(value: &str) -> Result<f64, String> {
    if let Some((paper, drawing)) = value.split_once(':') {
        let paper = number(paper)?;
        let drawing = number(drawing)?;
        if paper <= 0.0 || drawing <= 0.0 {
            return Err("PLOTSCALE ratio values must be positive.".into());
        }
        return Ok(paper / drawing);
    }
    let scale = number(value)?;
    if scale <= 0.0 {
        return Err("PLOTSCALE must be positive.".into());
    }
    Ok(scale)
}
