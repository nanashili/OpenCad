//! Bounded PDF inspection and page previews.
//!
//! This module is feature-gated by `desktop` because Hayro is an optional renderer dependency.
//! Preview pixels are returned as straight (unpremultiplied) RGBA8.

#[cfg(feature = "desktop")]
const MAX_INPUT_BYTES: usize = 128 * 1024 * 1024;
#[cfg(feature = "desktop")]
const MAX_PAGES: usize = 10_000;
#[cfg(feature = "desktop")]
const MAX_EDGE: u32 = 2048;
#[cfg(feature = "desktop")]
const MAX_PIXELS: u64 = 4_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfRenderError {
    InputTooLarge,
    RendererUnavailable,
    InvalidPdf,
    PageOutOfRange,
    TooManyPages,
    InvalidDimensions,
    InvalidDisplaySettings,
    RenderFailed,
}
impl std::fmt::Display for PdfRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PdfRenderError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfPageInfo {
    pub width_points: f32,
    pub height_points: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PdfInfo {
    pub page_count: usize,
    pub pages: Vec<PdfPageInfo>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaPreview {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn adjust_preview(
    preview: &mut RgbaPreview,
    fade: f64,
    contrast: f64,
    monochrome: bool,
) -> Result<(), PdfRenderError> {
    if !fade.is_finite()
        || !contrast.is_finite()
        || !(0.0..=100.0).contains(&fade)
        || !(0.0..=100.0).contains(&contrast)
    {
        return Err(PdfRenderError::InvalidDisplaySettings);
    }
    if preview.pixels.len()
        != usize::try_from(preview.width)
            .ok()
            .and_then(|width| width.checked_mul(preview.height as usize))
            .and_then(|pixels| pixels.checked_mul(4))
            .unwrap_or(0)
    {
        return Err(PdfRenderError::RenderFailed);
    }
    let factor = contrast / 100.0;
    for pixel in preview.pixels.chunks_exact_mut(4) {
        let mut channels = [pixel[0] as f64, pixel[1] as f64, pixel[2] as f64];
        if monochrome {
            let gray = 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
            channels = [gray, gray, gray];
        }
        for channel in &mut channels {
            *channel = (((*channel - 127.5) * factor) + 127.5) * (1.0 - fade / 100.0)
                + 255.0 * fade / 100.0;
        }
        pixel[0] = channels[0].round().clamp(0.0, 255.0) as u8;
        pixel[1] = channels[1].round().clamp(0.0, 255.0) as u8;
        pixel[2] = channels[2].round().clamp(0.0, 255.0) as u8;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
fn open_pdf(data: &[u8]) -> Result<hayro::hayro_syntax::Pdf, PdfRenderError> {
    if data.len() > MAX_INPUT_BYTES {
        return Err(PdfRenderError::InputTooLarge);
    }
    hayro::hayro_syntax::Pdf::new(data.to_vec()).map_err(|_| PdfRenderError::InvalidPdf)
}

#[cfg(feature = "desktop")]
pub fn pdf_info(data: &[u8]) -> Result<PdfInfo, PdfRenderError> {
    let pdf = open_pdf(data)?;
    let pages = pdf.pages();
    if pages.len() > MAX_PAGES {
        return Err(PdfRenderError::TooManyPages);
    }
    let mut out = Vec::with_capacity(pages.len());
    for page in pages.iter() {
        let (w, h) = page.render_dimensions();
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return Err(PdfRenderError::InvalidDimensions);
        }
        out.push(PdfPageInfo {
            width_points: w,
            height_points: h,
        });
    }
    Ok(PdfInfo {
        page_count: out.len(),
        pages: out,
    })
}

#[cfg(feature = "desktop")]
pub fn render_pdf_page(data: &[u8], page_number: usize) -> Result<RgbaPreview, PdfRenderError> {
    let pdf = open_pdf(data)?;
    let pages = pdf.pages();
    if pages.len() > MAX_PAGES {
        return Err(PdfRenderError::TooManyPages);
    }
    let page = pages
        .get(
            page_number
                .checked_sub(1)
                .ok_or(PdfRenderError::PageOutOfRange)?,
        )
        .ok_or(PdfRenderError::PageOutOfRange)?;
    let (w, h) = page.render_dimensions();
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(PdfRenderError::InvalidDimensions);
    }
    let mut scale = (MAX_EDGE as f32 / w.max(h)).min(1.0);
    let pixels = f64::from(w) * f64::from(h) * f64::from(scale) * f64::from(scale);
    if pixels > MAX_PIXELS as f64 {
        scale *= (MAX_PIXELS as f32 / pixels as f32).sqrt();
    }
    let requested_width = (w * scale).clamp(1.0, MAX_EDGE as f32) as u32;
    let requested_height = (h * scale).clamp(1.0, MAX_EDGE as f32) as u32;
    if u64::from(requested_width) * u64::from(requested_height) > MAX_PIXELS {
        return Err(PdfRenderError::InvalidDimensions);
    }
    let cache = hayro::RenderCache::new();
    let pixmap = hayro::render(
        page,
        &cache,
        &Default::default(),
        &Default::default(),
        &hayro::PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            ..Default::default()
        },
    );
    let width = u32::from(pixmap.width());
    let height = u32::from(pixmap.height());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(PdfRenderError::InvalidDimensions);
    }
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for pixel in pixmap.data() {
        let a = u16::from(pixel.a);
        let (r, g, b) = if a == 0 {
            (0, 0, 0)
        } else {
            (
                (u16::from(pixel.r) * 255 / a).min(255) as u8,
                (u16::from(pixel.g) * 255 / a).min(255) as u8,
                (u16::from(pixel.b) * 255 / a).min(255) as u8,
            )
        };
        out.extend_from_slice(&[r, g, b, pixel.a]);
    }
    if out.len() != width as usize * height as usize * 4 {
        return Err(PdfRenderError::RenderFailed);
    }
    Ok(RgbaPreview {
        width,
        height,
        pixels: out,
    })
}

#[cfg(not(feature = "desktop"))]
pub fn pdf_info(_data: &[u8]) -> Result<PdfInfo, PdfRenderError> {
    Err(PdfRenderError::RendererUnavailable)
}
#[cfg(not(feature = "desktop"))]
pub fn render_pdf_page(_data: &[u8], _page_number: usize) -> Result<RgbaPreview, PdfRenderError> {
    Err(PdfRenderError::RendererUnavailable)
}
