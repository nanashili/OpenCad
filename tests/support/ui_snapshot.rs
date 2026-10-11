//! Optional software previews of the actual egui paint output, without a window.
//! These previews exclude AppKit's title bar and native menus.

use eframe::egui::{self, ColorImage, TextureId, epaint};
use std::{collections::HashMap, path::PathBuf};

pub struct Snapshot {
    directory: PathBuf,
    textures: HashMap<TextureId, ColorImage>,
    pixels_per_point: f32,
}

impl Snapshot {
    pub fn from_environment() -> Option<Self> {
        std::env::var_os("OPENCAD_UI_SNAPSHOT_DIR").map(|path| Self {
            directory: path.into(),
            textures: HashMap::new(),
            pixels_per_point: 1.0,
        })
    }

    pub fn update(&mut self, output: &egui::FullOutput) {
        self.pixels_per_point = output.pixels_per_point;
        for (id, delta) in &output.textures_delta.set {
            let epaint::ImageData::Color(image) = &delta.image;
            if let Some([x, y]) = delta.pos {
                let target = self.textures.get_mut(id).expect("existing texture patch");
                for row in 0..image.size[1] {
                    let start = (y + row) * target.size[0] + x;
                    target.pixels[start..start + image.size[0]].copy_from_slice(
                        &image.pixels[row * image.size[0]..(row + 1) * image.size[0]],
                    );
                }
            } else {
                self.textures.insert(*id, (**image).clone());
            }
        }
    }

    pub fn save(
        &self,
        ctx: &egui::Context,
        shapes: &[epaint::ClippedShape],
        size: egui::Vec2,
        name: &str,
    ) {
        let scale = self.pixels_per_point;
        let [width, height] = [
            (size.x * scale).round() as u32,
            (size.y * scale).round() as u32,
        ];
        let mut image = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 255]));
        for primitive in ctx.tessellate(shapes.to_vec(), scale) {
            let epaint::Primitive::Mesh(mesh) = primitive.primitive else {
                panic!("UI preview does not support custom paint callbacks");
            };
            let texture = &self.textures[&mesh.texture_id];
            let clip = primitive.clip_rect * scale;
            for indices in mesh.indices.chunks_exact(3) {
                let mut vertices =
                    [indices[0], indices[1], indices[2]].map(|index| mesh.vertices[index as usize]);
                for vertex in &mut vertices {
                    vertex.pos *= scale;
                }
                let area = edge(vertices[0].pos, vertices[1].pos, vertices[2].pos);
                if area.abs() < f32::EPSILON {
                    continue;
                }
                if area < 0.0 {
                    vertices.swap(1, 2);
                }
                let [a, b, c] = vertices;
                let area = area.abs();
                let x0 = a
                    .pos
                    .x
                    .min(b.pos.x)
                    .min(c.pos.x)
                    .max(clip.min.x)
                    .max(0.0)
                    .floor() as u32;
                let y0 = a
                    .pos
                    .y
                    .min(b.pos.y)
                    .min(c.pos.y)
                    .max(clip.min.y)
                    .max(0.0)
                    .floor() as u32;
                let x1 = a
                    .pos
                    .x
                    .max(b.pos.x)
                    .max(c.pos.x)
                    .min(clip.max.x)
                    .min(width as f32)
                    .ceil() as u32;
                let y1 = a
                    .pos
                    .y
                    .max(b.pos.y)
                    .max(c.pos.y)
                    .min(clip.max.y)
                    .min(height as f32)
                    .ceil() as u32;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                        if !clip.contains(p) {
                            continue;
                        }
                        let edges = [(b.pos, c.pos), (c.pos, a.pos), (a.pos, b.pos)];
                        let weights = edges.map(|(start, end)| edge(start, end, p));
                        if weights.iter().zip(edges).any(|(&weight, (start, end))| {
                            weight < 0.0 || (weight == 0.0 && !top_left(start, end))
                        }) {
                            continue;
                        }
                        let weights = weights.map(|weight| weight / area);
                        let uv = a.uv.to_vec2() * weights[0]
                            + b.uv.to_vec2() * weights[1]
                            + c.uv.to_vec2() * weights[2];
                        let texel = sample(texture, uv);
                        let colors = vertices.map(|vertex| vertex.color.to_array());
                        let source: [f32; 4] = std::array::from_fn(|channel| {
                            texel[channel] / 255.0
                                * (0..3)
                                    .map(|i| f32::from(colors[i][channel]) * weights[i])
                                    .sum::<f32>()
                        });
                        let target = image.get_pixel_mut(x, y);
                        for channel in 0..3 {
                            target[channel] = (source[channel]
                                + f32::from(target[channel]) * (1.0 - source[3] / 255.0))
                                .round()
                                .clamp(0.0, 255.0)
                                as u8;
                        }
                    }
                }
            }
        }
        std::fs::create_dir_all(&self.directory).expect("preview directory");
        image
            .save(self.directory.join(format!("{name}.png")))
            .expect("PNG preview");
    }
}

fn edge(a: egui::Pos2, b: egui::Pos2, p: egui::Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn top_left(a: egui::Pos2, b: egui::Pos2) -> bool {
    b.y > a.y || (b.y == a.y && b.x < a.x)
}

fn sample(image: &ColorImage, uv: egui::Vec2) -> [f32; 4] {
    let x = (uv.x * image.size[0] as f32 - 0.5).clamp(0.0, (image.size[0] - 1) as f32);
    let y = (uv.y * image.size[1] as f32 - 0.5).clamp(0.0, (image.size[1] - 1) as f32);
    let [x0, y0] = [x.floor() as usize, y.floor() as usize];
    let [x1, y1] = [
        (x0 + 1).min(image.size[0] - 1),
        (y0 + 1).min(image.size[1] - 1),
    ];
    let [dx, dy] = [x.fract(), y.fract()];
    let pixels = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
        .map(|(x, y)| image.pixels[y * image.size[0] + x].to_array());
    std::array::from_fn(|channel| {
        f32::from(pixels[0][channel]) * (1.0 - dx) * (1.0 - dy)
            + f32::from(pixels[1][channel]) * dx * (1.0 - dy)
            + f32::from(pixels[2][channel]) * (1.0 - dx) * dy
            + f32::from(pixels[3][channel]) * dx * dy
    })
}
