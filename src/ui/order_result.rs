use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use super::{OrderResult, BACKGROUND_COLOR, PRIMARY_COLOR};

pub fn render_order_result(f: &mut ratatui::Frame, result: &OrderResult) {
    let area: Rect = f.area();
    let popup_width = 70;
    let popup_height = match result {
        OrderResult::Success { .. } => 18,
        OrderResult::Error(_) => 8,
    };
    let popup_x = area.x + (area.width - popup_width) / 2;
    let popup_y = area.y + (area.height - popup_height) / 2;
    let popup = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    match result {
        OrderResult::Success {
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
        } => {
            let block = Block::default()
                .title("✅ Order Created Successfully")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Green));
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
                "Press ESC to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Left);
            f.render_widget(paragraph, popup);
        }
        OrderResult::Error(error_msg) => {
            let block = Block::default()
                .title("❌ Order Failed")
                .borders(Borders::ALL)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));
            f.render_widget(block, popup);

            // Wrap error message if too long
            let error_lines: Vec<Line> = error_msg
                .chars()
                .collect::<Vec<_>>()
                .chunks(popup_width as usize - 4)
                .map(|chunk| Line::from(chunk.iter().collect::<String>()))
                .collect();

            let mut lines = vec![];
            for line in error_lines {
                lines.push(line);
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                "Press ESC to close",
                Style::default().fg(Color::DarkGray),
            )]));

            let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Left);
            f.render_widget(paragraph, popup);
        }
    }
}
