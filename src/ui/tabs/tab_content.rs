use mostro_core::prelude::*;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap};

use crate::ui::{
    helpers, MessageViewState, RatingOrderState, ViewingMessageButtonSelection, BACKGROUND_COLOR,
    PRIMARY_COLOR,
};

pub fn render_coming_soon(f: &mut ratatui::Frame, area: Rect, title: &str) {
    let paragraph = Paragraph::new(Span::raw("Coming soon")).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
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

/// Renders the Exit tab content with ASCII art logo
pub fn render_exit_tab(f: &mut ratatui::Frame, area: Rect) {
    let logo_lines = get_mostro_logo();

    // Create a layout to center the logo vertically
    let inner_area = Block::default()
        .title("Exit")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR))
        .inner(area);

    let logo_height = logo_lines.len() as u16;
    let available_height = inner_area.height;

    // Calculate vertical centering
    let start_y = if logo_height < available_height {
        inner_area.y + (available_height.saturating_sub(logo_height)) / 2
    } else {
        inner_area.y
    };

    // Render the block
    f.render_widget(
        Block::default()
            .title("Exit")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR)),
        area,
    );

    // Render ASCII art logo line by line
    for (idx, line) in logo_lines.iter().enumerate() {
        let y = start_y + idx as u16;
        if y < inner_area.y + inner_area.height {
            // Center the line horizontally
            let line_width = line.chars().count() as u16;
            let start_x = if line_width < inner_area.width {
                inner_area.x + (inner_area.width.saturating_sub(line_width)) / 2
            } else {
                inner_area.x
            };

            let centered_rect = Rect {
                x: start_x,
                y,
                width: line_width.min(inner_area.width),
                height: 1,
            };

            // Style different parts of the logo
            let spans: Vec<Span> = if line.contains('█') {
                // Style the ASCII art logo (block characters) with primary color
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
                // Style the box with primary color
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
                vec![Span::raw(*line)]
            };

            f.render_widget(Paragraph::new(Line::from(spans)), centered_rect);
        }
    }
}

pub fn render_message_view(f: &mut ratatui::Frame, view_state: &MessageViewState) {
    let area = f.area();
    let popup_width = area.width.saturating_sub(area.width / 4);

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
    );

    let hold_invoice_trinary = matches!(view_state.action, Action::HoldInvoicePaymentAccepted)
        && matches!(
            view_state.button_selection,
            ViewingMessageButtonSelection::Three { .. }
        );

    // Multiline body: hold-invoice trinary, or `BuyerTookOrder` (CANCEL / NO for cooperative cancel).
    let multiline_message_body =
        hold_invoice_trinary || matches!(view_state.action, Action::BuyerTookOrder);

    let content_line_count = if multiline_message_body {
        view_state.message_content.lines().count().max(1) as u16
    } else {
        0
    };
    let max_popup_h = area.height.saturating_sub(4).max(12);
    let mut message_chunk_height = if multiline_message_body {
        // Extra rows for soft-wrapped lines on narrow terminals.
        content_line_count.saturating_add(2).clamp(8, 14)
    } else {
        1
    };

    // Rows: spacer + title + sep + order + message + spacer + buttons + help  => 10 + message_chunk
    let mut inner_needed = 10u16.saturating_add(message_chunk_height);
    if multiline_message_body && inner_needed > max_popup_h {
        let shrink = inner_needed - max_popup_h;
        message_chunk_height = (message_chunk_height.saturating_sub(shrink)).max(6);
        inner_needed = 10u16.saturating_add(message_chunk_height);
    }

    let popup_height = if show_buttons {
        if multiline_message_body {
            inner_needed.min(max_popup_h)
        } else {
            14
        }
    } else {
        10
    };

    // Center the popup
    let popup = helpers::create_centered_popup(area, popup_width, popup_height);

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    let constraints = if show_buttons && multiline_message_body {
        vec![
            Constraint::Length(1), // spacer
            Constraint::Length(1), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // order id
            Constraint::Length(message_chunk_height),
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons
            Constraint::Length(2), // help text
        ]
    } else if show_buttons {
        vec![
            Constraint::Length(1), // spacer
            Constraint::Length(1), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // order id
            Constraint::Length(1), // message content
            Constraint::Length(1), // spacer
            Constraint::Length(3), // buttons (need space for borders)
            Constraint::Length(1), // help text
        ]
    } else {
        vec![
            Constraint::Length(1), // spacer
            Constraint::Length(1), // title
            Constraint::Length(1), // separator
            Constraint::Length(1), // order id
            Constraint::Length(1), // message content
            Constraint::Length(1), // spacer
            Constraint::Length(1), // exit text
        ]
    };

    let inner_chunks = Layout::new(Direction::Vertical, constraints).split(popup);

    let block = Block::default()
        .title("📨 Message")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

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
        inner_chunks[3],
    );

    // Message content (multi-line Text so `\n` in the string becomes real line breaks in ratatui)
    let body_style = Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR);
    let message_paragraph = if multiline_message_body {
        let lines: Vec<Line> = view_state
            .message_content
            .lines()
            .map(|line| Line::from(vec![Span::styled(line, body_style)]))
            .collect();
        Paragraph::new(Text::from(lines))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .padding(Padding::horizontal(1))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            )
            .wrap(Wrap { trim: true })
    } else {
        Paragraph::new(Line::from(vec![Span::styled(
            view_state.message_content.as_str(),
            body_style,
        )]))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
    };
    f.render_widget(message_paragraph, inner_chunks[4]);

    if show_buttons {
        let button_area = inner_chunks[6];
        if hold_invoice_trinary {
            let selected = match view_state.button_selection {
                ViewingMessageButtonSelection::Three { selected } => selected.min(2),
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
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Use ", Style::default()),
                    Span::styled(
                        "Left/Right",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to cycle YES / NO / CANCEL, ", Style::default()),
                    Span::styled(
                        "Enter",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to confirm, ", Style::default()),
                    Span::styled(
                        "Esc",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to dismiss", Style::default()),
                ]))
                .alignment(ratatui::layout::Alignment::Center),
                inner_chunks[7],
            );
        } else {
            let yes_selected = match view_state.button_selection {
                ViewingMessageButtonSelection::Two { yes_selected } => yes_selected,
                ViewingMessageButtonSelection::Three { .. } => true,
            };
            let (yes_label, no_label) = if matches!(view_state.action, Action::BuyerTookOrder) {
                ("CANCEL", "NO")
            } else {
                ("✓ YES", "✗ NO")
            };
            helpers::render_yes_no_buttons(f, button_area, yes_selected, yes_label, no_label);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Use ", Style::default()),
                    Span::styled(
                        "Left/Right",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to select, ", Style::default()),
                    Span::styled(
                        "Enter",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to confirm, ", Style::default()),
                    Span::styled(
                        "Esc",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" to dismiss", Style::default()),
                ]))
                .alignment(ratatui::layout::Alignment::Center),
                inner_chunks[7],
            );
        }
    } else {
        // Simple exit text for other actions
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Press ", Style::default()),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" or ", Style::default()),
                Span::styled(
                    "Return",
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" to exit", Style::default()),
            ]))
            .alignment(ratatui::layout::Alignment::Center),
            inner_chunks[6],
        );
    }
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
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
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
