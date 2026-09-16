//! Half-block QR renderer for Lightning invoices in the terminal.
//!
//! Encodes with ECC Level L (screens are clean) and paints explicit
//! `Rgb(0,0,0)` / `Rgb(255,255,255)` so the code stays scannable on any
//! terminal theme. Each cell is an upper-half block (`▀`) covering two
//! modules vertically — the densest packing phone cameras still read
//! reliably. If the symbol does not fit the terminal, the caller shows
//! the invoice as text.

use qrcode::{Color as QrModule, EcLevel, QrCode};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Painted black/white — named `Color::Black`/`White` can be remapped by the terminal.
const DARK: Color = Color::Rgb(0, 0, 0);
const LIGHT: Color = Color::Rgb(255, 255, 255);
const HALF_BLOCK: &str = "▀";

/// ISO/IEC 18004 quiet zone (modules of light padding around the symbol).
pub const QUIET_ZONE_SPEC: u16 = 4;
/// Tighter quiet zone when the spec size does not fit the terminal.
pub const QUIET_ZONE_FALLBACK: u16 = 2;

/// Half-block QR ready to draw into a ratatui [`Paragraph`].
#[derive(Clone)]
pub struct QrView {
    pub lines: Vec<Line<'static>>,
    pub width: u16,
    pub height: u16,
}

/// BOLT11 QR payload: `lightning:` + uppercase bech32 (QR alphanumeric mode).
pub fn qr_payload(raw: &str) -> String {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("lightning:")
        .or_else(|| trimmed.strip_prefix("LIGHTNING:"))
        .unwrap_or(trimmed)
        .trim();
    format!("lightning:{}", body.to_ascii_uppercase())
}

/// Encode `payload` with a quiet zone. `None` if the payload is too long for QR.
pub fn encode_qr(payload: &str, quiet_zone: u16) -> Option<QrView> {
    let code = encode(payload)?;
    let (width, height) = cell_size(code.width(), quiet_zone);
    Some(QrView {
        lines: render_half_block_lines(&code, quiet_zone),
        width,
        height,
    })
}

/// Encode a half-block QR that fits `max_width`×`max_height`. Tries the spec
/// quiet zone first, then a tighter one. `None` if it still cannot fit — the
/// popup should show the invoice text.
pub fn encode_qr_fitting(payload: &str, max_width: u16, max_height: u16) -> Option<QrView> {
    for quiet_zone in [QUIET_ZONE_SPEC, QUIET_ZONE_FALLBACK] {
        if let Some(view) = encode_qr(payload, quiet_zone) {
            if view.width <= max_width && view.height <= max_height {
                return Some(view);
            }
        }
    }
    None
}

fn encode(payload: &str) -> Option<QrCode> {
    QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::L).ok()
}

fn cell_size(modules: usize, quiet_zone: u16) -> (u16, u16) {
    let total = modules.saturating_add((quiet_zone as usize).saturating_mul(2));
    (
        u16::try_from(total).unwrap_or(u16::MAX),
        u16::try_from(total.div_ceil(2)).unwrap_or(u16::MAX),
    )
}

fn module_dark(code: &QrCode, x: isize, y: isize) -> bool {
    let n = code.width() as isize;
    if x < 0 || y < 0 || x >= n || y >= n {
        return false;
    }
    matches!(code[(x as usize, y as usize)], QrModule::Dark)
}

fn module_color(dark: bool) -> Color {
    if dark {
        DARK
    } else {
        LIGHT
    }
}

fn render_half_block_lines(code: &QrCode, quiet_zone: u16) -> Vec<Line<'static>> {
    let quiet = quiet_zone as isize;
    let total = (code.width() as isize + quiet * 2).max(0) as usize;
    let rows = total.div_ceil(2);
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let top_y = row as isize * 2 - quiet;
        let bot_y = row as isize * 2 + 1 - quiet;
        let mut spans = Vec::with_capacity(total);
        for col in 0..total {
            let x = col as isize - quiet;
            spans.push(Span::styled(
                HALF_BLOCK,
                Style::default()
                    .fg(module_color(module_dark(code, x, top_y)))
                    .bg(module_color(module_dark(code, x, bot_y))),
            ));
        }
        lines.push(Line::from(spans));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_payload_prefixes_and_uppercases() {
        assert_eq!(qr_payload("  lnbc1abc  "), "lightning:LNBC1ABC");
    }

    #[test]
    fn qr_payload_does_not_double_prefix() {
        assert_eq!(qr_payload("lightning:lnbc1abc"), "lightning:LNBC1ABC");
        assert_eq!(qr_payload("LIGHTNING:lnbc1abc"), "lightning:LNBC1ABC");
    }

    #[test]
    fn encode_qr_uses_half_blocks_and_explicit_colors() {
        let view = encode_qr("lightning:LNBC1TEST", QUIET_ZONE_SPEC).expect("encode");
        assert!(view.width > 0 && view.height > 0);
        assert_eq!(view.lines.len(), view.height as usize);
        assert_eq!(view.lines[0].spans.len(), view.width as usize);
        assert_eq!(view.lines[0].spans[0].content.as_ref(), HALF_BLOCK);

        let style = view.lines[0].spans[0].style;
        assert_eq!(style.fg, Some(LIGHT), "quiet zone must be light");
        assert_eq!(style.bg, Some(LIGHT), "quiet zone must be light");

        let has_dark = view.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.style.fg == Some(DARK) || span.style.bg == Some(DARK))
        });
        assert!(has_dark, "QR must contain dark modules");
    }

    #[test]
    fn cell_size_matches_quiet_zone_and_half_blocks() {
        let view = encode_qr("LNBC1", QUIET_ZONE_SPEC).expect("encode");
        let code = encode("LNBC1").expect("code");
        let expected_modules = code.width() as u16 + QUIET_ZONE_SPEC * 2;
        assert_eq!(view.width, expected_modules);
        assert_eq!(view.height, expected_modules.div_ceil(2));
    }

    #[test]
    fn encode_qr_fitting_returns_none_when_area_is_tiny() {
        assert!(encode_qr_fitting("lightning:LNBC1TEST", 3, 2).is_none());
    }

    #[test]
    fn encode_qr_fitting_keeps_half_blocks_when_they_fit() {
        let view = encode_qr_fitting("lightning:LNBC1TEST", 80, 40).expect("fit");
        assert_eq!(view.lines[0].spans[0].content.as_ref(), HALF_BLOCK);
    }

    #[test]
    fn encode_qr_fitting_uses_half_blocks_for_typical_bolt11_when_tall_enough() {
        let payload = format!("lightning:{}", "A".repeat(280));
        let view = encode_qr_fitting(&payload, 80, 50).expect("half-block should fit");
        assert_eq!(view.lines[0].spans[0].content.as_ref(), HALF_BLOCK);
    }

    #[test]
    fn encode_qr_fitting_returns_none_when_half_block_does_not_fit() {
        let payload = format!("lightning:{}", "A".repeat(280));
        let view = encode_qr(&payload, QUIET_ZONE_FALLBACK).expect("half-block");
        let max_h = view.height.saturating_sub(1);
        assert!(
            encode_qr_fitting(&payload, 80, max_h).is_none(),
            "too-small terminals must skip QR so the invoice can be shown as text"
        );
    }
}
