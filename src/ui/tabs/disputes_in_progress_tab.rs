//! Admin disputes-in-progress UI.
//!
//! Shortcut hints use a one-row keycap command bar (`i` / Esc for INSERT /
//! COMMAND on BUYER/SELLER; SERBERO is read-only). Groups that do not fit are
//! skipped so later shortcuts can still appear. Ctrl+K Actions pins the dispute
//! id; letter selects, Enter confirms. Composer drafts are bound to
//! `(dispute_id, party)`. Party, filter, and file hints sit on the chat border.
//! The Ctrl+H help overlay is styled in [`crate::ui::help_popup`].

use std::str::FromStr;

use mostro_core::prelude::DisputeStatus;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph, Wrap,
};
use tui_scrollview::{ScrollView, ScrollbarVisibility};

use crate::ui::constants::SOLVER_DMS_READ_ONLY;
use crate::ui::helpers::{
    build_chat_scrollview_content, count_visible_attachments, dispute_status_color,
    format_local_timestamp, format_user_rating, get_filtered_disputes, get_selected_chat_message,
    render_table_list_scrollbar,
};
use crate::ui::key_handler::chat_copy;
use crate::ui::tabs::solver_dms_view::{render_solver_dms, solver_dms_tab_label};
use crate::ui::ChatParty;
use crate::ui::{AdminMode, AppState, DisputeFilter, UiMode, BACKGROUND_COLOR, PRIMARY_COLOR};

/// Dispute Info line after Status when users closed the dispute themselves.
fn user_closed_resolution_label(status: Option<&str>) -> Option<&'static str> {
    match status.and_then(|s| DisputeStatus::from_str(s).ok()) {
        Some(DisputeStatus::CooperativelyCanceled) => Some("Closed by users (cooperative cancel)"),
        Some(DisputeStatus::Released) => Some("Closed by users (seller released)"),
        _ => None,
    }
}

fn should_auto_scroll_chat(
    tracker: Option<&(String, ChatParty, usize)>,
    dispute_id: &str,
    party: ChatParty,
    visible_count: usize,
) -> bool {
    match tracker {
        None => true,
        Some((tracked_dispute, tracked_party, last_count)) => {
            tracked_dispute != dispute_id || *tracked_party != party || visible_count > *last_count
        }
    }
}

/// Truncate a dispute id for the sidebar label by Unicode scalar values.
///
/// Preserves a `max_chars` prefix and appends `...` when longer. Using
/// `chars()` avoids panics from byte-indexing into multi-byte UTF-8.
fn truncate_dispute_id_label(display_id: &str, max_chars: usize) -> String {
    if display_id.chars().count() > max_chars {
        format!(
            "{}...",
            display_id.chars().take(max_chars).collect::<String>()
        )
    } else {
        display_id.to_string()
    }
}

/// High-contrast keycap groups that drop whole pairs when width is tight.
fn shortcut_bar(width: u16, hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    let mut used = 0;
    for (key, label) in hints {
        let gap = if spans.is_empty() { 0 } else { 2 };
        let key_text = format!(" {key} ");
        let label_text = format!(" {label}");
        let group_width = Span::raw(&key_text).width() + Span::raw(&label_text).width();
        // Skip groups that do not fit; keep trying later groups.
        if used + gap + group_width > usize::from(width) {
            continue;
        }
        let key_style = if spans.is_empty() {
            Style::default().fg(Color::Black).bg(PRIMARY_COLOR)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        if gap > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            key_text,
            key_style.add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(label_text, Style::default().fg(Color::Gray)));
        used += gap + group_width;
    }
    Line::from(spans)
}

/// Mode-aware keycap row for the dispute command bar (aligned with My Trades).
///
/// Finalized disputes show filter/remove/nav; managing disputes switch between
/// INSERT (`Enter` Send / `Esc` Commands) and COMMAND (`i` Write), with
/// Resolve/Recover/Filter/Remove in Ctrl+K Actions. SERBERO is read-only (no
/// Write/Send).
fn dispute_command_bar(
    width: u16,
    input_enabled: bool,
    is_finalized: bool,
    managing: bool,
    serbero: bool,
) -> Line<'static> {
    if is_finalized {
        return shortcut_bar(
            width,
            &[
                ("Ctrl+K", "Actions"),
                ("Ctrl+H", "Help"),
                ("↑↓", "Disputes"),
            ],
        );
    }
    if managing && serbero {
        return shortcut_bar(
            width,
            &[
                ("Tab", "Party"),
                ("Ctrl+K", "Actions"),
                ("Ctrl+H", "Help"),
                ("Ctrl+C", "Copy"),
            ],
        );
    }
    if managing && input_enabled {
        shortcut_bar(
            width,
            &[
                ("Enter", "Send"),
                ("Esc", "Commands"),
                ("Ctrl+K", "Actions"),
                ("Ctrl+H", "Help"),
                ("Ctrl+C", "Copy"),
            ],
        )
    } else if managing {
        shortcut_bar(
            width,
            &[
                ("i", "Write"),
                ("Ctrl+K", "Actions"),
                ("Ctrl+H", "Help"),
                ("Ctrl+C", "Copy"),
            ],
        )
    } else {
        shortcut_bar(
            width,
            &[
                ("Ctrl+K", "Actions"),
                ("Ctrl+H", "Help"),
                ("↑↓", "Disputes"),
            ],
        )
    }
}

/// Copy-mode keycaps; stacks one group per line when the row does not fit.
fn dispute_copy_controls(width: u16) -> Text<'static> {
    let hints = [("↑↓", "Select"), ("Enter", "Copy"), ("Esc", "Cancel")];
    let full = shortcut_bar(u16::MAX, &hints);
    if full.width() <= usize::from(width) {
        Text::from(full)
    } else {
        Text::from(
            hints
                .iter()
                .map(|&(key, label)| {
                    let line = shortcut_bar(width, &[(key, label)]);
                    if line.spans.is_empty() {
                        shortcut_bar(width, &[(key, "")])
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>(),
        )
    }
}

/// Short Shift+C filter target for the chat-border keycap (`Finalized` / `In progress`).
fn filter_hint_label(filter: DisputeFilter) -> &'static str {
    match filter {
        DisputeFilter::InProgress => "Finalized",
        DisputeFilter::Finalized => "In progress",
    }
}

/// Render the Disputes in Progress tab: sidebar list, dispute detail, party chat,
/// and a mode-aware keycap command bar (contextual hints on the chat border).
pub fn render_disputes_in_progress(f: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    chat_copy::validate_selection(app);
    let copy_selection = chat_copy::selected_index(app);
    let copy_range = chat_copy::selected_range(app);
    let copy_feedback = chat_copy::feedback_text(app);
    let copy_context = copy_selection.is_some() || copy_feedback.is_some();
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(20), Constraint::Percentage(80)],
    )
    .split(area);

    let sidebar_area = if copy_context && area.width < 60 {
        Rect::default()
    } else {
        chunks[0]
    };
    let main_area = if copy_context && area.width < 60 {
        area
    } else {
        chunks[1]
    };
    let copy_controls = if let Some(feedback) = copy_feedback {
        Text::styled(feedback, Style::default().fg(PRIMARY_COLOR))
    } else {
        dispute_copy_controls(main_area.width)
    };

    // Filter disputes based on current filter
    let filtered_disputes = get_filtered_disputes(app);

    // Resolve the selected dispute id to its display row in the filtered list
    let valid_selected_idx =
        crate::ui::helpers::selected_display_idx(app, &filtered_disputes).unwrap_or(0);

    // 1. Sidebar - Dispute List
    let sidebar_title = match app.dispute_filter {
        DisputeFilter::InProgress => "Disputes In Progress",
        DisputeFilter::Finalized => "Disputes Finalized",
    };
    let disputes_block = Block::default()
        .title(sidebar_title)
        .title_bottom(shortcut_bar(
            sidebar_area.width.saturating_sub(2),
            &[("↑↓", "Disputes")],
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
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
        // Stateful List keeps the selected row in view when the sidebar overflows
        // (same pattern as Orders / Disputes Pending tables). Scrollbar uses the
        // shared data-row track helper after ListState computes its offset.
        // Reserve: borders (2) + highlight symbol (2) + gap between id and status (2).
        let inner_width = sidebar_area.width.saturating_sub(6) as usize;
        let items: Vec<ListItem> = filtered_disputes
            .iter()
            .map(|(_original_idx, d)| {
                let status = d.status.as_deref().unwrap_or("unknown");
                let id_budget = inner_width
                    .saturating_sub(status.chars().count())
                    .clamp(4, 16);
                let truncated_id = truncate_dispute_id_label(&d.dispute_id, id_budget);
                ListItem::new(Line::from(Span::styled(
                    format!("{truncated_id}  {status}"),
                    Style::default().fg(dispute_status_color(d.status.as_deref())),
                )))
            })
            .collect();

        let list = List::new(items)
            .block(disputes_block)
            .highlight_style(Style::default().bg(PRIMARY_COLOR).fg(Color::Black))
            .highlight_symbol("▶ ")
            .highlight_spacing(HighlightSpacing::Always);

        let mut list_state = ListState::default().with_selected(Some(valid_selected_idx));
        f.render_stateful_widget(list, sidebar_area, &mut list_state);

        let visible_rows = sidebar_area.height.saturating_sub(2) as usize;
        render_table_list_scrollbar(
            f,
            sidebar_area,
            filtered_disputes.len(),
            visible_rows,
            0,
            list_state.offset(),
        );
    }

    // 2. Main Area
    if let Some((_original_idx, ref selected_dispute)) = filtered_disputes.get(valid_selected_idx) {
        // Determine layout based on filter state
        let is_finalized = selected_dispute.is_finalized();

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
            let mut input_height = (input_lines.min(10) as u16) + 2;
            // One keycap command row (+ toast). Contextual hints live on the chat border.
            let toast_extra = u16::from(app.attachment_toast.is_some());
            let mut footer_height = 1u16.saturating_add(toast_extra);

            let mut header_height = 7;
            let mut party_height = 3;
            if copy_context {
                footer_height = Paragraph::new(copy_controls.clone())
                    .wrap(Wrap { trim: true })
                    .line_count(main_area.width.max(1))
                    .min(3) as u16;
                let spare = main_area.height.saturating_sub(footer_height + 4);
                header_height = if spare >= 10 { 7 } else { 0 };
                party_height = if spare >= 3 { 3 } else { 0 };
                input_height = if spare >= 13 { 3 } else { 0 };
            }

            Layout::new(
                Direction::Vertical,
                [
                    Constraint::Length(header_height),
                    Constraint::Length(party_height),
                    Constraint::Min(0),                // Chat
                    Constraint::Length(input_height),  // Input (dynamic!)
                    Constraint::Length(footer_height), // Command bar (+ toast)
                ],
            )
            .split(main_area)
        };

        // Header - Enhanced with more dispute information
        let created_str = format_local_timestamp(selected_dispute.created_at, "%Y-%m-%d %H:%M:%S")
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
                let buyer_rating =
                    format_user_rating(selected_dispute.initiator_info_data.as_ref());
                let seller_rating =
                    format_user_rating(selected_dispute.counterpart_info_data.as_ref());
                (buyer_rating, seller_rating)
            }
            Some(false) => {
                // Initiator is seller, counterpart is buyer
                let seller_rating =
                    format_user_rating(selected_dispute.initiator_info_data.as_ref());
                let buyer_rating =
                    format_user_rating(selected_dispute.counterpart_info_data.as_ref());
                (buyer_rating, seller_rating)
            }
            None => {
                // Cannot determine roles, show Unknown
                ("Unknown".to_string(), "Unknown".to_string())
            }
        };

        // Format additional timestamps for finalized disputes
        let taken_str = format_local_timestamp(selected_dispute.taken_at, "%Y-%m-%d %H:%M:%S")
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
                    Style::default()
                        .fg(dispute_status_color(selected_dispute.status.as_deref()))
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

        // Keep this near the top (right after Status) so short terminals still show it.
        if is_finalized {
            if let Some(resolution) =
                user_closed_resolution_label(selected_dispute.status.as_deref())
            {
                header_lines.insert(
                    1,
                    Line::from(vec![
                        Span::styled("Resolution: ", Style::default().fg(Color::Gray)),
                        Span::styled(
                            resolution,
                            Style::default()
                                .fg(dispute_status_color(selected_dispute.status.as_deref()))
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                );
            }
        }

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
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
                    .style(Style::default().bg(BACKGROUND_COLOR)),
            )
            .alignment(ratatui::layout::Alignment::Left);
        f.render_widget(header, main_chunks[0]);

        // Only show party tabs, chat, and input for in-progress disputes
        if is_finalized {
            // No panes here: keep keys on the dispute list and chats.
            app.admin_show_solver_dms = false;
        } else {
            // Party Tabs
            let serbero_active = app.admin_show_solver_dms;
            let buyer_style = if !serbero_active && app.active_chat_party == ChatParty::Buyer {
                Style::default()
                    .bg(Color::Green)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let seller_style = if !serbero_active && app.active_chat_party == ChatParty::Seller {
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
                [
                    Constraint::Ratio(1, 3),
                    Constraint::Ratio(1, 3),
                    Constraint::Ratio(1, 3),
                ],
            )
            .split(party_tabs_area);

            // Create multi-line text for buyer and seller tabs with pubkeys
            let buyer_text = vec![
                Line::from(Span::styled(
                    "BUYER",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    fit_party_pubkey(&buyer_pubkey_display, party_chunks[0].width),
                    Style::default(),
                )),
            ];

            let seller_text = vec![
                Line::from(Span::styled(
                    "SELLER",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    fit_party_pubkey(&seller_pubkey_display, party_chunks[1].width),
                    Style::default(),
                )),
            ];

            f.render_widget(
                Paragraph::new(buyer_text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .style(buyer_style),
                    )
                    .alignment(ratatui::layout::Alignment::Center),
                party_chunks[0],
            );
            f.render_widget(
                Paragraph::new(seller_text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .style(seller_style),
                    )
                    .alignment(ratatui::layout::Alignment::Center),
                party_chunks[1],
            );
            let serbero_count = app
                .solver_dms
                .get(&selected_dispute.dispute_id)
                .map_or(0, Vec::len);
            let serbero_style = if serbero_active {
                Style::default()
                    .bg(Color::Magenta)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            };
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled(
                        fit_party_pubkey(
                            &solver_dms_tab_label(serbero_count),
                            party_chunks[2].width,
                        ),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(fit_party_pubkey("assistant", party_chunks[2].width)),
                ])
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .style(serbero_style),
                )
                .alignment(ratatui::layout::Alignment::Center),
                party_chunks[2],
            );

            if serbero_active {
                let dispute_id = selected_dispute.dispute_id.clone();
                if copy_context || main_chunks[2].height < MIN_SERBERO_PANE_HEIGHT {
                    // Short terminal: the messages matter more than the
                    // read-only notice, so the pane takes the input's rows too.
                    let pane = main_chunks[2].union(main_chunks[3]);
                    render_solver_dms(f, pane, app, &dispute_id);
                } else {
                    render_solver_dms(f, main_chunks[2], app, &dispute_id);
                    f.render_widget(
                        Paragraph::new(SOLVER_DMS_READ_ONLY)
                            .style(Style::default().fg(Color::Gray))
                            .block(
                                Block::default()
                                    .title("Message")
                                    .borders(Borders::ALL)
                                    .border_type(BorderType::Rounded)
                                    .border_style(Style::default().fg(Color::Gray)),
                            )
                            .wrap(ratatui::widgets::Wrap { trim: true }),
                        main_chunks[3],
                    );
                }
            } else {
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
                let mut content = build_chat_scrollview_content(
                    messages_slice,
                    app.active_chat_party,
                    content_width,
                    Some(max_content_width),
                );
                let selected_rows = content.select_messages(copy_range, copy_selection);

                let visible_count = content.line_start_per_message.len();
                app.admin_chat_line_starts = content.line_start_per_message.clone();

                if visible_count > 0 {
                    let should_scroll = should_auto_scroll_chat(
                        app.admin_chat_scroll_tracker.as_ref(),
                        dispute_id_key,
                        app.active_chat_party,
                        visible_count,
                    );
                    if should_scroll && copy_selection.is_none() {
                        app.admin_chat_scrollview_state = Default::default();
                        app.admin_chat_scrollview_state.scroll_to_bottom();
                        app.admin_chat_selected_message_idx = Some(visible_count.saturating_sub(1));
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

                let chat_title = if copy_selection.is_some() {
                    format!("Copy: {}", app.active_chat_party)
                } else if visible_count > 0 {
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

                let has_selected_attachment = get_selected_chat_message(app, dispute_id_key)
                    .and_then(|m| m.attachment.as_ref())
                    .is_some();
                // Resolve/Recover/Filter/Remove live in Ctrl+K Actions (like My Trades).
                let mut chat_hints = vec![
                    ("Tab", "Party"),
                    ("Shift+C", filter_hint_label(app.dispute_filter)),
                ];
                if has_selected_attachment {
                    chat_hints.push(("Ctrl+S", "Save file"));
                }
                chat_hints.push(("PgUp/PgDn", "Scroll"));
                let chat_border_hints = if copy_context {
                    Line::default()
                } else {
                    shortcut_bar(chat_area.width.saturating_sub(2), &chat_hints)
                };

                let chat_block = Block::default()
                    .title(chat_title)
                    .title_bottom(chat_border_hints)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(PRIMARY_COLOR))
                    .style(Style::default().bg(BACKGROUND_COLOR));
                let inner_area = chat_block.inner(chat_area);
                f.render_widget(chat_block, chat_area);

                if let Some(selected_rows) = selected_rows {
                    content.keep_selection_visible(
                        selected_rows,
                        inner_area.height,
                        &mut app.admin_chat_scrollview_state,
                    );
                }
                let display_height = content.content_height.max(1);
                let mut scroll_view =
                    ScrollView::new(Size::new(content.content_width, display_height))
                        .vertical_scrollbar_visibility(ScrollbarVisibility::Always);
                let content_rect = Rect::new(0, 0, content.content_width, display_height);
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
                let is_input_enabled = app.admin_chat_input_enabled && copy_selection.is_none();

                let input_style = if is_input_focused && is_input_enabled {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };

                let input_width = main_chunks[3].width;
                let input_title = if copy_selection.is_some() {
                    "Message (copying)"
                } else if is_input_focused && is_input_enabled {
                    if input_width < 36 {
                        "INSERT · Esc"
                    } else {
                        "Message / INSERT"
                    }
                } else if is_input_focused && !is_input_enabled {
                    if input_width < 36 {
                        "COMMAND · i"
                    } else {
                        "Message / COMMAND"
                    }
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
                            .border_type(BorderType::Rounded)
                            .border_style(input_border_style)
                            .style(input_style),
                    )
                    .wrap(Wrap { trim: true });
                f.render_widget(input, main_chunks[3]);
            }
        }

        // Compact keycap command bar (contextual party/file/filter hints sit on the chat border).
        let footer_chunk_idx = if is_finalized { 1 } else { 4 };
        let footer_area = main_chunks[footer_chunk_idx];
        let toast_extra = u16::from(!is_finalized && app.attachment_toast.is_some());
        let hint_height = footer_area.height.saturating_sub(toast_extra);

        if copy_context {
            f.render_widget(
                Paragraph::new(copy_controls).wrap(Wrap { trim: true }),
                footer_area,
            );
            return;
        }

        let managing = matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute));
        let serbero_active = !is_finalized && app.admin_show_solver_dms;
        if !is_finalized {
            if let Some((toast_msg, _)) = app.attachment_toast.as_ref() {
                f.render_widget(
                    Paragraph::new(toast_msg.as_str()).style(Style::default().fg(Color::Yellow)),
                    Rect::new(footer_area.x, footer_area.y, footer_area.width, 1),
                );
            }
        }
        if hint_height > 0 {
            f.render_widget(
                Paragraph::new(dispute_command_bar(
                    footer_area.width,
                    app.admin_chat_input_enabled,
                    is_finalized,
                    managing,
                    serbero_active,
                )),
                Rect::new(
                    footer_area.x,
                    footer_area.y.saturating_add(toast_extra),
                    footer_area.width,
                    hint_height,
                ),
            );
        }
    } else {
        // No disputes available - show empty message with footer
        // Render the outer block first, then content inside it
        let outer_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(PRIMARY_COLOR))
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

        f.render_widget(
            Paragraph::new(shortcut_bar(
                inner_chunks[1].width,
                &[
                    ("Ctrl+H", "Help"),
                    ("Shift+C", filter_hint_label(app.dispute_filter)),
                    ("Shift+R", "Recover"),
                    ("↑↓", "Disputes"),
                ],
            )),
            inner_chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::render_disputes_in_progress;
    use super::should_auto_scroll_chat;
    use super::truncate_dispute_id_label;
    use super::user_closed_resolution_label;
    use crate::models::AdminDispute;
    use crate::ui::key_handler::chat_copy;
    use crate::ui::PRIMARY_COLOR;
    use crate::ui::{AdminMode, AppState, ChatParty, DisputeFilter, UiMode, UserRole};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::time::Instant;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    fn dispute(id: &str, status: &str) -> AdminDispute {
        AdminDispute {
            dispute_id: id.to_string(),
            status: Some(status.to_string()),
            // Keep main-panel fields non-empty so render doesn't panic on unwraps.
            initiator_pubkey: "npub1test".to_string(),
            payment_method: "sepa".to_string(),
            fiat_code: "USD".to_string(),
            ..Default::default()
        }
    }

    fn copy_app() -> AppState {
        let mut app = chat_copy::tests::app_with_messages();
        app.admin_disputes_in_progress[0] = dispute("dispute", "in-progress");
        chat_copy::handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            |_| false,
        );
        app
    }

    fn render_copy(app: &mut AppState, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_disputes_in_progress(frame, frame.area(), app))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn highlighted_word(buffer: &ratatui::buffer::Buffer, word: &str) -> bool {
        (0..buffer.area.height).any(|row| {
            (0..buffer.area.width.saturating_sub(word.len() as u16)).any(|column| {
                word.chars().enumerate().all(|(offset, character)| {
                    let cell = &buffer[(column + offset as u16, row)];
                    cell.symbol() == character.to_string() && cell.bg == PRIMARY_COLOR
                })
            })
        })
    }

    #[test]
    fn chat_copy_highlight_and_controls_survive_narrow_short_and_resized_views() {
        let mut app = copy_app();
        for (width, height) in [(120, 28), (80, 12), (40, 12), (30, 8), (120, 28)] {
            let buffer = render_copy(&mut app, width, height);
            assert!(
                highlighted_word(&buffer, "first"),
                "selection missing at {width}x{height}"
            );
            assert!(
                buffer_contains(&buffer, "Enter"),
                "copy hint missing at {width}x{height}"
            );
            assert!(
                buffer_contains(&buffer, "Esc"),
                "cancel hint missing at {width}x{height}"
            );
            assert_eq!(chat_copy::selected_index(&app), Some(0));
        }
        for (width, height) in [(0, 0), (1, 1), (8, 3)] {
            render_copy(&mut app, width, height);
        }
    }

    #[test]
    fn chat_copy_scrolls_to_last_message_and_ignores_new_message_autoscroll() {
        let mut app = copy_app();
        app.admin_dispute_chats.get_mut("dispute").unwrap()[1].content =
            "first wrapped words ".repeat(40);
        app.chat_copy_session = None;
        chat_copy::handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            |_| false,
        );
        render_copy(&mut app, 60, 12);
        assert_eq!(app.admin_chat_scrollview_state.offset().y, 0);
        chat_copy::handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            |_| false,
        );
        let buffer = render_copy(&mut app, 60, 12);
        assert!(highlighted_word(&buffer, "last"));
        // Scroll keeps the cursor end visible; the anchor may be above the
        // viewport, so assert the range in state rather than on-screen bg.
        assert!(app.admin_chat_scrollview_state.offset().y > 0);
        assert_eq!(chat_copy::selected_range(&app), Some(0..=1));
        let mut incoming = app.admin_dispute_chats["dispute"][2].clone();
        incoming.content = "incoming ".repeat(40);
        app.admin_dispute_chats
            .get_mut("dispute")
            .unwrap()
            .push(incoming);
        let buffer = render_copy(&mut app, 60, 12);
        assert!(highlighted_word(&buffer, "last"));
        assert_eq!(chat_copy::selected_index(&app), Some(1));
        assert_eq!(chat_copy::selected_range(&app), Some(0..=1));
    }

    #[test]
    fn chat_copy_feedback_is_visible_and_highlight_is_removed_after_copy() {
        for (width, height) in [(120, 28), (40, 12), (30, 8)] {
            let mut app = copy_app();
            chat_copy::handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                |_| true,
            );
            let buffer = render_copy(&mut app, width, height);
            assert!(buffer_contains(&buffer, "Copied to clipboard"));
            assert!(!highlighted_word(&buffer, "first"));
        }
    }

    #[test]
    fn truncate_dispute_id_label_keeps_short_ids_unchanged() {
        assert_eq!(truncate_dispute_id_label("short-id", 20), "short-id");
        assert_eq!(
            truncate_dispute_id_label("12345678901234567890", 20),
            "12345678901234567890"
        );
    }

    #[test]
    fn user_closed_resolution_label_matches_user_closed_statuses() {
        assert_eq!(
            user_closed_resolution_label(Some("cooperatively-canceled")),
            Some("Closed by users (cooperative cancel)")
        );
        assert_eq!(
            user_closed_resolution_label(Some("released")),
            Some("Closed by users (seller released)")
        );
        assert_eq!(user_closed_resolution_label(Some("settled")), None);
        assert_eq!(user_closed_resolution_label(Some("seller-refunded")), None);
        assert_eq!(user_closed_resolution_label(Some("in-progress")), None);
        assert_eq!(user_closed_resolution_label(None), None);
    }

    #[test]
    fn finalized_header_shows_closed_by_users_resolution() {
        let mut app = AppState::new(UserRole::Admin);
        app.dispute_filter = DisputeFilter::Finalized;
        app.admin_disputes_in_progress = vec![dispute("dip-coop", "cooperatively-canceled")];
        app.selected_dispute_id = Some("dip-coop".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);

        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "Closed by users"),
            "finalized header must explain user resolution"
        );
        assert!(
            buffer_contains(buf, "Resolution:"),
            "Resolution label must appear after Status"
        );
        assert!(
            buffer_contains(buf, "cooperative cancel"),
            "cooperative cancel wording must be visible"
        );
    }

    #[test]
    fn short_narrow_finalized_header_keeps_closed_by_users_visible() {
        let mut app = AppState::new(UserRole::Admin);
        app.dispute_filter = DisputeFilter::Finalized;
        app.admin_disputes_in_progress = vec![dispute("dip-rel", "released")];
        app.selected_dispute_id = Some("dip-rel".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);

        // Short + narrow: Resolution sits on line 2 of the header so it stays
        // above FINALIZATION DETAILS that may clip.
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "Closed by users"),
            "short/narrow terminal must still show user resolution"
        );
    }

    #[test]
    fn truncate_dispute_id_label_uses_char_boundaries_not_bytes() {
        // 21 multi-byte chars: a byte slice at 20 would panic / split a char.
        let id: String = "あ".repeat(21);
        let truncated = truncate_dispute_id_label(&id, 20);
        assert_eq!(truncated.chars().count(), 23); // 20 + "..."
        assert!(truncated.ends_with("..."));
        assert_eq!(
            truncated.chars().take(20).collect::<String>(),
            "あ".repeat(20)
        );
    }

    #[test]
    fn auto_scrolls_when_chat_context_changes_or_messages_arrive() {
        let tracker = ("dispute-a".to_string(), ChatParty::Buyer, 3);

        assert!(should_auto_scroll_chat(
            None,
            "dispute-a",
            ChatParty::Buyer,
            3
        ));
        assert!(should_auto_scroll_chat(
            Some(&tracker),
            "dispute-b",
            ChatParty::Buyer,
            3
        ));
        assert!(should_auto_scroll_chat(
            Some(&tracker),
            "dispute-a",
            ChatParty::Seller,
            3
        ));
        assert!(should_auto_scroll_chat(
            Some(&tracker),
            "dispute-a",
            ChatParty::Buyer,
            4
        ));
        assert!(!should_auto_scroll_chat(
            Some(&tracker),
            "dispute-a",
            ChatParty::Buyer,
            3
        ));
    }

    /// When more disputes exist than sidebar rows, selecting a late row must
    /// scroll the stateful list so that dispute id is visible (not stuck on
    /// the first rows).
    #[test]
    fn sidebar_scrolls_to_keep_selected_dispute_visible() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = (0..20)
            .map(|i| dispute(&format!("dip-{i:02}"), "in-progress"))
            .collect();
        app.selected_dispute_id = Some("dip-19".to_string());

        // Tall enough for the main panel constraints; sidebar inner height is
        // small enough that 20 rows must scroll (20% of 100 ≈ 20 cols, h=16 → ~14 rows).
        let backend = TestBackend::new(100, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "dip-19"),
            "selected late dispute must be visible after list scroll"
        );
        assert!(
            !buffer_contains(buf, "dip-00"),
            "first dispute should scroll off-screen when selecting the last"
        );
    }

    /// Last sidebar selection must park the shared scrollbar thumb against `▼`
    /// on the sidebar's right edge (same remapping as Orders / Pending).
    #[test]
    fn sidebar_scrollbar_thumb_reaches_track_bottom_on_last_row() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = (0..20)
            .map(|i| dispute(&format!("dip-{i:02}"), "in-progress"))
            .collect();
        app.selected_dispute_id = Some("dip-19".to_string());

        let backend = TestBackend::new(100, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        // Sidebar is ~20% width → right edge of first column ≈ x=19
        let sidebar_right = (buf.area.width as f64 * 0.20).floor() as u16;
        let sidebar_right = sidebar_right.saturating_sub(1);
        let end_cap_y = buf.area.height - 2;
        assert_eq!(
            buf[(sidebar_right, end_cap_y)].symbol(),
            "▼",
            "sidebar scrollbar end cap must sit on the last track row"
        );
        assert_eq!(
            buf[(sidebar_right, end_cap_y - 1)].symbol(),
            "█",
            "thumb must reach the cell above ▼ when the last sidebar dispute is selected"
        );
    }

    #[test]
    fn sidebar_shows_first_disputes_when_selection_is_at_top() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = (0..20)
            .map(|i| dispute(&format!("dip-{i:02}"), "in-progress"))
            .collect();
        app.selected_dispute_id = Some("dip-00".to_string());

        let backend = TestBackend::new(100, 16);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "dip-00"),
            "first dispute must stay visible when selected"
        );
        assert!(
            !buffer_contains(buf, "dip-19"),
            "last dispute should not appear while scrolled to the top"
        );
    }

    #[test]
    fn shortcut_bar_keeps_complete_groups_within_display_width() {
        let hints = [("i", "Write"), ("Ctrl+K", "Actions"), ("Ctrl+H", "Help")];
        for width in 0..120 {
            let line = super::shortcut_bar(width, &hints);
            assert!(line.width() <= usize::from(width));
            let text = line.to_string();
            assert_eq!(text.contains("Ctrl+K"), text.contains("Actions"));
            assert_eq!(text.contains("Ctrl+H"), text.contains("Help"));
        }
        assert_eq!(super::shortcut_bar(9, &hints).to_string(), " i  Write");
        assert!(super::shortcut_bar(8, &hints).spans.is_empty());
        let line = super::shortcut_bar(80, &hints);
        assert_eq!(line.spans[0].style.bg, Some(PRIMARY_COLOR));
        assert_eq!(
            line.spans[3].style.bg,
            Some(ratatui::style::Color::DarkGray)
        );
    }

    #[test]
    fn command_bar_and_copy_controls_fit_narrow_widths() {
        for width in 0..120 {
            for insert in [false, true] {
                let line = super::dispute_command_bar(width, insert, false, true, false);
                assert!(line.width() <= usize::from(width));
                let text = line.to_string();
                if width >= 28 {
                    assert!(text.contains(if insert { "Commands" } else { "Actions" }));
                }
            }
            let controls = super::dispute_copy_controls(width);
            assert!(controls
                .lines
                .iter()
                .all(|line| line.width() <= usize::from(width)));
            if width >= 14 {
                let text = controls.to_string();
                for label in ["Select", "Copy", "Cancel"] {
                    assert!(text.contains(label), "missing {label} at width {width}");
                }
            }
        }
    }

    #[test]
    fn serbero_command_bar_is_read_only() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![dispute("dip-serbero", "in-progress")];
        app.selected_dispute_id = Some("dip-serbero".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        app.admin_show_solver_dms = true;
        app.admin_chat_input_enabled = true;

        let line = super::dispute_command_bar(120, true, false, true, true);
        let text = line.to_string();
        assert!(text.contains("Party"), "expected Tab Party: {text}");
        assert!(text.contains("Actions"), "expected Actions: {text}");
        assert!(
            !text.contains("Write"),
            "SERBERO must not show Write: {text}"
        );
        assert!(!text.contains("Send"), "SERBERO must not show Send: {text}");
    }

    #[test]
    fn shortcut_bar_skips_oversized_group_and_keeps_later_fit() {
        // A long first group that cannot fit must not suppress a later short group.
        let hints = [
            ("Ctrl+Shift+O", "Retry"),
            ("Ctrl+S", "Save"),
            ("Tab", "Peer"),
        ];
        let line = super::shortcut_bar(18, &hints);
        let text = line.to_string();
        assert!(
            text.contains("Save") || text.contains("Peer") || text.contains("Tab"),
            "expected a later short group after skipping Retry, got: {text}"
        );
        assert!(!text.contains("Retry"), "oversized Retry should be skipped");
    }

    #[test]
    fn command_bar_shows_primary_actions_on_one_row() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![dispute("dip-cmd", "in-progress")];
        app.selected_dispute_id = Some("dip-cmd".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        app.admin_chat_input_enabled = false;

        let backend = TestBackend::new(120, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        for label in ["Write", "Actions", "Help", "Copy"] {
            assert!(
                buffer_contains(buffer, label),
                "missing COMMAND label {label}"
            );
        }
        assert!(buffer_contains(buffer, "Party"));
        assert!(buffer_contains(buffer, "Message / COMMAND"));
        assert!(!buffer_contains(buffer, "Shift+I: Enable"));
        assert!(!buffer_contains(buffer, "Shift+F: Resolve"));

        app.admin_chat_input_enabled = true;
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        for label in ["Send", "Commands", "Actions", "Help", "Copy"] {
            assert!(
                buffer_contains(buffer, label),
                "missing INSERT label {label}"
            );
        }
        assert!(!buffer_contains(buffer, "Write"));
        assert!(buffer_contains(buffer, "Message / INSERT"));
    }

    #[test]
    fn party_and_filter_hints_stay_on_the_chat_border() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![dispute("dip-border", "in-progress")];
        app.selected_dispute_id = Some("dip-border".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);

        let backend = TestBackend::new(120, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "Party"),
            "Tab party hint must stay on the chat border"
        );
        assert!(
            buffer_contains(buf, "Finalized") || buffer_contains(buf, "In progress"),
            "filter toggle hint must stay on the chat border"
        );
        assert!(
            buffer_contains(buf, "Actions"),
            "Ctrl+K Actions must remain on the command bar"
        );
    }

    #[test]
    fn command_bar_with_toast_keeps_hints_and_toast() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![dispute("dip-toast", "in-progress")];
        app.selected_dispute_id = Some("dip-toast".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        app.attachment_toast = Some(("File saved".to_string(), Instant::now()));

        let backend = TestBackend::new(80, 28);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(
            buffer_contains(buf, "File saved"),
            "attachment toast must reserve its own footer row"
        );
        assert!(
            buffer_contains(buf, "Actions") || buffer_contains(buf, "Write"),
            "command bar must remain visible with toast"
        );
        assert!(
            buffer_contains(buf, "Party"),
            "party hint must remain on the chat border with toast"
        );
    }

    #[test]
    fn narrow_input_title_keeps_i_and_esc_mode_hints() {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![dispute("dip-narrow", "in-progress")];
        app.selected_dispute_id = Some("dip-narrow".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        app.admin_chat_input_enabled = false;

        // Narrow main panel so the input title uses the compact form (< 36 cols).
        let backend = TestBackend::new(42, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");
        assert!(
            buffer_contains(terminal.backend().buffer(), "COMMAND · i"),
            "narrow COMMAND title must keep the i hint"
        );

        app.admin_chat_input_enabled = true;
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");
        assert!(
            buffer_contains(terminal.backend().buffer(), "INSERT · Esc"),
            "narrow INSERT title must keep the Esc hint"
        );
    }
}

/// Below this many rows the SERBERO pane also takes the read-only input's rows.
const MIN_SERBERO_PANE_HEIGHT: u16 = 4;

/// Shortens a party-tab line (pubkey or label) to fit a tab of `tab_width`
/// columns: three tabs share the row, so narrow panels cut it with `…`.
fn fit_party_pubkey(display: &str, tab_width: u16) -> String {
    let inner = usize::from(tab_width.saturating_sub(2));
    if display.chars().count() <= inner {
        return display.to_string();
    }
    let keep = inner.saturating_sub(1);
    format!("{}…", display.chars().take(keep).collect::<String>())
}

#[cfg(test)]
mod solver_dms_pane_tests {
    use super::*;
    use crate::models::AdminDispute;
    use crate::ui::UserRole;
    use crate::util::solver_dms::SolverDm;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_contains(buf: &ratatui::buffer::Buffer, needle: &str) -> bool {
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat.contains(needle)
    }

    fn app_with_handoff() -> AppState {
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![AdminDispute {
            dispute_id: "dip-1".to_string(),
            status: Some("in-progress".to_string()),
            ..Default::default()
        }];
        app.selected_dispute_id = Some("dip-1".to_string());
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        app.solver_dms.insert(
            "dip-1".to_string(),
            vec![SolverDm {
                event_id: "e1".into(),
                sender_pubkey: String::new(),
                recipient_pubkey: String::new(),
                dispute_id: Some("dip-1".into()),
                subject: "handed off: conflicting_claims".into(),
                text:
                    "Dispute dip-1 · handed off: conflicting_claims\nTopic: payment_not_confirmed"
                        .into(),
                created_at: 100,
            }],
        );
        app
    }

    fn draw(app: &mut AppState) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(120, 32)).expect("terminal");
        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), app))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    #[test]
    fn solver_dm_copy_controls_and_feedback_fit_the_full_view() {
        for (width, height) in [(120, 28), (80, 12), (40, 12), (30, 8)] {
            for success in [true, false] {
                let mut app = chat_copy::tests::app_with_solver_dms();
                assert!(chat_copy::handle_key_with(
                    &mut app,
                    &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                    |_| false
                ));
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| render_disputes_in_progress(frame, frame.area(), &mut app))
                    .unwrap();
                let buffer = terminal.backend().buffer();
                assert!(buffer_contains(buffer, "Copy: Serbero"));
                assert!(buffer_contains(buffer, "newest"));
                assert!(buffer_contains(buffer, "Enter"));
                assert!(buffer_contains(buffer, "Esc"));
                assert!(chat_copy::handle_key_with(
                    &mut app,
                    &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    |_| success
                ));
                terminal
                    .draw(|frame| render_disputes_in_progress(frame, frame.area(), &mut app))
                    .unwrap();
                assert!(buffer_contains(
                    terminal.backend().buffer(),
                    if success {
                        "Copied to clipboard"
                    } else {
                        "Clipboard unavailable"
                    }
                ));
                assert!(app.chat_copy_session.is_none());
                assert_eq!(app.admin_chat_input, "draft\n  untouched");
            }
        }
    }

    #[test]
    fn party_pubkeys_are_shortened_to_fit_narrow_tabs() {
        assert_eq!(
            fit_party_pubkey("abcd1234...wxyz9876", 40),
            "abcd1234...wxyz9876"
        );
        assert_eq!(fit_party_pubkey("abcd1234...wxyz9876", 12), "abcd1234.…");
        assert_eq!(fit_party_pubkey("abcd", 2), "…");
    }

    #[test]
    fn a_narrow_panel_keeps_all_three_tab_names_readable() {
        let mut app = app_with_handoff();
        let mut terminal = Terminal::new(TestBackend::new(60, 32)).expect("terminal");

        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "BUYER"));
        assert!(buffer_contains(buf, "SELLER"));
        assert!(buffer_contains(buf, "SERBERO"));
    }

    #[test]
    fn a_finalized_dispute_turns_the_serbero_pane_off() {
        let mut app = app_with_handoff();
        app.admin_disputes_in_progress[0].status = Some("settled".to_string());
        app.admin_show_solver_dms = true;

        draw(&mut app);

        assert!(!app.admin_show_solver_dms);
    }

    #[test]
    fn a_short_terminal_still_shows_the_serbero_subject() {
        let mut app = app_with_handoff();
        app.admin_show_solver_dms = true;
        let mut terminal = Terminal::new(TestBackend::new(120, 17)).expect("terminal");

        terminal
            .draw(|f| render_disputes_in_progress(f, f.area(), &mut app))
            .expect("draw");

        assert!(buffer_contains(
            terminal.backend().buffer(),
            "handed off: conflicting_claims"
        ));
    }

    #[test]
    fn the_party_row_offers_a_serbero_tab_with_its_count() {
        let mut app = app_with_handoff();

        let buf = draw(&mut app);

        assert!(buffer_contains(&buf, "SERBERO (1)"));
        assert!(!buffer_contains(&buf, "Topic: payment_not_confirmed"));
    }

    #[test]
    fn the_serbero_tab_shows_the_assistant_messages_and_locks_the_input() {
        let mut app = app_with_handoff();
        app.admin_show_solver_dms = true;

        let buf = draw(&mut app);

        assert!(buffer_contains(&buf, "handed off: conflicting_claims"));
        assert!(buffer_contains(&buf, "Topic: payment_not_confirmed"));
        assert!(buffer_contains(&buf, SOLVER_DMS_READ_ONLY));
    }
}
