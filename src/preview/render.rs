//! Page rasterization: PDF bytes in, PNG bytes (or page geometry) out.
//!
//! Pure and synchronous on purpose — no filesystem, no async, no config — so
//! it unit-tests against `printpdf` fixtures without a tempdir or a runtime,
//! and every side effect lives in `mod.rs`.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, RenderCache, RenderSettings};

/// Raster width cap. The picker shows pages at ~250 CSS px, so this covers a
/// 2× display with headroom and keeps a cached page well under 100 KB. Pages
/// narrower than this are never upscaled — a preview is a glance, and the
/// stored bytes are what a rebuild would have to redo.
const PREVIEW_WIDTH_PX: f32 = 640.0;

/// A document's page count plus the first page's rendered size. The size is
/// what lets the frontend lay out correctly-shaped placeholders before any
/// image arrives, so the preview pane doesn't reflow as pages load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PageMeta {
    pub(crate) pages: u32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RenderError {
    /// hayro could not read the document: encrypted, truncated, or not a PDF.
    /// The frontend's cue to show the text card instead of an image.
    Unrenderable,
    /// The document has fewer pages than the requested index.
    PageOutOfRange,
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::Unrenderable => f.write_str("the PDF could not be rendered"),
            RenderError::PageOutOfRange => f.write_str("page out of range"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Page count and first-page geometry, without rasterizing anything.
pub(crate) fn page_meta(bytes: Vec<u8>) -> Result<PageMeta, RenderError> {
    let pdf = Pdf::new(bytes).map_err(|_| RenderError::Unrenderable)?;
    let pages = pdf.pages();
    // A zero-page document is well-formed PDF but nothing a preview can show.
    let first = pages.first().ok_or(RenderError::Unrenderable)?;
    let (width, height) = first.render_dimensions();
    Ok(PageMeta {
        pages: pages.len() as u32,
        width,
        height,
    })
}

/// Rasterize one page to PNG bytes.
pub(crate) fn page_png(bytes: Vec<u8>, page: u32) -> Result<Vec<u8>, RenderError> {
    let pdf = Pdf::new(bytes).map_err(|_| RenderError::Unrenderable)?;
    let pages = pdf.pages();
    let page = pages
        .get(page as usize)
        .ok_or(RenderError::PageOutOfRange)?;
    let (width, _) = page.render_dimensions();
    let scale = if width > 0.0 {
        (PREVIEW_WIDTH_PX / width).min(1.0)
    } else {
        1.0
    };
    // hayro's own default is a transparent ground, which would let the
    // reader's dark theme show through the paper.
    let settings = RenderSettings {
        x_scale: scale,
        y_scale: scale,
        bg_color: WHITE,
        ..Default::default()
    };
    let pixmap = render(
        page,
        &RenderCache::new(),
        &InterpreterSettings::default(),
        &settings,
    );
    pixmap.into_png().map_err(|_| RenderError::Unrenderable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_bytes(pages: usize) -> Vec<u8> {
        use printpdf::*;
        let page = |n: usize| {
            PdfPage::new(
                Mm(210.0),
                Mm(297.0),
                vec![
                    Op::StartTextSection,
                    Op::SetFont {
                        font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                        size: Pt(12.0),
                    },
                    Op::SetTextCursor {
                        pos: Point::new(Mm(15.0), Mm(280.0)),
                    },
                    Op::ShowText {
                        items: vec![TextItem::Text(format!("page {n}"))],
                    },
                    Op::EndTextSection,
                ],
            )
        };
        let mut doc = PdfDocument::new("preview fixture");
        doc.with_pages((0..pages).map(page).collect::<Vec<_>>())
            .save(&PdfSaveOptions::default(), &mut Vec::new())
    }

    #[test]
    fn reads_page_count_and_geometry() {
        let meta = page_meta(pdf_bytes(3)).unwrap();
        assert_eq!(meta.pages, 3);
        assert!(meta.width > 0.0 && meta.height > meta.width);
    }

    #[test]
    fn renders_a_page_to_png() {
        let png = page_png(pdf_bytes(2), 1).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn caps_the_raster_width() {
        let png = page_png(pdf_bytes(1), 0).unwrap();
        // The PNG header carries the pixel width in bytes 16..20, big-endian.
        let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        assert!(width <= PREVIEW_WIDTH_PX as u32, "width was {width}");
    }

    #[test]
    fn rejects_a_page_past_the_end() {
        assert_eq!(
            page_png(pdf_bytes(1), 5).unwrap_err(),
            RenderError::PageOutOfRange
        );
    }

    #[test]
    fn rejects_bytes_that_are_not_a_pdf() {
        assert_eq!(
            page_meta(b"not a pdf at all".to_vec()).unwrap_err(),
            RenderError::Unrenderable
        );
        assert_eq!(
            page_png(b"not a pdf at all".to_vec(), 0).unwrap_err(),
            RenderError::Unrenderable
        );
    }
}
