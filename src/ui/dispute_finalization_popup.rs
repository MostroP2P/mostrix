use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Render the dispute finalization popup with full dispute details and action buttons
pub fn render_finalization_popup(
    f: &mut ratatui::Frame,
    app: &AppState,
    dispute_id: &uuid::Uuid,
    selected_button: usize,
) {
    // Find the dispute by ID
    let dispute = app
        .admin_disputes_in_progress
        .iter()
        .find(|d| d.id == dispute_id.to_string());

    let Some(selected_dispute) = dispute else {
        // If dispute not found, show error
        let area = f.area();
        let popup_width = area.width.saturating_sub(area.width / 4);
        let popup_height = 10;
        let popup = center_rect(area, popup_width, popup_height);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title("Error")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));
        f.render_widget(block, popup);
        return;
    };

    let area = f.area();
    // Large popup (80% width, 70% height) to show all details
    let popup_width = area.width.saturating_mul(8).saturating_div(10);
    let popup_height = area.height.saturating_mul(7).saturating_div(10);
    let popup = center_rect(area, popup_width, popup_height);

    // Clear the popup area to make it fully opaque
    f.render_widget(Clear, popup);

    // Main block
    let block = Block::default()
        .title("⚖️ Finalize Dispute")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    f.render_widget(block, popup);

    // Inner layout for content
    let inner_area = Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(0),    // Content area
            Constraint::Length(3), // Buttons area
        ],
    )
    .split(inner_area);

    // Content area - scrollable details
    render_dispute_details(f, chunks[0], selected_dispute);

    // Buttons area
    render_action_buttons(f, chunks[1], selected_button);
}

/// Render detailed dispute information
fn render_dispute_details(
    f: &mut ratatui::Frame,
    area: Rect,
    dispute: &crate::models::AdminDispute,
) {
    // Truncate pubkeys for display
    let truncate_pubkey = |pubkey: &str| -> String {
        if pubkey.len() > 16 {
            format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
        } else {
            pubkey.to_string()
        }
    };

    let buyer_pubkey = dispute
        .buyer_pubkey
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("Unknown");
    let seller_pubkey = dispute
        .seller_pubkey
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("Unknown");

    let is_initiator_buyer = &dispute.initiator_pubkey == buyer_pubkey;
    let buyer_pubkey_display = truncate_pubkey(buyer_pubkey);
    let seller_pubkey_display = truncate_pubkey(seller_pubkey);

    // Format timestamps
    let created_date = DateTime::from_timestamp(dispute.created_at, 0);
    let created_str = created_date
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let taken_date = DateTime::from_timestamp(dispute.taken_at, 0);
    let taken_str = taken_date
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    // Privacy indicators
    let buyer_privacy = if dispute.initiator_full_privacy && is_initiator_buyer
        || dispute.counterpart_full_privacy && !is_initiator_buyer
    {
        "🕶️ Private"
    } else {
        "👁️ Public"
    };
    let seller_privacy = if dispute.initiator_full_privacy && !is_initiator_buyer
        || dispute.counterpart_full_privacy && is_initiator_buyer
    {
        "🕶️ Private"
    } else {
        "👁️ Public"
    };

    // Rating information
    let (buyer_rating, seller_rating) = if is_initiator_buyer {
        let buyer_rating = if let Some(ref info) = dispute.initiator_info_data {
            format!(
                "⭐ {:.1}/10 ({} trades, {} days)",
                info.rating, info.reviews, info.operating_days
            )
        } else {
            "No rating".to_string()
        };
        let seller_rating = if let Some(ref info) = dispute.counterpart_info_data {
            format!(
                "⭐ {:.1}/10 ({} trades, {} days)",
                info.rating, info.reviews, info.operating_days
            )
        } else {
            "No rating".to_string()
        };
        (buyer_rating, seller_rating)
    } else {
        let seller_rating = if let Some(ref info) = dispute.initiator_info_data {
            format!(
                "⭐ {:.1}/10 ({} trades, {} days)",
                info.rating, info.reviews, info.operating_days
            )
        } else {
            "No rating".to_string()
        };
        let buyer_rating = if let Some(ref info) = dispute.counterpart_info_data {
            format!(
                "⭐ {:.1}/10 ({} trades, {} days)",
                info.rating, info.reviews, info.operating_days
            )
        } else {
            "No rating".to_string()
        };
        (buyer_rating, seller_rating)
    };

    // Build the content lines
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Dispute ID: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(&dispute.id, Style::default().fg(PRIMARY_COLOR)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(dispute.kind.as_deref().unwrap_or("Unknown")),
            Span::raw("  |  "),
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(dispute.status.as_deref().unwrap_or("Unknown")),
        ]),
        Line::from(vec![
            Span::styled("Created: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(created_str),
            Span::raw("  |  "),
            Span::styled("Taken: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(taken_str),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "━━━ PARTIES ━━━",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(PRIMARY_COLOR),
        )]),
        Line::from(""),
    ];

    // Buyer info
    let buyer_role_str = if is_initiator_buyer {
        "🟢 BUYER (Initiator)"
    } else {
        "🟢 BUYER"
    };
    lines.push(Line::from(vec![Span::styled(
        buyer_role_str,
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![
        Span::raw("  Pubkey: "),
        Span::styled(&buyer_pubkey_display, Style::default().fg(PRIMARY_COLOR)),
        Span::raw("  |  Privacy: "),
        Span::raw(buyer_privacy),
    ]));
    lines.push(Line::from(vec![Span::raw("  "), Span::raw(&buyer_rating)]));
    lines.push(Line::from(""));

    // Seller info
    let seller_role_str = if !is_initiator_buyer {
        "🔴 SELLER (Initiator)"
    } else {
        "🔴 SELLER"
    };
    lines.push(Line::from(vec![Span::styled(
        seller_role_str,
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![
        Span::raw("  Pubkey: "),
        Span::styled(&seller_pubkey_display, Style::default().fg(PRIMARY_COLOR)),
        Span::raw("  |  Privacy: "),
        Span::raw(seller_privacy),
    ]));
    lines.push(Line::from(vec![Span::raw("  "), Span::raw(&seller_rating)]));
    lines.push(Line::from(""));

    // Financial info
    lines.push(Line::from(vec![Span::styled(
        "━━━ FINANCIAL ━━━",
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(PRIMARY_COLOR),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Amount: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{} sats", dispute.amount),
            Style::default().fg(Color::Yellow),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled(
            "Fiat Amount: ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{}", dispute.fiat_amount)),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Premium: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("{}%", dispute.premium)),
    ]));

    if !dispute.payment_method.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "Payment Method: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(&dispute.payment_method),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Render the three action buttons at the bottom
fn render_action_buttons(f: &mut ratatui::Frame, area: Rect, selected_button: usize) {
    // Create three equal-width buttons side by side
    let button_chunks = Layout::new(
        Direction::Horizontal,
        [
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ],
    )
    .split(area);

    // Button 0: Pay Buyer (Full)
    let pay_buyer_style = if selected_button == 0 {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let pay_buyer_block = Block::default()
        .title("Pay Buyer (Full)")
        .borders(Borders::ALL)
        .style(pay_buyer_style);
    let pay_buyer_text = Paragraph::new("AdminSettle")
        .alignment(ratatui::layout::Alignment::Center)
        .style(pay_buyer_style);
    f.render_widget(pay_buyer_block, button_chunks[0]);
    let inner = button_chunks[0].inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    f.render_widget(pay_buyer_text, inner);

    // Button 1: Refund Seller (Full)
    let refund_seller_style = if selected_button == 1 {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };
    let refund_seller_block = Block::default()
        .title("Refund Seller (Full)")
        .borders(Borders::ALL)
        .style(refund_seller_style);
    let refund_seller_text = Paragraph::new("AdminCancel")
        .alignment(ratatui::layout::Alignment::Center)
        .style(refund_seller_style);
    f.render_widget(refund_seller_block, button_chunks[1]);
    let inner = button_chunks[1].inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    f.render_widget(refund_seller_text, inner);

    // Button 2: Exit
    let exit_style = if selected_button == 2 {
        Style::default()
            .bg(Color::Gray)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let exit_block = Block::default()
        .title("Exit")
        .borders(Borders::ALL)
        .style(exit_style);
    let exit_text = Paragraph::new("No Action")
        .alignment(ratatui::layout::Alignment::Center)
        .style(exit_style);
    f.render_widget(exit_block, button_chunks[2]);
    let inner = button_chunks[2].inner(ratatui::layout::Margin {
        vertical: 1,
        horizontal: 1,
    });
    f.render_widget(exit_text, inner);
}

/// Helper function to center a rect
fn center_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(popup);
    popup
}
