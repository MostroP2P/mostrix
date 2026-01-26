use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::{
    apply_status_color, AdminMode, AppState, DisputeFilter, UiMode, BACKGROUND_COLOR, PRIMARY_COLOR,
};
use mostro_core::prelude::*;
use std::str::FromStr;

/// Filter disputes based on the current filter state
fn get_filtered_disputes(app: &AppState) -> Vec<(usize, &crate::models::AdminDispute)> {
    app.admin_disputes_in_progress
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            let status = d
                .status
                .as_deref()
                .and_then(|s| DisputeStatus::from_str(s).ok());
            match app.dispute_filter {
                DisputeFilter::InProgress => status == Some(DisputeStatus::InProgress),
                DisputeFilter::Finalized => matches!(
                    status,
                    Some(DisputeStatus::Settled)
                        | Some(DisputeStatus::SellerRefunded)
                        | Some(DisputeStatus::Released)
                ),
            }
        })
        .collect()
}

/// Render the "Disputes in Progress" tab for admin mode
/// This shows a sidebar with active disputes and a detailed view with chat interface
/// Can filter between InProgress and Finalized disputes
pub fn render_disputes_in_progress(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(20), Constraint::Percentage(80)],
    )
    .split(area);

    let sidebar_area = chunks[0];
    let main_area = chunks[1];

    // Filter disputes based on current filter
    let filtered_disputes = get_filtered_disputes(app);

    // Ensure selected index is within bounds of filtered list
    // Use a local variable to avoid borrow checker issues
    let valid_selected_idx = if filtered_disputes.is_empty() {
        0
    } else {
        app.selected_in_progress_idx
            .min(filtered_disputes.len().saturating_sub(1))
    };

    // 1. Sidebar - Dispute List
    let sidebar_title = match app.dispute_filter {
        DisputeFilter::InProgress => "Disputes In Progress",
        DisputeFilter::Finalized => "Disputes Finalized",
    };
    let disputes_block = Block::default()
        .title(sidebar_title)
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    if filtered_disputes.is_empty() {
        let empty_msg = match app.dispute_filter {
            DisputeFilter::InProgress => "No disputes in progress",
            DisputeFilter::Finalized => "No finalized disputes",
        };
        let empty_paragraph = Paragraph::new(empty_msg)
            .block(disputes_block)
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty_paragraph, sidebar_area);
    } else {
        let items: Vec<ListItem> = filtered_disputes
            .iter()
            .enumerate()
            .map(|(display_idx, (_original_idx, d))| {
                let style = if display_idx == valid_selected_idx {
                    Style::default().bg(PRIMARY_COLOR).fg(Color::Black)
                } else {
                    Style::default().fg(Color::White)
                };
                // Show dispute_id
                let display_id = &d.dispute_id;
                let truncated_id = if display_id.len() > 20 {
                    format!("{}...", &display_id[..20])
                } else {
                    display_id.to_string()
                };
                ListItem::new(Line::from(vec![Span::styled(
                    format!("Dispute ID: {}", truncated_id),
                    style,
                )]))
            })
            .collect();

        let list = List::new(items).block(disputes_block);
        f.render_widget(list, sidebar_area);
    }

    // 2. Main Area
    if let Some((_original_idx, selected_dispute)) = filtered_disputes.get(valid_selected_idx) {
        // Determine layout based on filter state
        let is_finalized = app.dispute_filter == DisputeFilter::Finalized;

        let main_chunks = if is_finalized {
            // For finalized disputes: no chat/input, just expanded header and footer
            Layout::new(
                Direction::Vertical,
                [
                    Constraint::Min(0),    // Expanded header (takes remaining space)
                    Constraint::Length(1), // Footer
                ],
            )
            .split(main_area)
        } else {
            // For in-progress disputes: calculate input height and show chat/input
            let available_width = main_area.width.saturating_sub(4).max(1) as usize;

            // Calculate how many lines we need using simplified word-wrapping
            let input_lines = if app.admin_chat_input.is_empty() {
                1 // Empty input = 1 line minimum
            } else {
                let mut lines = 0;
                let mut current_width = 0;

                // Process each word (ratatui's wrap with trim: true splits on whitespace)
                for word in app.admin_chat_input.split_whitespace() {
                    // Use ratatui's Span to get Unicode-aware width
                    let word_span = Span::raw(word);
                    let word_width = word_span.width();
                    let space_width = if current_width > 0 { 1 } else { 0 }; // Space before word

                    if current_width + space_width + word_width > available_width {
                        // Word doesn't fit, wrap to next line
                        lines += 1;
                        current_width = word_width;
                    } else {
                        // Word fits on current line
                        current_width += space_width + word_width;
                    }
                }

                // Add final line if there's content
                if current_width > 0 {
                    lines += 1;
                }

                lines.max(1) // At least 1 line
            };

            // Cap at reasonable maximum (e.g., 10 lines) and add 2 for borders
            let input_height = (input_lines.min(10) as u16) + 2;

            Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(8),            // Header (expanded for ratings)
                    Constraint::Length(3),            // Party Tabs
                    Constraint::Min(0),               // Chat
                    Constraint::Length(input_height), // Input (dynamic!)
                    Constraint::Length(1),            // Footer
                ],
            )
            .split(main_area)
        };

        // Header - Enhanced with more dispute information
        let created_date = DateTime::from_timestamp(selected_dispute.created_at, 0);
        let created_str = created_date
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        // Get buyer and seller pubkeys (do not default to initiator_pubkey)
        let buyer_pubkey = selected_dispute.buyer_pubkey.as_deref();
        let seller_pubkey = selected_dispute.seller_pubkey.as_deref();

        // Check who initiated the dispute - only compute when both initiator_pubkey and buyer_pubkey are present
        let is_initiator_buyer = buyer_pubkey.map(|bp| selected_dispute.initiator_pubkey == *bp);

        // Truncate pubkeys for display
        let truncate_pubkey = |pubkey: &str| -> String {
            if pubkey.len() > 16 {
                format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
            } else {
                pubkey.to_string()
            }
        };

        let buyer_pubkey_display = buyer_pubkey
            .map(truncate_pubkey)
            .unwrap_or_else(|| "Unknown".to_string());
        let seller_pubkey_display = seller_pubkey
            .map(truncate_pubkey)
            .unwrap_or_else(|| "Unknown".to_string());

        // Determine which party to show in header (the one who initiated the dispute)
        let (initiator_role, initiator_pubkey_display) = match is_initiator_buyer {
            Some(true) => ("Buyer", buyer_pubkey_display.clone()),
            Some(false) => ("Seller", seller_pubkey_display.clone()),
            None => {
                // If we can't determine, show initiator directly
                let initiator_display = truncate_pubkey(&selected_dispute.initiator_pubkey);
                ("Initiator", initiator_display)
            }
        };

        // Privacy indicators (Yes = private mode enabled, No = public mode)
        // Show "Unknown" when is_initiator_buyer is None
        let buyer_privacy_text = match is_initiator_buyer {
            Some(true) => {
                if selected_dispute.initiator_full_privacy {
                    "Yes"
                } else {
                    "No"
                }
            }
            Some(false) => {
                if selected_dispute.counterpart_full_privacy {
                    "Yes"
                } else {
                    "No"
                }
            }
            None => "Unknown",
        };
        let seller_privacy_text = match is_initiator_buyer {
            Some(false) => {
                if selected_dispute.initiator_full_privacy {
                    "Yes"
                } else {
                    "No"
                }
            }
            Some(true) => {
                if selected_dispute.counterpart_full_privacy {
                    "Yes"
                } else {
                    "No"
                }
            }
            None => "Unknown",
        };

        // Labels for privacy line (will be displayed with "Privacy: " prefix)
        let (buyer_label, seller_label) = (
            format!("Buyer - {}", buyer_privacy_text),
            format!("Seller - {}", seller_privacy_text),
        );

        // Format rating information (map to buyer/seller based on who initiated)
        // Show "Unknown" when is_initiator_buyer is None
        let (buyer_rating, seller_rating) = match is_initiator_buyer {
            Some(true) => {
                // Initiator is buyer, counterpart is seller
                let buyer_rating = if let Some(ref info) = selected_dispute.initiator_info_data {
                    let star_count = (info.rating.round() as usize).min(5);
                    let stars = "⭐".repeat(star_count);
                    format!(
                        "{} {:.1}/5 ({} trades completed, {} days)",
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
            }
            Some(false) => {
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
            }
            None => {
                // Cannot determine roles, show Unknown
                ("Unknown".to_string(), "Unknown".to_string())
            }
        };

        // Format additional timestamps for finalized disputes
        let taken_date = DateTime::from_timestamp(selected_dispute.taken_at, 0);
        let taken_str = taken_date
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        // Build header lines - expand for finalized disputes
        let mut header_lines = vec![
            Line::from(vec![
                Span::styled("Order ID: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    &selected_dispute.id,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Dispute ID: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    &selected_dispute.dispute_id,
                    Style::default()
                        .fg(Color::Cyan)
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
                        selected_dispute.fiat_amount, selected_dispute.fiat_code
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

        // Add additional information for finalized disputes
        if is_finalized {
            header_lines.push(Line::from(""));
            header_lines.push(Line::from(vec![Span::styled(
                "━━━ FINALIZATION DETAILS ━━━",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(PRIMARY_COLOR),
            )]));
            header_lines.push(Line::from(""));
            header_lines.push(Line::from(vec![
                Span::styled("Taken At: ", Style::default().fg(Color::Gray)),
                Span::styled(&taken_str, Style::default().fg(Color::Yellow)),
            ]));
            header_lines.push(Line::from(vec![
                Span::styled("Payment Method: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    &selected_dispute.payment_method,
                    Style::default().fg(Color::White),
                ),
            ]));
            header_lines.push(Line::from(vec![
                Span::styled("Premium: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{}%", selected_dispute.premium),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("  |  "),
                Span::styled("Fee: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} sats", selected_dispute.fee),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("  |  "),
                Span::styled("Routing Fee: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{} sats", selected_dispute.routing_fee),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            if let Some(ref order_previous_status) = selected_dispute.order_previous_status {
                header_lines.push(Line::from(vec![
                    Span::styled("Previous Status: ", Style::default().fg(Color::Gray)),
                    Span::styled(order_previous_status, Style::default().fg(Color::White)),
                ]));
            }
            if let Some(ref buyer_invoice) = selected_dispute.buyer_invoice {
                if !buyer_invoice.is_empty() {
                    let invoice_display: String = if buyer_invoice.len() > 50 {
                        format!("{}...", &buyer_invoice[..50])
                    } else {
                        buyer_invoice.clone()
                    };
                    header_lines.push(Line::from(vec![
                        Span::styled("Buyer Invoice: ", Style::default().fg(Color::Gray)),
                        Span::styled(invoice_display, Style::default().fg(Color::Cyan)),
                    ]));
                }
            }
        }

        let header_title = if is_finalized {
            "📋 Finalized Dispute Info"
        } else {
            "📋 Dispute Info"
        };

        let header = Paragraph::new(header_lines)
            .block(
                Block::default()
                    .title(Span::styled(
                        header_title,
                        Style::default()
                            .fg(PRIMARY_COLOR)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            )
            .alignment(ratatui::layout::Alignment::Left);
        f.render_widget(header, main_chunks[0]);

        // Only show party tabs, chat, and input for in-progress disputes
        if !is_finalized {
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

            // Chat History - Display chat messages for this dispute using List widget
            let dispute_id_key = &selected_dispute.dispute_id;
            let chat_messages = app.admin_dispute_chats.get(dispute_id_key);

            let items = if let Some(messages) = chat_messages {
                super::helpers::build_chat_list_items(messages, app.active_chat_party)
            } else {
                super::helpers::build_chat_list_items(&[], app.active_chat_party)
            };

            // Update ListState to show latest message if we're at the bottom or selection is invalid
            let total_items = items.len();
            if total_items > 0 {
                let current_selection = app.admin_chat_list_state.selected();
                // Reset to bottom if: no selection, selection is out of bounds, or selection is at the end
                if current_selection.is_none_or(|sel| sel >= total_items.saturating_sub(1)) {
                    app.admin_chat_list_state
                        .select(Some(total_items.saturating_sub(1)));
                }
            } else {
                // No items, clear selection
                app.admin_chat_list_state.select(None);
            }

            let chat_list = List::new(items)
                .block(
                    Block::default()
                        .title(format!(
                            "Chat with {} ({})",
                            app.active_chat_party,
                            if total_items > 0 {
                                format!("{} messages", total_items)
                            } else {
                                "no messages".to_string()
                            }
                        ))
                        .borders(Borders::ALL),
                )
                .style(Style::default());

            f.render_stateful_widget(chat_list, main_chunks[2], &mut app.admin_chat_list_state);

            // Render scrollbar on the right side of the chat area
            super::helpers::render_chat_scrollbar(
                f,
                main_chunks[2],
                total_items,
                &app.admin_chat_list_state,
            );

            // Input Area
            // Check if we're in ManagingDispute mode (input is active)
            let is_input_focused =
                matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute));
            let is_input_enabled = app.admin_chat_input_enabled;

            let input_style = if is_input_focused && is_input_enabled {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let input_title = if is_input_focused && is_input_enabled {
                "💬 Message (typing enabled)"
            } else if is_input_focused && !is_input_enabled {
                "💬 Message (disabled - Shift+I to enable)"
            } else {
                "Message"
            };

            let input_border_style = if is_input_focused && is_input_enabled {
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
        }

        // Footer
        let filter_hint = match app.dispute_filter {
            DisputeFilter::InProgress => "Shift+C: View Finalized",
            DisputeFilter::Finalized => "Shift+C: View In Progress",
        };
        let footer_text = if is_finalized {
            // Simplified footer for finalized disputes
            format!("{} | ↑↓: Select Dispute", filter_hint)
        } else {
            // Full footer for in-progress disputes
            let is_input_focused =
                matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute));
            if is_input_focused {
                let is_input_enabled = app.admin_chat_input_enabled;
                if is_input_enabled {
                    format!("Tab: Switch Party | Enter: Send | Shift+I: Disable | Shift+F: Resolve | {} | PgUp/PgDn: Scroll | End: Bottom | ↑↓: Select Dispute", filter_hint)
                } else {
                    format!("Tab: Switch Party | Shift+I: Enable | Shift+F: Resolve | {} | PgUp/PgDn: Scroll | ↑↓: Navigate Chat | End: Bottom | ↑↓: Select Dispute", filter_hint)
                }
            } else {
                format!("Tab: Switch Party | Shift+F: Resolve | {} | ↑↓: Select Dispute | PgUp/PgDn: Scroll Chat | End: Bottom", filter_hint)
            }
        };
        let footer = Paragraph::new(footer_text);
        let footer_chunk_idx = if is_finalized { 1 } else { 4 };
        f.render_widget(footer, main_chunks[footer_chunk_idx]);

        // Update the selected index after rendering is complete (to avoid borrow checker issues)
        app.selected_in_progress_idx = valid_selected_idx;
    } else {
        let no_selection = Paragraph::new("Select a dispute from the sidebar")
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(no_selection, main_area);
        // Reset index when no disputes are available
        app.selected_in_progress_idx = 0;
    }
}
