use chrono::DateTime;
use mostro_core::prelude::*;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::{apply_status_color, AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Render the "Disputes in Progress" tab for admin mode
/// This shows a sidebar with active disputes and a detailed view with chat interface
pub fn render_disputes_in_progress(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(20), Constraint::Percentage(80)],
    )
    .split(area);

    let sidebar_area = chunks[0];
    let main_area = chunks[1];

    // 1. Sidebar - Dispute List
    let disputes_block = Block::default()
        .title("Disputes in Progress")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    if app.admin_disputes_in_progress.is_empty() {
        let empty_msg = Paragraph::new("No disputes in progress")
            .block(disputes_block)
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty_msg, sidebar_area);
    } else {
        let items: Vec<ListItem> = app
            .admin_disputes_in_progress
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let style = if i == app.selected_in_progress_idx {
                    Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![Span::styled(
                    format!("ID: {}...", &d.id[..20]),
                    style,
                )]))
            })
            .collect();

        let list = List::new(items).block(disputes_block);
        f.render_widget(list, sidebar_area);
    }

    // 2. Main Area
    if let Some(selected_dispute) = app
        .admin_disputes_in_progress
        .get(app.selected_in_progress_idx)
    {
        let main_chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(8), // Header (expanded for ratings)
                Constraint::Length(3), // Party Tabs
                Constraint::Min(0),    // Chat
                Constraint::Length(3), // Input
                Constraint::Length(1), // Footer
            ],
        )
        .split(main_area);

        // Header - Enhanced with more dispute information
        let created_date = DateTime::from_timestamp(selected_dispute.created_at, 0);
        let created_str = created_date
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        // Determine who is buyer and who is seller by comparing pubkeys
        let buyer_pubkey = selected_dispute
            .buyer_pubkey
            .as_ref()
            .unwrap_or(&selected_dispute.initiator_pubkey);
        let seller_pubkey = selected_dispute
            .seller_pubkey
            .as_ref()
            .unwrap_or(&selected_dispute.initiator_pubkey);

        // Check who initiated the dispute
        let is_initiator_buyer = &selected_dispute.initiator_pubkey == buyer_pubkey;

        // Truncate pubkeys for display
        let truncate_pubkey = |pubkey: &str| -> String {
            if pubkey.len() > 16 {
                format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
            } else {
                pubkey.to_string()
            }
        };

        let buyer_pubkey_display = truncate_pubkey(buyer_pubkey);
        let seller_pubkey_display = truncate_pubkey(seller_pubkey);

        // Determine which party to show in header (the one who initiated the dispute)
        let (initiator_role, initiator_pubkey_display) = if is_initiator_buyer {
            ("Buyer", buyer_pubkey_display.clone())
        } else {
            ("Seller", seller_pubkey_display.clone())
        };

        // Privacy indicators (🕶️ = private/anonymous, 👁️ = public/visible)
        let buyer_privacy_icon = if selected_dispute.initiator_full_privacy && is_initiator_buyer
            || selected_dispute.counterpart_full_privacy && !is_initiator_buyer
        {
            "🕶️"
        } else {
            "👁️"
        };
        let seller_privacy_icon = if selected_dispute.initiator_full_privacy && !is_initiator_buyer
            || selected_dispute.counterpart_full_privacy && is_initiator_buyer
        {
            "🕶️"
        } else {
            "👁️"
        };

        // Labels for privacy line
        let (buyer_label, seller_label) = (
            format!("{} Buyer", buyer_privacy_icon),
            format!("{} Seller", seller_privacy_icon),
        );

        // Format rating information (map to buyer/seller based on who initiated)
        let (buyer_rating, seller_rating) = if is_initiator_buyer {
            // Initiator is buyer, counterpart is seller
            let buyer_rating = if let Some(ref info) = selected_dispute.initiator_info_data {
                let stars = "⭐"
                    .repeat((info.rating / 2.0).round() as usize)
                    .chars()
                    .take(5)
                    .collect::<String>();
                format!(
                    "{} {:.1}/10 ({} trades completed, {} days)",
                    stars, info.rating, info.reviews, info.operating_days
                )
            } else {
                "No rating available".to_string()
            };
            let seller_rating = if let Some(ref info) = selected_dispute.counterpart_info_data {
                let stars = "⭐"
                    .repeat((info.rating / 2.0).round() as usize)
                    .chars()
                    .take(5)
                    .collect::<String>();
                format!(
                    "{} {:.1}/10 ({} trades completed, {} days)",
                    stars, info.rating, info.reviews, info.operating_days
                )
            } else {
                "No rating available".to_string()
            };
            (buyer_rating, seller_rating)
        } else {
            // Initiator is seller, counterpart is buyer
            let seller_rating = if let Some(ref info) = selected_dispute.initiator_info_data {
                let stars = "⭐"
                    .repeat((info.rating / 2.0).round() as usize)
                    .chars()
                    .take(5)
                    .collect::<String>();
                format!(
                    "{} {:.1}/10 ({} trades completed, {} days)",
                    stars, info.rating, info.reviews, info.operating_days
                )
            } else {
                "No rating available".to_string()
            };
            let buyer_rating = if let Some(ref info) = selected_dispute.counterpart_info_data {
                let stars = "⭐"
                    .repeat((info.rating / 2.0).round() as usize)
                    .chars()
                    .take(5)
                    .collect::<String>();
                format!(
                    "{} {:.1}/10 ({} trades completed, {} days)",
                    stars, info.rating, info.reviews, info.operating_days
                )
            } else {
                "No rating available".to_string()
            };
            (buyer_rating, seller_rating)
        };

        let header_lines = vec![
            Line::from(vec![
                Span::styled("ID: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    &selected_dispute.id,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Type: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    selected_dispute.kind.as_deref().unwrap_or("Unknown"),
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Status: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    selected_dispute.status.as_deref().unwrap_or("Unknown"),
                    apply_status_color(selected_dispute.status.as_deref().unwrap_or("Unknown"))
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("Initiator: {} ", initiator_role),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(&initiator_pubkey_display, Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled("Created: ", Style::default().fg(Color::Gray)),
                Span::styled(&created_str, Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("Amount: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} sats", selected_dispute.amount),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Fiat: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!(
                        "{} {}",
                        selected_dispute.fiat_amount,
                        selected_dispute
                            .payment_method
                            .split(',')
                            .next()
                            .unwrap_or("USD")
                    ),
                    Style::default().fg(Color::Yellow),
                ),
            ]),
            Line::from(vec![
                Span::styled("Privacy: ", Style::default().fg(Color::Gray)),
                Span::styled(&buyer_label, Style::default().fg(Color::White)),
                Span::raw("  |  "),
                Span::styled(&seller_label, Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Buyer Rating: ", Style::default().fg(Color::Gray)),
                Span::styled(&buyer_rating, Style::default().fg(Color::Yellow)),
                Span::raw("  |  "),
                Span::styled("Seller Rating: ", Style::default().fg(Color::Gray)),
                Span::styled(&seller_rating, Style::default().fg(Color::Yellow)),
            ]),
        ];

        let header = Paragraph::new(header_lines)
            .block(
                Block::default()
                    .title(Span::styled(
                        "📋 Dispute Info",
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            )
            .alignment(ratatui::layout::Alignment::Left);
        f.render_widget(header, main_chunks[0]);

        // Party Tabs
        let buyer_style = if app.active_chat_party == super::ChatParty::Buyer {
            Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        let seller_style = if app.active_chat_party == super::ChatParty::Seller {
            Style::default()
                .bg(Color::Red)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };

        let party_tabs_area = main_chunks[1];
        let party_chunks = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .split(party_tabs_area);

        // Create multi-line text for buyer and seller tabs with pubkeys
        let buyer_text = vec![
            Line::from(Span::styled(
                "BUYER",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(&buyer_pubkey_display, Style::default())),
        ];

        let seller_text = vec![
            Line::from(Span::styled(
                "SELLER",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(&seller_pubkey_display, Style::default())),
        ];

        f.render_widget(
            Paragraph::new(buyer_text)
                .block(Block::default().borders(Borders::ALL).style(buyer_style))
                .alignment(ratatui::layout::Alignment::Center),
            party_chunks[0],
        );
        f.render_widget(
            Paragraph::new(seller_text)
                .block(Block::default().borders(Borders::ALL).style(seller_style))
                .alignment(ratatui::layout::Alignment::Center),
            party_chunks[1],
        );

        // Chat History (Filtered)
        let messages_lock = app.messages.lock().unwrap();
        // For admin disputes, we need to filter messages that belong to this dispute ID
        // AND are either from/to the active chat party.
        // NOTE: Admin messages usually have the order_id set.
        let chat_party_pubkey = match app.active_chat_party {
            super::ChatParty::Buyer => selected_dispute.buyer_pubkey.as_ref(),
            super::ChatParty::Seller => selected_dispute.seller_pubkey.as_ref(),
        };

        let filtered_messages: Vec<ListItem> = messages_lock
            .iter()
            .filter(|m| {
                let is_same_order =
                    m.order_id.map(|id| id.to_string()) == Some(selected_dispute.id.clone());
                let _is_correct_party = if let Some(party_pk) = chat_party_pubkey {
                    m.sender.to_string() == *party_pk
                        || m.message.get_inner_message_kind().action == Action::AdminTookDispute
                // Placeholder check
                } else {
                    false
                };
                is_same_order // For now, just show all messages for this order ID
            })
            .map(|m| {
                let sender_label = if m.sender.to_string() == selected_dispute.initiator_pubkey {
                    "Initiator"
                } else {
                    "Other"
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}] ", sender_label),
                        Style::default().fg(PRIMARY_COLOR),
                    ),
                    Span::raw(format!("{:?}", m.message.get_inner_message_kind().action)),
                ]))
            })
            .collect();

        let chat_list = List::new(filtered_messages).block(
            Block::default()
                .title(format!("Chat with {}", app.active_chat_party))
                .borders(Borders::ALL),
        );
        f.render_widget(chat_list, main_chunks[2]);

        // Input Area
        let input = Paragraph::new(app.admin_chat_input.as_str()).block(
            Block::default()
                .title("Message")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(input, main_chunks[3]);

        // Footer
        let footer =
            Paragraph::new("Tab: Switch Party | Enter: Finalize Dispute | ↑↓: Select Dispute");
        f.render_widget(footer, main_chunks[4]);
    } else {
        let no_selection = Paragraph::new("Select a dispute from the sidebar")
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(no_selection, main_area);
    }
}
