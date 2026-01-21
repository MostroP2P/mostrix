use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::{apply_status_color, AdminMode, AppState, UiMode, BACKGROUND_COLOR, PRIMARY_COLOR};

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
        // Calculate dynamic input box height based on text content with proper wrapping
        // Account for borders (2 chars on each side) and some padding
        let available_width = main_area.width.saturating_sub(4).max(1) as usize;
        
        // Calculate how many lines we need - simulate ratatui's wrapping behavior with trim: true
        let input_lines = if app.admin_chat_input.is_empty() {
            1 // Empty input = 1 line minimum
        } else {
            let text = &app.admin_chat_input;
            let mut lines = 0;
            let mut current_pos = 0;
            
            while current_pos < text.len() {
                let remaining = &text[current_pos..];
                
                // Skip leading whitespace (trim behavior)
                let trimmed_remaining = remaining.trim_start();
                if trimmed_remaining.is_empty() {
                    break; // No more non-whitespace content
                }
                
                // Adjust position for skipped whitespace
                let skipped = remaining.len() - trimmed_remaining.len();
                current_pos += skipped;
                
                // Find how much fits on this line
                if trimmed_remaining.len() <= available_width {
                    // Rest of text fits on this line
                    lines += 1;
                    break;
                }
                
                // Find last space within available width
                let chunk = &trimmed_remaining[..available_width.min(trimmed_remaining.len())];
                if let Some(last_space) = chunk.rfind(char::is_whitespace) {
                    // Wrap at last space
                    current_pos += last_space + 1; // +1 to skip the space
                    lines += 1;
                } else {
                    // No space found, hard break at width
                    current_pos += available_width;
                    lines += 1;
                }
            }
            
            lines.max(1) // At least 1 line
        };
        
        // Cap at reasonable maximum (e.g., 10 lines) and add 2 for borders
        let input_height = (input_lines.min(10) as u16) + 2;
        
        let main_chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(8),            // Header (expanded for ratings)
                Constraint::Length(3),            // Party Tabs
                Constraint::Min(0),               // Chat
                Constraint::Length(input_height), // Input (dynamic!)
                Constraint::Length(1),            // Footer
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

        // Privacy indicators (🟢 = info available, 🔴 = no info/private)
        let buyer_privacy_icon = if selected_dispute.initiator_full_privacy && is_initiator_buyer
            || selected_dispute.counterpart_full_privacy && !is_initiator_buyer
        {
            "🔴"
        } else {
            "🟢"
        };
        let seller_privacy_icon = if selected_dispute.initiator_full_privacy && !is_initiator_buyer
            || selected_dispute.counterpart_full_privacy && is_initiator_buyer
        {
            "🔴"
        } else {
            "🟢"
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

        // Chat History - Display chat messages for this dispute
        let dispute_id = &selected_dispute.id;
        let chat_messages = app.admin_dispute_chats.get(dispute_id);
        
        let mut chat_lines: Vec<Line> = Vec::new();
        
        if let Some(messages) = chat_messages {
            // Calculate how many messages can fit in the visible area
            let chat_area_height = main_chunks[2].height.saturating_sub(2) as usize; // Subtract borders
            
            // Get messages with scroll offset (0 = show latest)
            let total_messages = messages.len();
            let start_idx = if total_messages > chat_area_height {
                total_messages.saturating_sub(chat_area_height + app.admin_chat_scroll_offset)
            } else {
                0
            };
            
            let visible_messages = &messages[start_idx..];
            
            for msg in visible_messages {
                let (sender_label, sender_color, alignment_prefix) = match msg.sender {
                    super::ChatSender::Admin => ("Admin", Color::Cyan, "  ▶ "),
                    super::ChatSender::Buyer => {
                        if app.active_chat_party == super::ChatParty::Buyer {
                            ("Buyer", Color::Green, "  ◀ ")
                        } else {
                            continue; // Skip if not active party
                        }
                    }
                    super::ChatSender::Seller => {
                        if app.active_chat_party == super::ChatParty::Seller {
                            ("Seller", Color::Red, "  ◀ ")
                        } else {
                            continue; // Skip if not active party
                        }
                    }
                };
                
                // Format message with sender label and content
                let formatted_message = format!("{}{}: {}", alignment_prefix, sender_label, msg.content);
                
                chat_lines.push(Line::from(vec![
                    Span::styled(formatted_message, Style::default().fg(sender_color)),
                ]));
                
                // Add empty line for spacing
                chat_lines.push(Line::from(""));
            }
        } else {
            // No messages yet
            chat_lines.push(Line::from(Span::styled(
                "No messages yet. Start the conversation!",
                Style::default().fg(Color::Gray),
            )));
        }

        let chat_paragraph = ratatui::widgets::Paragraph::new(chat_lines).block(
            Block::default()
                .title(format!("Chat with {}", app.active_chat_party))
                .borders(Borders::ALL),
        );
        f.render_widget(chat_paragraph, main_chunks[2]);

        // Input Area
        // Check if we're in ManagingDispute mode (input is active)
        let is_input_focused = matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute));
        
        let input_style = if is_input_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        
        let input_title = if is_input_focused {
            "💬 Message (typing enabled)"
        } else {
            "Message"
        };
        
        let input_border_style = if is_input_focused {
            Style::default()
                .fg(PRIMARY_COLOR)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        
        let input = Paragraph::new(app.admin_chat_input.as_str())
            .block(
                Block::default()
                    .title(input_title)
                    .borders(Borders::ALL)
                    .border_style(input_border_style)
                    .style(input_style),
            )
            .wrap(ratatui::widgets::Wrap { trim: true }); // Enable text wrapping with trimmed spaces
        f.render_widget(input, main_chunks[3]);

        // Footer
        let footer_text = if is_input_focused {
            "Tab: Switch Party | Enter: Send Message (or Finalize if empty) | PgUp/PgDn: Scroll Chat | ↑↓: Select Dispute"
        } else {
            "Tab: Switch Party | Enter: Finalize Dispute | ↑↓: Select Dispute | PgUp/PgDn: Scroll Chat"
        };
        let footer = Paragraph::new(footer_text);
        f.render_widget(footer, main_chunks[4]);
    } else {
        let no_selection = Paragraph::new("Select a dispute from the sidebar")
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(no_selection, main_area);
    }
}
