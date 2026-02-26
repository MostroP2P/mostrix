use std::str::FromStr;

use chrono::DateTime;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::constants::{
    FILTER_VIEW_FINALIZED, FILTER_VIEW_IN_PROGRESS, FOOTER_CTRL_S_SAVE_FILE, FOOTER_END_BOTTOM,
    FOOTER_ENTER_SEND, FOOTER_NAV_CHAT, FOOTER_PGUP_PGDN_SCROLL, FOOTER_PGUP_PGDN_SCROLL_CHAT,
    FOOTER_SHIFT_F_RESOLVE, FOOTER_SHIFT_I_DISABLE, FOOTER_SHIFT_I_ENABLE, FOOTER_TAB_PARTY,
    FOOTER_TAB_SWITCH_PARTY, FOOTER_UP_DOWN_SELECT, FOOTER_UP_DOWN_SELECT_DISPUTE, HELP_KEY,
};
use crate::ui::helpers::{
    build_chat_scrollview_content, count_visible_attachments, get_selected_chat_message,
};
use crate::ui::{AdminMode, AppState, DisputeFilter, UiMode, BACKGROUND_COLOR, PRIMARY_COLOR};
use mostro_core::prelude::*;

/// Filter disputes based on the current filter state.
/// Returns owned data so the caller can mutate app (e.g. scroll state) in the same block.
fn get_filtered_disputes(app: &AppState) -> Vec<(usize, crate::models::AdminDispute)> {
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
        .map(|(i, d)| (i, d.clone()))
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
    if let Some((_original_idx, ref selected_dispute)) = filtered_disputes.get(valid_selected_idx) {
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
            // Reserve two lines for footer when wide (two-line hints) or when attachment toast is shown
            let use_two_line_footer = main_area.width >= 90;
            let footer_height = if app.attachment_toast.is_some() {
                if use_two_line_footer {
                    3
                } else {
                    2
                }
            } else if use_two_line_footer {
                2
            } else {
                1
            };

            Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(7),             // Header (amount+fiat+privacy on one line)
                    Constraint::Length(3),             // Party Tabs
                    Constraint::Min(0),                // Chat
                    Constraint::Length(input_height),  // Input (dynamic!)
                    Constraint::Length(footer_height), // Footer (2 when toast visible)
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
                let buyer_rating = crate::ui::helpers::format_user_rating(
                    selected_dispute.initiator_info_data.as_ref(),
                );
                let seller_rating = crate::ui::helpers::format_user_rating(
                    selected_dispute.counterpart_info_data.as_ref(),
                );
                (buyer_rating, seller_rating)
            }
            Some(false) => {
                // Initiator is seller, counterpart is buyer
                let seller_rating = crate::ui::helpers::format_user_rating(
                    selected_dispute.initiator_info_data.as_ref(),
                );
                let buyer_rating = crate::ui::helpers::format_user_rating(
                    selected_dispute.counterpart_info_data.as_ref(),
                );
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
                    Style::default().add_modifier(Modifier::BOLD),
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
                Span::raw("  |  "),
                Span::styled("Privacy: ", Style::default().fg(Color::Gray)),
                Span::styled(&buyer_label, Style::default().fg(Color::White)),
                Span::raw("  "),
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
            let buyer_style = if app.active_chat_party == crate::ui::ChatParty::Buyer {
                Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let seller_style = if app.active_chat_party == crate::ui::ChatParty::Seller {
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

            // Chat History - Display chat messages using ScrollView
            let dispute_id_key = &selected_dispute.dispute_id;
            let chat_messages = app.admin_dispute_chats.get(dispute_id_key);
            let chat_area = main_chunks[2];

            // Full inner width (minus borders and scrollbar) so counterpart messages align to the right edge
            let inner_width = Block::default()
                .borders(Borders::ALL)
                .inner(chat_area)
                .width;
            let content_width = inner_width.saturating_sub(1).max(1); // reserve 1 col for scrollbar
            let max_content_width = (content_width / 2).max(1); // wrap long lines at half width for readability

            let file_count = chat_messages
                .map(|msgs| count_visible_attachments(msgs, app.active_chat_party))
                .unwrap_or(0);

            let messages_slice = chat_messages.map(|m| m.as_slice()).unwrap_or(&[]);
            let content = build_chat_scrollview_content(
                messages_slice,
                app.active_chat_party,
                content_width,
                Some(max_content_width),
            );

            let visible_count = content.line_start_per_message.len();
            app.admin_chat_line_starts = content.line_start_per_message.clone();

            if visible_count > 0 {
                let current_key = (dispute_id_key.clone(), app.active_chat_party);
                if let Some((ref d, ref p, last_count)) = app.admin_chat_scroll_tracker {
                    if *d == current_key.0 && *p == current_key.1 && visible_count > last_count {
                        app.admin_chat_scrollview_state.scroll_to_bottom();
                        app.admin_chat_selected_message_idx = Some(visible_count.saturating_sub(1));
                    }
                }
                app.admin_chat_scroll_tracker =
                    Some((dispute_id_key.clone(), app.active_chat_party, visible_count));

                let sel = app.admin_chat_selected_message_idx;
                if sel.is_none_or(|idx| idx >= visible_count.saturating_sub(1)) {
                    app.admin_chat_selected_message_idx = Some(visible_count.saturating_sub(1));
                }
            } else {
                app.admin_chat_selected_message_idx = None;
                app.admin_chat_scroll_tracker =
                    Some((dispute_id_key.clone(), app.active_chat_party, 0));
            }

            let chat_title = if visible_count > 0 {
                if file_count > 0 {
                    format!(
                        "Chat with {} ({} messages, {} file(s))",
                        app.active_chat_party, visible_count, file_count
                    )
                } else {
                    format!(
                        "Chat with {} ({} messages)",
                        app.active_chat_party, visible_count
                    )
                }
            } else {
                format!("Chat with {} (no messages)", app.active_chat_party)
            };

            let chat_block = Block::default()
                .title(chat_title)
                .borders(Borders::ALL)
                .style(Style::default());
            let inner_area = chat_block.inner(chat_area);
            f.render_widget(chat_block, chat_area);

            let mut scroll_view = ScrollView::new(Size::new(
                content.content_width,
                content.content_height.max(1),
            ))
            .vertical_scrollbar_visibility(ScrollbarVisibility::Always);
            let content_rect =
                Rect::new(0, 0, content.content_width, content.content_height.max(1));
            scroll_view.render_widget(
                Paragraph::new(content.lines).wrap(ratatui::widgets::Wrap { trim: true }),
                content_rect,
            );
            f.render_stateful_widget(
                scroll_view,
                inner_area,
                &mut app.admin_chat_scrollview_state,
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

        // Footer (width-aware: minimal on narrow, 1 or 2 lines when wide; always include Ctrl+H)
        let filter_hint = match app.dispute_filter {
            DisputeFilter::InProgress => FILTER_VIEW_FINALIZED,
            DisputeFilter::Finalized => FILTER_VIEW_IN_PROGRESS,
        };
        let has_selected_attachment = !is_finalized
            && get_selected_chat_message(app, &selected_dispute.dispute_id)
                .and_then(|m| m.attachment.as_ref())
                .is_some();
        let ctrl_s_hint = if has_selected_attachment {
            FOOTER_CTRL_S_SAVE_FILE
        } else {
            ""
        };
        let footer_chunk_idx = if is_finalized { 1 } else { 4 };
        let footer_area = main_chunks[footer_chunk_idx];
        let footer_width = footer_area.width;

        // When wide (>=90) and not finalized, use two lines to avoid overflow
        let (footer_line1, footer_line2) = if footer_width < 50 {
            (HELP_KEY.to_string(), None)
        } else if footer_width < 90 {
            let one = if is_finalized {
                format!("{} | {} | {}", HELP_KEY, filter_hint, FOOTER_UP_DOWN_SELECT)
            } else {
                let is_input_focused =
                    matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute));
                let short = if is_input_focused && app.admin_chat_input_enabled {
                    format!(
                        "{} | {} | {} | {} | {}",
                        HELP_KEY,
                        FOOTER_ENTER_SEND,
                        FOOTER_TAB_PARTY,
                        FOOTER_SHIFT_F_RESOLVE,
                        filter_hint
                    )
                } else {
                    format!(
                        "{} | {} | {} | {}",
                        HELP_KEY, FOOTER_TAB_PARTY, FOOTER_SHIFT_F_RESOLVE, filter_hint
                    )
                };
                format!("{}{}", short, ctrl_s_hint)
            };
            (one, None)
        } else if is_finalized {
            (
                format!(
                    "{} | {} | {}",
                    HELP_KEY, filter_hint, FOOTER_UP_DOWN_SELECT_DISPUTE
                ),
                None,
            )
        } else {
            let is_input_focused =
                matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute));
            let (line1, line2) = if is_input_focused {
                let is_input_enabled = app.admin_chat_input_enabled;
                if is_input_enabled {
                    (
                        format!(
                            "{} | {} | {} | {} | {} | {}",
                            HELP_KEY,
                            FOOTER_TAB_SWITCH_PARTY,
                            FOOTER_ENTER_SEND,
                            FOOTER_SHIFT_I_DISABLE,
                            FOOTER_SHIFT_F_RESOLVE,
                            filter_hint
                        ),
                        format!(
                            "{} | {} | {}{}",
                            FOOTER_PGUP_PGDN_SCROLL,
                            FOOTER_END_BOTTOM,
                            FOOTER_UP_DOWN_SELECT_DISPUTE,
                            ctrl_s_hint
                        ),
                    )
                } else {
                    (
                        format!(
                            "{} | {} | {} | {} | {}",
                            HELP_KEY,
                            FOOTER_TAB_SWITCH_PARTY,
                            FOOTER_SHIFT_I_ENABLE,
                            FOOTER_SHIFT_F_RESOLVE,
                            filter_hint
                        ),
                        format!(
                            "{} | {} | {} | {}{}",
                            FOOTER_PGUP_PGDN_SCROLL,
                            FOOTER_NAV_CHAT,
                            FOOTER_END_BOTTOM,
                            FOOTER_UP_DOWN_SELECT_DISPUTE,
                            ctrl_s_hint
                        ),
                    )
                }
            } else {
                (
                    format!(
                        "{} | {} | {} | {} | {}",
                        HELP_KEY,
                        FOOTER_TAB_SWITCH_PARTY,
                        FOOTER_SHIFT_F_RESOLVE,
                        filter_hint,
                        FOOTER_UP_DOWN_SELECT_DISPUTE
                    ),
                    format!(
                        "{} | {}{}",
                        FOOTER_PGUP_PGDN_SCROLL_CHAT, FOOTER_END_BOTTOM, ctrl_s_hint
                    ),
                )
            };
            (line1, Some(line2))
        };

        if !is_finalized && app.attachment_toast.is_some() {
            let n = if footer_line2.is_some() { 3 } else { 2 };
            let chunks = Layout::new(
                Direction::Vertical,
                (0..n).map(|_| Constraint::Length(1)).collect::<Vec<_>>(),
            )
            .split(footer_area);
            let (toast_area, footer_areas) = (chunks[0], &chunks[1..]);
            let (toast_msg, _) = app.attachment_toast.as_ref().unwrap();
            f.render_widget(
                Paragraph::new(toast_msg.as_str()).style(Style::default().fg(Color::Yellow)),
                toast_area,
            );
            f.render_widget(Paragraph::new(footer_line1.as_str()), footer_areas[0]);
            if let Some(ref line2) = footer_line2 {
                f.render_widget(Paragraph::new(line2.as_str()), footer_areas[1]);
            }
        } else if let Some(ref line2) = footer_line2 {
            let footer_chunks = Layout::new(
                Direction::Vertical,
                [Constraint::Length(1), Constraint::Length(1)],
            )
            .split(footer_area);
            f.render_widget(Paragraph::new(footer_line1.as_str()), footer_chunks[0]);
            f.render_widget(Paragraph::new(line2.as_str()), footer_chunks[1]);
        } else {
            f.render_widget(Paragraph::new(footer_line1.as_str()), footer_area);
        }

        // Update the selected index after rendering is complete (to avoid borrow checker issues)
        app.selected_in_progress_idx = valid_selected_idx;
    } else {
        // No disputes available - show empty message with footer
        // Render the outer block first, then content inside it
        let outer_block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(BACKGROUND_COLOR));
        let inner_area = outer_block.inner(main_area);
        f.render_widget(outer_block, main_area);

        // Split the inner area for content and footer
        let inner_chunks = Layout::new(
            Direction::Vertical,
            [
                Constraint::Min(0),    // Content area
                Constraint::Length(1), // Footer
            ],
        )
        .split(inner_area);

        // Render empty message in content area
        let no_selection = Paragraph::new("Select a dispute from the sidebar")
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(no_selection, inner_chunks[0]);

        // Render footer with key hints (width-aware)
        let filter_hint = match app.dispute_filter {
            DisputeFilter::InProgress => FILTER_VIEW_FINALIZED,
            DisputeFilter::Finalized => FILTER_VIEW_IN_PROGRESS,
        };
        let footer_width = inner_chunks[1].width;
        let footer_text = if footer_width < 50 {
            HELP_KEY.to_string()
        } else {
            format!(
                "{} | {} | {}",
                HELP_KEY, filter_hint, FOOTER_UP_DOWN_SELECT_DISPUTE
            )
        };
        let footer = Paragraph::new(footer_text);
        f.render_widget(footer, inner_chunks[1]);

        // Reset index when no disputes are available
        app.selected_in_progress_idx = 0;
    }
}
