use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{OperationResult, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::helpers::create_centered_popup;
use crate::ui::orders::OrderSuccess;

/// Split on newlines, then wrap each paragraph at word boundaries.
fn wrap_message_lines(message: &str, width: usize) -> Vec<Line<'static>> {
    let wrap_width = width.max(1);
    let mut lines = Vec::new();

    for paragraph in message.split('\n') {
        if paragraph.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= wrap_width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(Line::from(std::mem::take(&mut current)));
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(Line::from(current));
        }
    }

    lines
}

fn info_popup_height(message: &str, popup_width: u16, max_height: u16) -> u16 {
    let inner_width = popup_width.saturating_sub(2) as usize;
    let (_, height) = info_message_popup_layout(message, inner_width, max_height);
    height
}

fn close_footer_line() -> Line<'static> {
    Line::from(vec![Span::styled(
        "Press ESC or ENTER to close",
        Style::default().fg(Color::DarkGray),
    )])
}

fn info_message_lines(message: &str, inner_width: usize, compact: bool) -> Vec<Line<'static>> {
    let mut lines = wrap_message_lines(message, inner_width);
    if !compact {
        lines.push(Line::from(""));
    }
    lines.push(close_footer_line());
    lines
}

/// Pick full or compact info-popup content that fits `max_height` popup rows.
fn info_message_popup_layout(
    message: &str,
    inner_width: usize,
    max_height: u16,
) -> (Vec<Line<'static>>, u16) {
    let full = info_message_lines(message, inner_width, false);
    let full_height = full.len() as u16 + 2;
    if full_height <= max_height {
        return (full, full_height);
    }

    let compact = info_message_lines(message, inner_width, true);
    let compact_height = (compact.len() as u16 + 2).min(max_height);
    (compact, compact_height)
}

fn trim_info_lines_to_inner_height(
    mut lines: Vec<Line<'static>>,
    max_inner_rows: usize,
) -> Vec<Line<'static>> {
    if lines.len() <= max_inner_rows || max_inner_rows == 0 {
        return lines;
    }
    let footer = lines.pop().unwrap_or_else(close_footer_line);
    let budget = max_inner_rows.saturating_sub(1);
    if budget == 0 {
        return vec![footer];
    }
    let start = lines.len().saturating_sub(budget);
    let mut trimmed: Vec<Line<'static>> = lines.into_iter().skip(start).collect();
    trimmed.push(footer);
    trimmed
}

fn render_info_message_block(
    f: &mut ratatui::Frame,
    popup: Rect,
    title: &str,
    title_color: Color,
    message: &str,
    max_popup_height: u16,
) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(title_color));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let inner_width = inner.width.max(1) as usize;
    let max_inner_rows = inner.height.max(1) as usize;
    let (lines, _) = info_message_popup_layout(message, inner_width, max_popup_height);
    let lines = trim_info_lines_to_inner_height(lines, max_inner_rows);

    let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    f.render_widget(paragraph, inner);
}

/// Split a plain hex string into fixed-width chunks so it never gets silently
/// truncated by `Paragraph` on narrow terminals. Hex has no whitespace for
/// [`wrap_message_lines`] to break on, so it needs its own char-width chunking.
fn chunk_hex_lines(hex: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let chars: Vec<char> = hex.chars().collect();
    if chars.is_empty() {
        return vec![Line::from("")];
    }
    chars
        .chunks(width)
        .map(|chunk| Line::from(chunk.iter().collect::<String>()))
        .collect()
}

/// Re-wrap `text` at word boundaries and apply `style` uniformly to every resulting line.
fn styled_wrapped_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    wrap_message_lines(text, width)
        .into_iter()
        .map(|line| {
            let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            Line::from(Span::styled(content, style))
        })
        .collect()
}

/// Wrap a sequence of styled text fragments (e.g. plain text around a bold key
/// name) at word boundaries into possibly multiple `Line`s that fit `width`
/// columns, preserving each word's own fragment style (so a highlighted key
/// like "C" keeps its style even when the surrounding sentence wraps).
fn wrap_styled_fragments(fragments: &[(&str, Style)], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_len = 0usize;

    for (text, style) in fragments {
        for word in text.split_whitespace() {
            let word_len = word.chars().count();
            if !current.is_empty() && current_len + 1 + word_len > width {
                lines.push(Line::from(std::mem::take(&mut current)));
                current_len = 0;
            }
            if !current.is_empty() {
                current.push(Span::raw(" "));
                current_len += 1;
            }
            current.push(Span::styled(word.to_string(), *style));
            current_len += word_len;
        }
    }
    if !current.is_empty() {
        lines.push(Line::from(current));
    }
    if lines.is_empty() {
        lines.push(Line::from(""));
    }
    lines
}

/// Build the Shift+K Shared key disclosure popup's content lines.
///
/// `compact` drops the blank spacer line after the warning so the required
/// elements — Shared key, warning, copy state, and close instruction — stay
/// visible on short terminals instead of being clipped.
fn conversation_disclosure_lines(
    conv_hex: &str,
    copied_to_clipboard: bool,
    inner_width: usize,
    compact: bool,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![Span::styled(
        "Shared key (read-only grant for solvers):",
        Style::default().fg(PRIMARY_COLOR),
    )])];
    lines.extend(chunk_hex_lines(conv_hex, inner_width));
    lines.push(Line::from(""));

    lines.extend(styled_wrapped_lines(
        "Disclose the Shared key only. Never share your signing key.",
        inner_width,
        Style::default().fg(Color::Yellow),
    ));
    if !compact {
        lines.push(Line::from(""));
    }

    if copied_to_clipboard {
        lines.extend(wrap_styled_fragments(
            &[(
                "✓ Shared key copied to clipboard!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )],
            inner_width,
        ));
    } else {
        lines.extend(wrap_styled_fragments(
            &[
                ("Press", Style::default()),
                (
                    "C",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                ("to copy the Shared key to clipboard.", Style::default()),
            ],
            inner_width,
        ));
    }
    lines.extend(wrap_styled_fragments(
        &[(
            "Press ESC or ENTER to close",
            Style::default().fg(Color::DarkGray),
        )],
        inner_width,
    ));

    lines
}

/// Picks the widest layout (full, then compact) whose content fits `max_height`
/// rows, falling back to compact when even the full layout would be clipped.
/// Returns the content lines and the popup height they need (content + borders).
fn conversation_disclosure_layout(
    conv_hex: &str,
    copied_to_clipboard: bool,
    inner_width: usize,
    max_height: u16,
) -> (Vec<Line<'static>>, u16) {
    let full = conversation_disclosure_lines(conv_hex, copied_to_clipboard, inner_width, false);
    let full_height = full.len() as u16 + 2; // + top/bottom border
    if full_height <= max_height {
        return (full, full_height);
    }

    let compact = conversation_disclosure_lines(conv_hex, copied_to_clipboard, inner_width, true);
    let compact_height = (compact.len() as u16 + 2).min(max_height);
    (compact, compact_height)
}

pub fn render_operation_result(f: &mut ratatui::Frame, result: &OperationResult) {
    let area: Rect = f.area();
    let popup_width = 70u16.min(area.width.max(1));
    let inner_width = popup_width.saturating_sub(2).max(1) as usize;
    let disclosure_layout = if let OperationResult::ConversationDisclosure {
        conv_hex,
        copied_to_clipboard,
    } = result
    {
        Some(conversation_disclosure_layout(
            conv_hex,
            *copied_to_clipboard,
            inner_width,
            area.height.max(1),
        ))
    } else {
        None
    };
    let max_popup_height = area.height.max(1);
    let popup_height = match result {
        OperationResult::Success(_) => 18.min(max_popup_height),
        OperationResult::ConversationDisclosure { .. } => {
            disclosure_layout.as_ref().map(|(_, h)| *h).unwrap_or(16)
        }
        OperationResult::PaymentRequestRequired { .. }
        | OperationResult::ObserverChatLoaded { .. }
        | OperationResult::ObserverChatError { .. } => 8.min(max_popup_height),
        OperationResult::Info(message)
        | OperationResult::SessionRestored { message }
        | OperationResult::OrdersRefreshed { message }
        | OperationResult::Error(message) => {
            info_popup_height(message, popup_width, max_popup_height)
        }
        OperationResult::InvoiceSubmitted { .. }
        | OperationResult::TradeClosed { .. }
        | OperationResult::OrderHistoryDeleted { .. }
        | OperationResult::AdminDisputeDeleted { .. }
        | OperationResult::MyTradesMakerBookChanged
        | OperationResult::OpenInvoicePopup { .. }
        | OperationResult::OrderChatAttachmentSent { .. }
        | OperationResult::OrderChatAttachmentSendFailed { .. }
        | OperationResult::OrderChatAttachmentError { .. } => 8,
    };
    // Clamp to the available area so the popup never exceeds narrow/short terminals.
    let popup = create_centered_popup(area, popup_width, popup_height);

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    match result {
        OperationResult::Success(OrderSuccess {
            order_id,
            kind,
            amount,
            fiat_code,
            fiat_amount,
            min_amount,
            max_amount,
            payment_method,
            premium,
            status,
            ..
        }) => {
            let block = Block::default()
                .title("✅ Order Created Successfully")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Green));

            // Calculate inner area (excluding borders)
            let inner = block.inner(popup);
            f.render_widget(block, popup);

            let mut lines = vec![];

            if let Some(id) = order_id {
                lines.push(Line::from(vec![
                    Span::styled("📋 Order ID: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(id.to_string(), Style::default()),
                ]));
            }

            if let Some(k) = kind {
                lines.push(Line::from(vec![
                    Span::styled("📈 Type: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(format!("{:?}", k), Style::default()),
                ]));
            }

            if *amount > 0 {
                lines.push(Line::from(vec![
                    Span::styled("💰 Amount: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(format!("{} sats", amount), Style::default()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("💰 Amount: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled("Market rate", Style::default()),
                ]));
            }

            if let (Some(min), Some(max)) = (min_amount, max_amount) {
                lines.push(Line::from(vec![
                    Span::styled("💵 Fiat Range: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(format!("{}-{} {}", min, max, fiat_code), Style::default()),
                ]));
            } else if *fiat_amount > 0 {
                lines.push(Line::from(vec![
                    Span::styled("💵 Fiat Amount: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(format!("{} {}", fiat_amount, fiat_code), Style::default()),
                ]));
            }

            if !payment_method.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("💳 Payment Method: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(payment_method.clone(), Style::default()),
                ]));
            }

            if *premium != 0 {
                lines.push(Line::from(vec![
                    Span::styled("📈 Premium: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(format!("{}%", premium), Style::default()),
                ]));
            }

            if let Some(s) = status {
                lines.push(Line::from(vec![
                    Span::styled("📊 Status: ", Style::default().fg(PRIMARY_COLOR)),
                    Span::styled(format!("{:?}", s), Style::default()),
                ]));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press ESC or ENTER to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let content_height: u16 = lines.len().try_into().unwrap_or(inner.height);
            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            let vertical_chunks = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Min(0),
                    Constraint::Length(content_height.min(inner.height)),
                    Constraint::Min(0),
                ],
            )
            .split(inner);
            let content_area = vertical_chunks[1];

            f.render_widget(paragraph, content_area);
        }
        OperationResult::Error(error_msg) => {
            render_info_message_block(
                f,
                popup,
                "❌ Operation Failed",
                Color::Red,
                error_msg,
                max_popup_height,
            );
        }
        OperationResult::Info(message)
        | OperationResult::SessionRestored { message }
        | OperationResult::OrdersRefreshed { message }
        | OperationResult::InvoiceSubmitted { message, .. }
        | OperationResult::TradeClosed { message, .. }
        | OperationResult::OrderHistoryDeleted { message, .. }
        | OperationResult::AdminDisputeDeleted { message, .. } => {
            render_info_message_block(
                f,
                popup,
                "✅ Operation Successful",
                Color::Green,
                message,
                max_popup_height,
            );
        }
        OperationResult::ConversationDisclosure { .. } => {
            let block = Block::default()
                .title("✅ Operation Successful")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Green));

            let inner = block.inner(popup);
            f.render_widget(block, popup);

            // Computed above (before the popup was sized) so the height clamp and
            // the rendered content always agree on full vs. compact layout.
            let lines = disclosure_layout
                .map(|(lines, _)| lines)
                .unwrap_or_default();

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
        }
        OperationResult::ObserverChatLoaded { .. } | OperationResult::ObserverChatError { .. } => {
            // Handled directly in handle_operation_result, should not reach render
        }
        OperationResult::PaymentRequestRequired { .. } => {
            // This should not be displayed - it's converted to a notification in main.rs
            // But if it somehow reaches here, show a simple message
            let block = Block::default()
                .title("💳 Payment Request")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));

            let inner = block.inner(popup);
            f.render_widget(block, popup);

            let lines = vec![
                Line::from(vec![Span::styled(
                    "Payment request received",
                    Style::default(),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Press ESC or ENTER to close",
                    Style::default().fg(Color::DarkGray),
                )]),
            ];

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
        }
        OperationResult::MyTradesMakerBookChanged
        | OperationResult::OpenInvoicePopup { .. }
        | OperationResult::OrderChatAttachmentSent { .. }
        | OperationResult::OrderChatAttachmentSendFailed { .. }
        | OperationResult::OrderChatAttachmentError { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut hay = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                hay.push_str(buf[(x, y)].symbol());
            }
        }
        hay.contains(needle)
    }

    fn render(result: &OperationResult) -> ratatui::buffer::Buffer {
        render_at(result, 80, 24)
    }

    fn render_at(result: &OperationResult, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| render_operation_result(f, result))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn conversation_disclosure_shows_shared_key_and_copy_hint() {
        let result = OperationResult::ConversationDisclosure {
            conv_hex: "a".repeat(64),
            copied_to_clipboard: false,
        };
        let buf = render(&result);
        assert!(
            buffer_contains(&buf, "Shared key"),
            "popup must label the disclosed K_conv secret as Shared key"
        );
        assert!(
            buffer_contains(&buf, &"a".repeat(64)),
            "popup must show the Shared key hex"
        );
        assert!(
            !buffer_contains(&buf, "Signer pubkey"),
            "popup should not display a Signer pubkey locator"
        );
        assert!(
            buffer_contains(&buf, "Never share your signing key"),
            "popup must warn against disclosing the signing key"
        );
        assert!(
            buffer_contains(&buf, "C") && buffer_contains(&buf, "clipboard"),
            "popup must hint that C copies the Shared key"
        );
        assert!(
            !buffer_contains(&buf, "K_conv"),
            "popup must not label the field K_conv; users see Shared key"
        );
    }

    #[test]
    fn conversation_disclosure_shows_copied_confirmation() {
        let result = OperationResult::ConversationDisclosure {
            conv_hex: "a".repeat(64),
            copied_to_clipboard: true,
        };
        let buf = render(&result);
        assert!(
            buffer_contains(&buf, "Shared key copied to clipboard"),
            "popup must confirm the Shared key was copied"
        );
    }

    #[test]
    fn conversation_disclosure_popup_fits_within_a_constrained_terminal() {
        // Regression: on a narrow (< 70 cols) and short (< 16 rows) terminal, the
        // popup must not exceed the terminal, and its content must not be clipped
        // or silently truncated (the full Shared key hex must remain readable).
        //
        // Uses 'q' (absent from every static string in the popup) as the hex
        // digit so counting occurrences unambiguously measures the rendered Shared
        // key, not incidental letters from prose like "Shared" or "share".
        let conv_hex = "q".repeat(64);
        let result = OperationResult::ConversationDisclosure {
            conv_hex: conv_hex.clone(),
            copied_to_clipboard: false,
        };
        let (width, height) = (40, 12);
        let buf = render_at(&result, width, height);

        assert!(
            buf.area.width <= width,
            "popup must not exceed terminal width"
        );
        assert!(
            buf.area.height <= height,
            "popup must not exceed terminal height"
        );

        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
        }
        let hex_char_count = flat.chars().filter(|c| *c == 'q').count();
        assert_eq!(
            hex_char_count,
            conv_hex.len(),
            "the full Shared key hex must be visible, not truncated, on a narrow terminal"
        );
        // Word wrap may split the warning/hint across lines (each independently
        // centered, so exact multi-word phrases can gain extra padding spaces
        // between wrapped lines); check for representative words instead.
        assert!(
            flat.contains("Disclose") && flat.contains("Never") && flat.contains("signing"),
            "the disclosure warning must remain visible on a short terminal"
        );
        assert!(
            flat.contains("Press") && flat.contains('C') && flat.contains("clipboard"),
            "the copy-state hint must remain visible on a short terminal"
        );
        assert!(
            flat.contains("ESC") && flat.contains("ENTER") && flat.contains("close"),
            "the close instruction must remain visible on a short terminal"
        );
    }

    #[test]
    fn conversation_disclosure_layout_clamps_height_to_available_area() {
        let (lines, height) = conversation_disclosure_layout(&"a".repeat(64), false, 38, 6);
        assert!(
            height <= 6,
            "popup height must never exceed the available area"
        );
        assert!(
            (lines.len() as u16 + 2) >= height,
            "returned lines should justify the computed height"
        );
    }

    #[test]
    fn conversation_disclosure_layout_prefers_full_layout_when_it_fits() {
        let (lines, height) = conversation_disclosure_layout(&"a".repeat(64), false, 38, 24);
        let compact_lines = conversation_disclosure_lines(&"a".repeat(64), false, 38, true);
        assert!(
            lines.len() > compact_lines.len(),
            "full layout (plenty of height) should keep the spacer line compact mode drops"
        );
        assert_eq!(height, lines.len() as u16 + 2);
    }

    const RESTORE_SUMMARY: &str = "Session restored: 3 order(s) recovered, 1 already known, \
1 dispute(s). 1 order(s) had no relay details and were saved with minimal info.";

    fn render_restore_summary(width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render_operation_result(
                    f,
                    &OperationResult::SessionRestored {
                        message: RESTORE_SUMMARY.to_string(),
                    },
                )
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn restore_summary_fits_on_a_narrow_terminal() {
        let buf = render_restore_summary(40, 24);
        assert!(buffer_contains(&buf, "recovered,"));
        assert!(buffer_contains(&buf, "Press ESC or ENTER to close"));
    }

    #[test]
    fn restore_summary_keeps_counts_visible_on_a_short_terminal() {
        let buf = render_restore_summary(40, 12);
        assert!(buffer_contains(&buf, "Session restored:"));
        assert!(buffer_contains(&buf, "Press ESC or ENTER to close"));
    }

    #[test]
    fn restore_summary_keeps_close_hint_on_a_very_short_terminal() {
        let buf = render_restore_summary(40, 8);
        assert!(buffer_contains(&buf, "Session restored:"));
        assert!(buffer_contains(&buf, "Press ESC or ENTER to close"));
    }

    #[test]
    fn restore_summary_wraps_on_ultra_narrow_width() {
        let buf = render_restore_summary(12, 24);
        assert!(buffer_contains(&buf, "Session"));
        assert!(buffer_contains(&buf, "ESC") || buffer_contains(&buf, "ENTER"));
    }

    #[test]
    fn info_message_layout_prefers_compact_on_short_height() {
        let (full, _) = info_message_popup_layout(RESTORE_SUMMARY, 36, 24);
        let (compact, compact_height) = info_message_popup_layout(RESTORE_SUMMARY, 36, 8);
        assert!(
            compact.len() < full.len(),
            "compact layout should drop the spacer before the footer"
        );
        assert!(compact_height <= 8);
    }
}
