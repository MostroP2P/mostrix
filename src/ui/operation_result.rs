use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{OperationResult, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::orders::OrderSuccess;

pub fn render_operation_result(f: &mut ratatui::Frame, result: &OperationResult) {
    let area: Rect = f.area();
    let popup_width = 70;
    let popup_height = match result {
        OperationResult::Success(_) => 18,
        OperationResult::PaymentRequestRequired { .. } => 8, // Should not be displayed, converted to notification
        OperationResult::Error(_) | OperationResult::Info(_) => 8,
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

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
        }
        OperationResult::Error(error_msg) => {
            let block = Block::default()
                .title("❌ Operation Failed")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));

            // Calculate inner area (excluding borders)
            let inner = block.inner(popup);
            f.render_widget(block, popup);

            // Wrap error message if too long (accounting for borders)
            let error_lines: Vec<Line> = error_msg
                .chars()
                .collect::<Vec<_>>()
                .chunks(inner.width as usize - 2)
                .map(|chunk| Line::from(chunk.iter().collect::<String>()))
                .collect();

            let mut lines = vec![];
            for line in error_lines {
                lines.push(line);
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press ESC or ENTER to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
        }
        OperationResult::Info(message) => {
            let block = Block::default()
                .title("✅ Operation Successful")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Green));

            // Calculate inner area (excluding borders)
            let inner = block.inner(popup);
            f.render_widget(block, popup);

            // Wrap message if too long (accounting for borders)
            let info_lines: Vec<Line> = message
                .chars()
                .collect::<Vec<_>>()
                .chunks(inner.width as usize - 2)
                .map(|chunk| Line::from(chunk.iter().collect::<String>()))
                .collect();

            let mut lines = vec![];
            for line in info_lines {
                lines.push(line);
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press ESC or ENTER to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
            f.render_widget(paragraph, inner);
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
    }
}
