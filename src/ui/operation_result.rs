use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{OperationResult, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::orders::OrderSuccess;

/// Split on newlines, then wrap each paragraph at word boundaries.
fn wrap_message_lines(message: &str, width: usize) -> Vec<Line<'static>> {
    let wrap_width = width.max(10);
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

fn info_popup_height(message: &str, popup_width: u16) -> u16 {
    let inner_width = popup_width.saturating_sub(2) as usize;
    let content_lines = wrap_message_lines(message, inner_width).len();
    // content + blank line + footer + top/bottom border padding
    (content_lines + 4).clamp(8, 22) as u16
}

pub fn render_operation_result(f: &mut ratatui::Frame, result: &OperationResult) {
    let area: Rect = f.area();
    let popup_width = 70;
    let popup_height = match result {
        OperationResult::Success(_) => 18,
        OperationResult::PaymentRequestRequired { .. }
        | OperationResult::ObserverChatLoaded(_)
        | OperationResult::ObserverChatError(_) => 8,
        OperationResult::Info(message) => info_popup_height(message, popup_width),
        OperationResult::Error(_)
        | OperationResult::InvoiceSubmitted { .. }
        | OperationResult::TradeClosed { .. }
        | OperationResult::OrderHistoryDeleted { .. }
        | OperationResult::MyTradesMakerBookChanged => 8,
    };
    // Center the popup using Flex::Center
    let popup = {
        let [popup] = Layout::horizontal([Constraint::Length(popup_width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(popup_height)])
            .flex(Flex::Center)
            .areas(popup);
        popup
    };

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
            trade_index: _,
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
            let block = Block::default()
                .title("❌ Operation Failed")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));

            // Calculate inner area (excluding borders)
            let inner = block.inner(popup);
            f.render_widget(block, popup);

            let mut lines = wrap_message_lines(error_msg, inner.width as usize);
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press ESC or ENTER to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
        }
        OperationResult::Info(message)
        | OperationResult::InvoiceSubmitted { message, .. }
        | OperationResult::TradeClosed { message, .. }
        | OperationResult::OrderHistoryDeleted { message, .. } => {
            let block = Block::default()
                .title("✅ Operation Successful")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Green));

            // Calculate inner area (excluding borders)
            let inner = block.inner(popup);
            f.render_widget(block, popup);

            let mut lines = wrap_message_lines(message, inner.width as usize);
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press ESC or ENTER to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
        }
        OperationResult::ObserverChatLoaded(_) | OperationResult::ObserverChatError(_) => {
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
        OperationResult::MyTradesMakerBookChanged => {}
    }
}
