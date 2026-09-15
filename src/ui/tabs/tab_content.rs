use mostro_core::prelude::*;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap};

use crate::ui::{
    helpers::{self, render_centered_lines},
    MessageViewState, RatingOrderState, ThreeState, ViewingMessageButtonSelection,
    BACKGROUND_COLOR, PRIMARY_COLOR,
};

pub fn render_coming_soon(f: &mut ratatui::Frame, area: Rect, title: &str) {
    let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(PRIMARY_COLOR))
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );
    f.render_widget(paragraph, area);
}

/// Returns ASCII art logo for Mostro
fn get_mostro_logo() -> Vec<&'static str> {
    vec![
        "    ███╗   ███╗ ██████╗ ███████╗████████╗██████╗  ██████╗ ",
        "    ████╗ ████║██╔═══██╗██╔════╝╚══██╔══╝██╔══██╗██╔═══██╗",
        "    ██╔████╔██║██║   ██║███████╗   ██║   ██████╔╝██║   ██║",
        "    ██║╚██╔╝██║██║   ██║╚════██║   ██║   ██╔══██╗██║   ██║",
        "    ██║ ╚═╝ ██║╚██████╔╝███████║   ██║   ██║  ██║╚██████╔╝",
        "    ╚═╝     ╚═╝ ╚═════╝ ╚══════╝   ╚═╝   ╚═╝  ╚═╝ ╚═════╝ ",
        "                                                              ",
        "              ╔═══════════════════════════╗                 ",
        "              ║   Press Enter to exit     ║                 ",
        "              ╚═══════════════════════════╝                 ",
        "    ",
    ]
}

fn style_exit_logo_line(line: &str) -> Vec<Span<'static>> {
    if line.contains('█') {
        line.chars()
            .map(|c| {
                if c == '█' {
                    Span::styled(
                        c.to_string(),
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw(c.to_string())
                }
            })
            .collect()
    } else if line.contains('╔') || line.contains('║') || line.contains('╚') {
        line.chars()
            .map(|c| {
                if ['╔', '║', '╚', '═', '╗', '╝'].contains(&c) {
                    Span::styled(
                        c.to_string(),
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw(c.to_string())
                }
            })
            .collect()
    } else {
        vec![Span::raw(line.to_string())]
    }
}

/// Renders the Exit tab content with ASCII art logo
pub fn render_exit_tab(f: &mut ratatui::Frame, area: Rect) {
    let logo_lines = get_mostro_logo();

    f.render_widget(
        Block::default()
            .title("Exit")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(PRIMARY_COLOR))
            .style(Style::default().bg(BACKGROUND_COLOR)),
        area,
    );

    let inner_area = Block::default()
        .title("Exit")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR))
        .inner(area);

    let line_refs: Vec<&str> = logo_lines.to_vec();
    render_centered_lines(f, inner_area, &line_refs, style_exit_logo_line);
}

pub fn render_message_view(f: &mut ratatui::Frame, view_state: &MessageViewState) {
    let area = f.area();
    // Keep a margin on roomy terminals; use the full width when every column counts.
    let popup_width = if area.width < MESSAGE_VIEW_FULL_WIDTH_BELOW {
        area.width
    } else {
        area.width.saturating_sub(area.width / 4)
    };

    // YES/NO (or YES/NO/CANCEL for hold invoice): same pattern as exit/settings confirms.
    let show_buttons = matches!(
        view_state.action,
        Action::HoldInvoicePaymentAccepted
            | Action::BuyerTookOrder
            | Action::FiatSentOk
            | Action::CooperativeCancelInitiatedByPeer
            | Action::Cancel
            | Action::FiatSent
            | Action::Release
            | Action::Dispute
            | Action::Orders
    );

    let hold_invoice_trinary = matches!(view_state.action, Action::HoldInvoicePaymentAccepted)
        && matches!(
            view_state.button_selection,
            ViewingMessageButtonSelection::Three(_)
        );

    // Multiline body: hold-invoice trinary, or `BuyerTookOrder` (CANCEL / NO for cooperative cancel).
    let multiline_message_body =
        hold_invoice_trinary || matches!(view_state.action, Action::BuyerTookOrder);

    let body_style = Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR);
    // Multi-line Text so `\n` in the string becomes real line breaks in ratatui.
    let body_lines: Vec<Line> = if multiline_message_body {
        view_state
            .message_content
            .lines()
            .map(|line| Line::from(vec![Span::styled(line, body_style)]))
            .collect()
    } else {
        vec![Line::from(vec![Span::styled(
            view_state.message_content.as_str(),
            body_style,
        )])]
    };
    let help_line = message_view_help_line(show_buttons, hold_invoice_trinary);

    let inner_width = popup_width.saturating_sub(2);
    // The multiline body carries one column of horizontal padding on each side.
    let body_padding = if multiline_message_body { 2 } else { 0 };
    let body_rows =
        helpers::wrapped_rows(&body_lines, inner_width.saturating_sub(body_padding)).max(1);
    let help_rows = helpers::wrapped_rows(std::slice::from_ref(&help_line), inner_width).max(1);
    let buttons_h: u16 = if show_buttons { 3 } else { 0 };

    // spacer + order id + body + spacer + buttons + help
    let needed_inner = 1 + 1 + body_rows + 1 + buttons_h + help_rows;
    let popup = helpers::create_centered_popup(area, popup_width, needed_inner.saturating_add(2));

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title("📨 Message")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = message_view_rows(inner.height, show_buttons, body_rows, help_rows);
    // Not enough rows for the wrapped sentence: fall back to the terse key hint.
    let help_line = if rows.help < help_rows {
        message_view_short_help_line(show_buttons, hold_invoice_trinary)
    } else {
        help_line
    };
    let [_, order_area, body_area, _, button_area, help_area] = Layout::vertical([
        Constraint::Length(rows.top_spacer),
        Constraint::Length(rows.order_id),
        Constraint::Length(rows.body),
        Constraint::Length(rows.mid_spacer),
        Constraint::Length(rows.buttons),
        Constraint::Length(rows.help),
    ])
    .areas(inner);

    // Order ID
    let order_id_str = helpers::format_order_id(view_state.order_id);
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            order_id_str,
            Style::default()
                .bg(BACKGROUND_COLOR)
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        order_area,
    );

    let mut message_paragraph = Paragraph::new(Text::from(body_lines))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    if multiline_message_body {
        message_paragraph = message_paragraph.block(
            Block::default()
                .padding(Padding::horizontal(1))
                .style(Style::default().bg(BACKGROUND_COLOR)),
        );
    }
    f.render_widget(message_paragraph, body_area);

    if show_buttons {
        if hold_invoice_trinary {
            let selected = match view_state.button_selection {
                ViewingMessageButtonSelection::Three(selected) => selected.index(),
                ViewingMessageButtonSelection::Two { .. } => 0,
            };
            helpers::render_yes_no_cancel_buttons(
                f,
                button_area,
                selected,
                "✓ YES",
                "✗ NO",
                "CANCEL",
            );
        } else {
            let yes_selected = match view_state.button_selection {
                ViewingMessageButtonSelection::Two { yes_selected } => yes_selected,
                ViewingMessageButtonSelection::Three(ThreeState::Yes)
                | ViewingMessageButtonSelection::Three(ThreeState::Cancel) => true,
                ViewingMessageButtonSelection::Three(ThreeState::No) => false,
            };
            let (yes_label, no_label) = if matches!(view_state.action, Action::BuyerTookOrder) {
                ("CANCEL", "NO")
            } else {
                ("✓ YES", "✗ NO")
            };
            // Shrink the 18-column buttons so both stay whole on narrow terminals.
            let button_width = 18u16.min(button_area.width.saturating_sub(1) / 2);
            helpers::render_yes_no_buttons_with_width(
                f,
                button_area,
                button_width,
                yes_selected,
                yes_label,
                no_label,
            );
        }
    }

    f.render_widget(
        Paragraph::new(help_line)
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: true }),
        help_area,
    );
}

/// Below this terminal width the message popup spans the full width.
const MESSAGE_VIEW_FULL_WIDTH_BELOW: u16 = 60;

/// Row budget for [`render_message_view`].
#[derive(Debug, PartialEq, Eq)]
struct MessageViewRows {
    top_spacer: u16,
    order_id: u16,
    body: u16,
    mid_spacer: u16,
    buttons: u16,
    help: u16,
}

/// Split `inner_height` rows between the message popup sections.
///
/// When the popup cannot get all it needs, rows are granted in priority order
/// so the actionable parts survive: buttons, the first body and help rows, the
/// rest of the body, the order id, the rest of the help, and only then spacers.
fn message_view_rows(
    inner_height: u16,
    show_buttons: bool,
    body_rows: u16,
    help_rows: u16,
) -> MessageViewRows {
    let mut remaining = inner_height;
    let mut take = |want: u16| {
        let got = want.min(remaining);
        remaining -= got;
        got
    };
    let buttons = take(if show_buttons { 3 } else { 0 });
    let body_first = take(1);
    let help_first = take(1);
    let body_rest = take(body_rows.saturating_sub(1));
    let order_id = take(1);
    let help_rest = take(help_rows.saturating_sub(1));
    let top_spacer = take(1);
    let mid_spacer = take(1);
    MessageViewRows {
        top_spacer,
        order_id,
        body: body_first + body_rest,
        mid_spacer,
        buttons,
        help: help_first + help_rest,
    }
}

/// One-row variant of [`message_view_help_line`] for cramped terminals.
fn message_view_short_help_line(show_buttons: bool, hold_invoice_trinary: bool) -> Line<'static> {
    let key = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    if !show_buttons {
        return Line::from(vec![
            Span::styled("Esc", key),
            Span::styled("/", Style::default()),
            Span::styled("Return", key),
            Span::styled(" exit", Style::default()),
        ]);
    }
    let move_verb = if hold_invoice_trinary {
        " cycle · "
    } else {
        " select · "
    };
    Line::from(vec![
        Span::styled("←/→", key),
        Span::styled(move_verb, Style::default()),
        Span::styled("Enter", key),
        Span::styled(" ok · ", Style::default()),
        Span::styled("Esc", key),
        Span::styled(" dismiss", Style::default()),
    ])
}

fn message_view_help_line(show_buttons: bool, hold_invoice_trinary: bool) -> Line<'static> {
    let key = Style::default()
        .fg(PRIMARY_COLOR)
        .add_modifier(Modifier::BOLD);
    if !show_buttons {
        // Simple exit text for other actions
        return Line::from(vec![
            Span::styled("Press ", Style::default()),
            Span::styled("Esc", key),
            Span::styled(" or ", Style::default()),
            Span::styled("Return", key),
            Span::styled(" to exit", Style::default()),
        ]);
    }
    let select = if hold_invoice_trinary {
        " to cycle YES / NO / CANCEL, "
    } else {
        " to select, "
    };
    Line::from(vec![
        Span::styled("Use ", Style::default()),
        Span::styled("Left/Right", key),
        Span::styled(select, Style::default()),
        Span::styled("Enter", key),
        Span::styled(" to confirm, ", Style::default()),
        Span::styled("Esc", key),
        Span::styled(" to dismiss", Style::default()),
    ])
}

/// Popup to choose a 1..=5 star rating before sending `RateUser` to Mostro.
pub fn render_rating_order(f: &mut ratatui::Frame, state: &RatingOrderState) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);
    let popup = helpers::create_centered_popup(area, popup_width, 14);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .title("Rate counterparty")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PRIMARY_COLOR))
        .style(Style::default().bg(BACKGROUND_COLOR));
    f.render_widget(block.clone(), popup);
    let inner = block.inner(popup);
    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    )
    .split(inner);

    let order_line = helpers::format_order_id(Some(state.order_id));
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            order_line,
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[1],
    );

    let stars: String = (1..=5)
        .map(|i| {
            if i <= state.selected_rating {
                "★ "
            } else {
                "☆ "
            }
        })
        .collect::<String>();
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            stars.trim_end(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[2],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!(
            "{} / {}",
            state.selected_rating, MAX_RATING
        ))]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[3],
    );

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Left/Right ", Style::default().fg(PRIMARY_COLOR)),
            Span::raw("or "),
            Span::styled("+/- ", Style::default().fg(PRIMARY_COLOR)),
            Span::raw("adjust  "),
            Span::styled("Enter ", Style::default().fg(PRIMARY_COLOR)),
            Span::raw("submit  "),
            Span::styled("Esc ", Style::default().fg(PRIMARY_COLOR)),
            Span::raw("cancel"),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::render_message_view;
    use crate::ui::{MessageViewState, ViewingMessageButtonSelection};
    use mostro_core::prelude::Action;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use uuid::Uuid;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    #[test]
    fn refresh_confirmation_shows_yes_no_buttons() {
        let view_state = MessageViewState {
            message_content: crate::ui::constants::HELP_MY_TRADES_REFRESH_MSG.to_string(),
            order_id: Some(Uuid::nil()),
            action: Action::Orders,
            button_selection: ViewingMessageButtonSelection::Two { yes_selected: true },
        };
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_message_view(frame, &view_state))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer_contains(buffer, "YES"));
        assert!(buffer_contains(buffer, "NO"));
        assert!(buffer_contains(buffer, "Refresh this order"));
    }

    fn draw_message_view(
        width: u16,
        height: u16,
        view_state: &MessageViewState,
    ) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_message_view(frame, view_state))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn refresh_confirmation_shows_the_whole_question_on_small_terminals() {
        let view_state = MessageViewState {
            message_content: crate::ui::constants::HELP_MY_TRADES_REFRESH_MSG.to_string(),
            order_id: Some(Uuid::nil()),
            action: Action::Orders,
            button_selection: ViewingMessageButtonSelection::Two { yes_selected: true },
        };
        let last_word = crate::ui::constants::HELP_MY_TRADES_REFRESH_MSG
            .split_whitespace()
            .last()
            .unwrap();
        for (width, height) in [(40u16, 12u16), (40, 24), (80, 24)] {
            let buffer = draw_message_view(width, height, &view_state);
            let label = format!("{width}x{height}");
            // Regression: the single-line body used to get one row and cut the question.
            assert!(buffer_contains(&buffer, "Refresh this order"), "{label}");
            assert!(
                buffer_contains(&buffer, last_word),
                "{label}: question truncated"
            );
            assert!(
                buffer_contains(&buffer, "YES") && buffer_contains(&buffer, "NO"),
                "{label}"
            );
            assert!(buffer_contains(&buffer, "Esc"), "{label}: key hint missing");
        }
    }

    #[test]
    fn hold_invoice_confirmation_keeps_three_buttons_and_a_hint_at_40x12() {
        let view_state = MessageViewState {
            message_content: "Seller paid the hold invoice.\n\nSend the fiat payment now.\n\
YES: mark fiat sent\nNO: not yet\nCANCEL: request cooperative cancel"
                .to_string(),
            order_id: Some(Uuid::nil()),
            action: Action::HoldInvoicePaymentAccepted,
            button_selection: ViewingMessageButtonSelection::Three(crate::ui::ThreeState::Yes),
        };
        for (width, height) in [(40u16, 12u16), (40, 24)] {
            let buffer = draw_message_view(width, height, &view_state);
            let label = format!("{width}x{height}");
            for needle in ["YES", "NO", "CANCEL", "Esc", "Seller paid"] {
                assert!(
                    buffer_contains(&buffer, needle),
                    "{label}: missing {needle:?}"
                );
            }
        }
    }

    #[test]
    fn message_view_rows_give_up_spacers_before_buttons_body_or_help() {
        use super::{message_view_rows, MessageViewRows};

        // Plenty of room: everything gets what it asked for.
        assert_eq!(
            message_view_rows(11, true, 3, 2),
            MessageViewRows {
                top_spacer: 1,
                order_id: 1,
                body: 3,
                mid_spacer: 1,
                buttons: 3,
                help: 2
            }
        );
        // Two rows short: both spacers go first.
        assert_eq!(
            message_view_rows(9, true, 3, 2),
            MessageViewRows {
                top_spacer: 0,
                order_id: 1,
                body: 3,
                mid_spacer: 0,
                buttons: 3,
                help: 2
            }
        );
        // Very short: buttons, one body row and one help row survive.
        assert_eq!(
            message_view_rows(5, true, 3, 2),
            MessageViewRows {
                top_spacer: 0,
                order_id: 0,
                body: 1,
                mid_spacer: 0,
                buttons: 3,
                help: 1
            }
        );
    }
}
