use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Paragraph};

use crate::ui::PRIMARY_COLOR;

/// Creates a centered popup area within the given area.
pub fn create_centered_popup(area: Rect, width: u16, height: u16) -> Rect {
    let (popup_width, popup_height) = (width.min(area.width), height.min(area.height));
    let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::vertical([Constraint::Length(popup_height)])
        .flex(Flex::Center)
        .areas(popup);
    popup
}

/// Renders help text with a styled key binding.
pub fn render_help_text(f: &mut ratatui::Frame, area: Rect, prefix: &str, key: &str, suffix: &str) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, Style::default()),
            Span::styled(
                key,
                Style::default()
                    .fg(PRIMARY_COLOR)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(suffix, Style::default()),
        ]))
        .alignment(ratatui::layout::Alignment::Center),
        area,
    );
}

/// Render a pair of centered YES/NO buttons inside the given area.
/// `selected_button = true` highlights YES, `false` highlights NO.
pub fn render_yes_no_buttons(
    f: &mut ratatui::Frame,
    area: Rect,
    selected_button: bool,
    yes_label: &str,
    no_label: &str,
) {
    let button_width = 15;
    let separator_width = 1;
    let total_button_width = (button_width * 2) + separator_width;

    let button_x = area.x + (area.width.saturating_sub(total_button_width)) / 2;
    let centered_button_area = Rect {
        x: button_x,
        y: area.y,
        width: total_button_width.min(area.width),
        height: area.height,
    };

    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Length(button_width),
            Constraint::Length(separator_width),
            Constraint::Length(button_width),
        ],
    )
    .split(centered_button_area);

    let yes_style = if selected_button {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let yes_block = ratatui::widgets::Block::default()
        .borders(Borders::ALL)
        .style(yes_style);
    f.render_widget(yes_block, button_chunks[0]);

    let yes_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[0]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            yes_label,
            Style::default()
                .fg(if selected_button {
                    Color::Black
                } else {
                    Color::Green
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        yes_inner[0],
    );

    let no_style = if !selected_button {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let no_block = ratatui::widgets::Block::default()
        .borders(Borders::ALL)
        .style(no_style);
    f.render_widget(no_block, button_chunks[2]);

    let no_inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
        .margin(1)
        .split(button_chunks[2]);

    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            no_label,
            Style::default()
                .fg(if !selected_button {
                    Color::Black
                } else {
                    Color::Red
                })
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(ratatui::layout::Alignment::Center),
        no_inner[0],
    );
}

/// Three buttons: YES (green), NO (red), CANCEL (yellow). `selected` is `0`, `1`, or `2`.
pub fn render_yes_no_cancel_buttons(
    f: &mut ratatui::Frame,
    area: Rect,
    selected: u8,
    yes_label: &str,
    no_label: &str,
    cancel_label: &str,
) {
    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            // spacer to avoid buttons from touching the frame border
            Constraint::Length(2),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            // spacer to avoid buttons from touching the frame border
            Constraint::Length(2),
        ],
    )
    .split(area);

    let mut render_one =
        |idx: u8, chunk: Rect, label: &str, current: u8, base_fg: Color, selected_bg: Color| {
            let is_on = current == idx;
            let block_style = if is_on {
                Style::default()
                    .bg(selected_bg)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(base_fg).add_modifier(Modifier::BOLD)
            };
            let block = ratatui::widgets::Block::default()
                .borders(Borders::ALL)
                .style(block_style);
            f.render_widget(block, chunk);
            let inner = Layout::new(Direction::Vertical, [Constraint::Min(0)])
                .margin(1)
                .split(chunk);
            f.render_widget(
                Paragraph::new(Line::from(vec![Span::styled(
                    label,
                    Style::default()
                        .fg(if is_on { Color::Black } else { base_fg })
                        .add_modifier(Modifier::BOLD),
                )]))
                .alignment(ratatui::layout::Alignment::Center),
                inner[0],
            );
        };

    render_one(
        0,
        button_chunks[1],
        yes_label,
        selected,
        Color::Green,
        Color::Green,
    );
    render_one(
        1,
        button_chunks[2],
        no_label,
        selected,
        Color::Red,
        Color::Red,
    );
    render_one(
        2,
        button_chunks[3],
        cancel_label,
        selected,
        Color::Yellow,
        Color::Yellow,
    );
}
