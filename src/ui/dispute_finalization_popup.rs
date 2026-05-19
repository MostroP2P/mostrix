use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::dispute_bond_slash_popup;
use super::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::ui::helpers::{format_user_rating, is_dispute_finalized};
use crate::util::order_utils::BondSlashChoice;

/// Render the dispute finalization popup with full dispute details and action buttons
pub fn render_finalization_popup(
    f: &mut ratatui::Frame,
    app: &AppState,
    dispute_id: &uuid::Uuid,
    selected_button: usize,
    bond: BondSlashChoice,
    slash_submenu_open: bool,
    slash_submenu_index: usize,
) {
    let bond_ui_enabled =
        crate::util::mostro_info::instance_bonds_enabled(app.mostro_info.as_ref());
    // Find the dispute by dispute_id (or fallback to order_id for backwards compatibility)
    let dispute = app
        .admin_disputes_in_progress
        .iter()
        .find(|d| d.dispute_id == dispute_id.to_string() || d.id == dispute_id.to_string());

    let Some(selected_dispute) = dispute else {
        // If dispute not found, show error with message
        let area = f.area();
        let popup_width = area.width.saturating_sub(area.width / 4);
        let popup_height = 10;
        let popup = center_rect(area, popup_width, popup_height);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title("❌ Error")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));

        // Calculate inner area (excluding borders)
        let inner = block.inner(popup);
        f.render_widget(block, popup);

        // Create error message with dispute ID
        let error_msg = format!("Dispute not found: {}", dispute_id);

        // Wrap error message if too long (accounting for borders)
        // Ensure wrap_width is at least 1 to avoid panic from chunks(0)
        let wrap_width = inner.width.saturating_sub(2).max(1) as usize;
        let error_lines: Vec<Line> = error_msg
            .chars()
            .collect::<Vec<_>>()
            .chunks(wrap_width)
            .map(|chunk| Line::from(chunk.iter().collect::<String>()))
            .collect();

        let mut lines = vec![];
        lines.push(Line::from(""));
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
        return;
    };

    // Validate required fields
    if selected_dispute.buyer_pubkey.is_none() || selected_dispute.seller_pubkey.is_none() {
        // Show error popup
        let area = f.area();
        let popup_width = area.width.saturating_sub(area.width / 4);
        let popup_height = 12;
        let popup = center_rect(area, popup_width, popup_height);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .title("❌ Data Integrity Error")
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR).fg(Color::Red));

        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let error_msg = format!(
            "The dispute data in the database is incomplete.\n\n\
             Missing required fields: buyer_pubkey or seller_pubkey.\n\n\
             This dispute cannot be finalized. Please contact support or\n\
             check the database entry for dispute ID: {}",
            dispute_id
        );

        // Wrap error message
        let wrap_width = inner.width.saturating_sub(2).max(1) as usize;
        let error_lines: Vec<Line> = error_msg
            .lines()
            .flat_map(|line| {
                line.chars()
                    .collect::<Vec<_>>()
                    .chunks(wrap_width)
                    .map(|chunk| Line::from(chunk.iter().collect::<String>()))
                    .collect::<Vec<_>>()
            })
            .collect();

        let mut lines = vec![Line::from("")];
        lines.extend(error_lines);
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Press ESC or ENTER to close",
            Style::default().fg(Color::DarkGray),
        )]));

        let paragraph = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(paragraph, inner);
        return;
    }

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

    // Buttons area - pass dispute status to check if finalized
    let dispute_is_finalized = is_dispute_finalized(selected_dispute).unwrap_or(false);
    render_action_buttons(
        f,
        chunks[1],
        selected_button,
        bond,
        dispute_is_finalized,
        bond_ui_enabled,
    );

    if bond_ui_enabled && slash_submenu_open {
        dispute_bond_slash_popup::render_bond_slash_overlay(f, popup, slash_submenu_index);
    }
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

    // These should never be None due to validation, but we handle it defensively
    let buyer_pubkey = dispute
        .buyer_pubkey
        .as_deref()
        .expect("buyer_pubkey should be validated before rendering");
    let seller_pubkey = dispute
        .seller_pubkey
        .as_deref()
        .expect("seller_pubkey should be validated before rendering");

    let is_initiator_buyer = dispute.initiator_pubkey == buyer_pubkey;
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

    // Privacy indicators (Yes = private mode enabled, No = public mode)
    let buyer_privacy = if dispute.initiator_full_privacy && is_initiator_buyer
        || dispute.counterpart_full_privacy && !is_initiator_buyer
    {
        "Yes"
    } else {
        "No"
    };
    let seller_privacy = if dispute.initiator_full_privacy && !is_initiator_buyer
        || dispute.counterpart_full_privacy && is_initiator_buyer
    {
        "Yes"
    } else {
        "No"
    };

    // Rating information
    let (buyer_rating, seller_rating) = if is_initiator_buyer {
        let buyer_rating = format_user_rating(dispute.initiator_info_data.as_ref());
        let seller_rating = format_user_rating(dispute.counterpart_info_data.as_ref());
        (buyer_rating, seller_rating)
    } else {
        let seller_rating = format_user_rating(dispute.initiator_info_data.as_ref());
        let buyer_rating = format_user_rating(dispute.counterpart_info_data.as_ref());
        (buyer_rating, seller_rating)
    };

    // Build the content lines
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Order ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(&dispute.id, Style::default().fg(PRIMARY_COLOR)),
        ]),
        Line::from(vec![
            Span::styled(
                "Dispute ID: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(&dispute.dispute_id, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(dispute.kind.as_deref().unwrap_or("Unknown")),
            Span::raw("  |  "),
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                dispute
                    .status
                    .as_deref()
                    .and_then(|s| {
                        use std::str::FromStr;
                        mostro_core::prelude::DisputeStatus::from_str(s).ok()
                    })
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Unknown".to_string()),
            ),
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
        Span::raw(format!("{} {}", dispute.fiat_amount, dispute.fiat_code)),
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

/// Render Pay Buyer, Refund Seller, and Bond slash action buttons at the bottom.
fn render_action_buttons(
    f: &mut ratatui::Frame,
    area: Rect,
    selected_button: usize,
    bond: BondSlashChoice,
    is_finalized: bool,
    bond_ui_enabled: bool,
) {
    let button_chunks = if bond_ui_enabled {
        Layout::new(
            Direction::Horizontal,
            [
                Constraint::Percentage(33),
                Constraint::Percentage(34),
                Constraint::Percentage(33),
            ],
        )
        .split(area)
    } else {
        Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(area)
    };

    // Button 0: Pay buyer (settle in buyer's favor)
    let pay_buyer_style = if is_finalized {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else if selected_button == 0 {
        Style::default()
            .bg(Color::Green)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let pay_buyer_block = Block::default()
        .title("💰 Pay buyer")
        .borders(Borders::ALL)
        .style(pay_buyer_style);
    f.render_widget(pay_buyer_block, button_chunks[0]);
    if is_finalized {
        let inner = button_chunks[0].inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("—")
                .alignment(ratatui::layout::Alignment::Center)
                .style(pay_buyer_style),
            inner,
        );
    }

    // Button 1: Refund seller
    let refund_seller_style = if is_finalized {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else if selected_button == 1 {
        Style::default()
            .bg(Color::Red)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };
    let refund_seller_block = Block::default()
        .title("↩️ Refund seller")
        .borders(Borders::ALL)
        .style(refund_seller_style);
    f.render_widget(refund_seller_block, button_chunks[1]);
    if is_finalized {
        let inner = button_chunks[1].inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(
            Paragraph::new("—")
                .alignment(ratatui::layout::Alignment::Center)
                .style(refund_seller_style),
            inner,
        );
    }

    if bond_ui_enabled {
        let bond_label = bond.label();
        let bond_style = if selected_button == 2 {
            Style::default()
                .bg(PRIMARY_COLOR)
                .fg(BACKGROUND_COLOR)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PRIMARY_COLOR)
        };
        let bond_block = Block::default()
            .title("Bond")
            .borders(Borders::ALL)
            .style(bond_style);
        let bond_text = Paragraph::new(bond_label)
            .alignment(ratatui::layout::Alignment::Center)
            .style(bond_style);
        f.render_widget(bond_block, button_chunks[2]);
        let inner = button_chunks[2].inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });
        f.render_widget(bond_text, inner);
    }
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
