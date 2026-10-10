use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ui::helpers::{message_visible_for_party, selected_filtered_dispute};
use crate::ui::key_handler::chat_helpers::live_order_chat_draft_target;
use crate::ui::key_handler::{copy_to_native_clipboard, NativeClipboardOutcome};
use crate::ui::terminal::copy_with_osc52;
use crate::ui::{
    AdminMode, AdminTab, AppState, ChatAttachment, ChatParty, ChatSender, DisputeFilter, Tab,
    UiMode, UserChatChannel, UserChatSender, UserRole, UserTab,
};
use crate::SETTINGS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatCopyTarget {
    Dispute { id: String, party: ChatParty },
    Order { id: Uuid, channel: UserChatChannel },
    SolverDm { dispute_id: String },
    Observer { generation: u64 },
}

pub(crate) struct ChatCopySession {
    target: ChatCopyTarget,
    /// Fixed end of the selection while extending with Shift+Up/Down; plain
    /// Up/Down move it together with the cursor (single-message selection).
    anchor_index: usize,
    /// Moving end of the selection (Up/Down cursor).
    selected_index: usize,
    fingerprint: [u8; 32],
    event_id: Option<String>,
    anchor_fingerprint: [u8; 32],
    anchor_event_id: Option<String>,
    /// Fingerprints for every message in the current inclusive range (display order).
    range_fingerprints: Vec<[u8; 32]>,
}

pub(crate) struct ChatCopyFeedback {
    target: ChatCopyTarget,
    text: &'static str,
}

fn focused_target(app: &AppState) -> Option<ChatCopyTarget> {
    if app.user_role == UserRole::Admin && app.observer_inputs_editable() {
        return Some(ChatCopyTarget::Observer {
            generation: app.observer_fetch_generation,
        });
    }
    if app.user_role == UserRole::User
        && app.active_tab == Tab::User(UserTab::MyTrades)
        && app.mode.user_my_trades_interactive()
    {
        return live_order_chat_draft_target(app)
            .map(|(id, channel)| ChatCopyTarget::Order { id, channel });
    }
    if app.user_role != UserRole::Admin
        || app.active_tab != Tab::Admin(AdminTab::DisputesInProgress)
        || !matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute))
        || app.dispute_filter != DisputeFilter::InProgress
    {
        return None;
    }
    selected_filtered_dispute(app)
        .filter(|dispute| {
            !dispute.is_finalized()
                && app.selected_dispute_id.as_deref() == Some(dispute.dispute_id.as_str())
        })
        .map(|dispute| {
            if app.admin_show_solver_dms {
                ChatCopyTarget::SolverDm {
                    dispute_id: dispute.dispute_id,
                }
            } else {
                ChatCopyTarget::Dispute {
                    id: dispute.dispute_id,
                    party: app.active_chat_party,
                }
            }
        })
}

#[derive(Clone, Copy)]
struct CopyMessage<'a> {
    event_id: Option<&'a str>,
    content: &'a str,
    attachment: Option<&'a ChatAttachment>,
    timestamp: i64,
    sender: u8,
}

fn messages<'a>(
    app: &'a AppState,
    target: &ChatCopyTarget,
) -> impl Iterator<Item = CopyMessage<'a>> {
    let (disputes, orders, solver_dms, party) = match target {
        ChatCopyTarget::Dispute { id, party } => {
            (app.admin_dispute_chats.get(id), None, None, Some(*party))
        }
        ChatCopyTarget::Order { id, channel } => {
            let orders = match channel {
                UserChatChannel::Peer => app.order_chats.get(&id.to_string()),
                UserChatChannel::Solver => app.user_dispute_chats.get(&id.to_string()),
            };
            (None, orders, None, None)
        }
        ChatCopyTarget::SolverDm { dispute_id } => {
            (None, None, app.solver_dms.get(dispute_id), None)
        }
        ChatCopyTarget::Observer { .. } => (
            (!app.observer_loading).then_some(&app.observer_messages),
            None,
            None,
            None,
        ),
    };
    disputes
        .into_iter()
        .flatten()
        .filter(move |message| party.is_none_or(|party| message_visible_for_party(message, party)))
        .map(|message| CopyMessage {
            event_id: None,
            content: &message.content,
            attachment: message.attachment.as_ref(),
            timestamp: message.timestamp,
            sender: match message.sender {
                ChatSender::Admin => 0,
                ChatSender::Buyer => 1,
                ChatSender::Seller => 2,
            },
        })
        .chain(orders.into_iter().flatten().map(|message| CopyMessage {
            event_id: None,
            content: &message.content,
            attachment: message.attachment.as_ref(),
            timestamp: message.timestamp,
            sender: match message.sender {
                UserChatSender::You => 0,
                UserChatSender::Peer => 1,
            },
        }))
        .chain(
            solver_dms
                .into_iter()
                .flatten()
                .rev()
                .map(|message| CopyMessage {
                    event_id: Some(&message.event_id),
                    content: &message.text,
                    attachment: None,
                    timestamp: message.created_at,
                    sender: 0,
                }),
        )
}

fn fingerprint(message: CopyMessage<'_>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(message.timestamp.to_le_bytes());
    digest.update([message.sender]);
    digest.update(message.content.len().to_le_bytes());
    digest.update(message.content.as_bytes());
    if let Some(attachment) = &message.attachment {
        digest.update([1]);
        digest.update(attachment.filename.as_bytes());
    } else {
        digest.update([0]);
    }
    digest.finalize().into()
}

fn resolve_message_index(
    app: &AppState,
    target: &ChatCopyTarget,
    event_id: Option<&str>,
    index: usize,
    expected: [u8; 32],
) -> Option<usize> {
    let resolved = if let Some(event_id) = event_id {
        messages(app, target).position(|message| message.event_id == Some(event_id))?
    } else {
        index
    };
    messages(app, target)
        .nth(resolved)
        .filter(|message| fingerprint(*message) == expected)
        .map(|_| resolved)
}

fn copy_text_for_message(message: CopyMessage<'_>) -> Option<String> {
    match &message.attachment {
        Some(attachment) => (!attachment.filename.is_empty()).then(|| attachment.filename.clone()),
        None => Some(message.content.to_owned()),
    }
}

fn selection_bounds(session: &ChatCopySession) -> (usize, usize) {
    (
        session.anchor_index.min(session.selected_index),
        session.anchor_index.max(session.selected_index),
    )
}

fn range_fingerprints_for(
    app: &AppState,
    target: &ChatCopyTarget,
    start: usize,
    end: usize,
) -> Vec<[u8; 32]> {
    messages(app, target)
        .skip(start)
        .take(end.saturating_sub(start).saturating_add(1))
        .map(fingerprint)
        .collect()
}

fn refresh_range_fingerprints(app: &AppState, session: &mut ChatCopySession) {
    let (start, end) = selection_bounds(session);
    session.range_fingerprints = range_fingerprints_for(app, &session.target, start, end);
}

pub(crate) fn validate_selection(app: &mut AppState) {
    let Some(mut session) = app.chat_copy_session.take() else {
        return;
    };
    let focused = focused_target(app).as_ref() == Some(&session.target);
    let cursor = focused
        .then(|| {
            resolve_message_index(
                app,
                &session.target,
                session.event_id.as_deref(),
                session.selected_index,
                session.fingerprint,
            )
        })
        .flatten();
    let anchor = focused
        .then(|| {
            resolve_message_index(
                app,
                &session.target,
                session.anchor_event_id.as_deref(),
                session.anchor_index,
                session.anchor_fingerprint,
            )
        })
        .flatten();
    if let (Some(cursor), Some(anchor)) = (cursor, anchor) {
        session.selected_index = cursor;
        session.anchor_index = anchor;
        let (start, end) = selection_bounds(&session);
        let current = range_fingerprints_for(app, &session.target, start, end);
        if current == session.range_fingerprints {
            app.chat_copy_session = Some(session);
            return;
        }
    }
    app.chat_copy_cancelled = true;
}

pub(crate) fn selected_index(app: &AppState) -> Option<usize> {
    app.chat_copy_session
        .as_ref()
        .map(|session| session.selected_index)
}

pub(crate) fn selected_range(app: &AppState) -> Option<std::ops::RangeInclusive<usize>> {
    app.chat_copy_session.as_ref().map(|session| {
        let (start, end) = selection_bounds(session);
        start..=end
    })
}

pub(crate) fn validate_order_view(app: &mut AppState, order_id: &str, channel: UserChatChannel) {
    let displayed = Uuid::parse_str(order_id)
        .ok()
        .map(|id| ChatCopyTarget::Order { id, channel });
    if app
        .chat_copy_session
        .as_ref()
        .is_some_and(|session| Some(&session.target) != displayed.as_ref())
    {
        app.chat_copy_session = None;
        app.chat_copy_cancelled = true;
    }
    if app
        .chat_copy_feedback
        .as_ref()
        .is_some_and(|feedback| Some(&feedback.target) != displayed.as_ref())
    {
        app.chat_copy_feedback = None;
    }
}

pub(crate) fn feedback_text(app: &AppState) -> Option<&'static str> {
    let feedback = app.chat_copy_feedback.as_ref()?;
    (focused_target(app).as_ref() == Some(&feedback.target)).then_some(feedback.text)
}

#[derive(Debug, PartialEq, Eq)]
enum CopyOutcome {
    Copied,
    SentToTerminal,
    Unavailable,
    Indeterminate,
}

fn copy_chat_text_with(
    text: String,
    osc52_enabled: bool,
    native: impl FnOnce(String) -> NativeClipboardOutcome,
    terminal: impl FnOnce(&str) -> bool,
) -> CopyOutcome {
    let fallback = osc52_enabled.then(|| Zeroizing::new(text.clone()));
    match native(text) {
        NativeClipboardOutcome::Copied => return CopyOutcome::Copied,
        NativeClipboardOutcome::Indeterminate => return CopyOutcome::Indeterminate,
        NativeClipboardOutcome::Failed => {}
    }
    if fallback
        .as_ref()
        .is_some_and(|text| terminal(text.as_str()))
    {
        CopyOutcome::SentToTerminal
    } else {
        CopyOutcome::Unavailable
    }
}

pub(crate) fn handle_key(app: &mut AppState, key: &KeyEvent) -> bool {
    handle_key_with_result(app, key, |text| {
        copy_chat_text_with(
            text,
            SETTINGS
                .get()
                .is_some_and(|settings| settings.clipboard_osc52),
            copy_to_native_clipboard,
            copy_with_osc52,
        )
    })
}

#[cfg(test)]
pub(crate) fn handle_key_with(
    app: &mut AppState,
    key: &KeyEvent,
    copy: impl FnOnce(String) -> bool,
) -> bool {
    handle_key_with_result(app, key, |text| {
        if copy(text) {
            CopyOutcome::Copied
        } else {
            CopyOutcome::Unavailable
        }
    })
}

fn handle_key_with_result(
    app: &mut AppState,
    key: &KeyEvent,
    copy: impl FnOnce(String) -> CopyOutcome,
) -> bool {
    if app.chat_copy_block_enter && key.code == KeyCode::Enter {
        return true;
    }
    app.chat_copy_block_enter = false;
    app.chat_copy_feedback = None;
    if app.user_role == UserRole::Admin
        && app.observer_inputs_editable()
        && (app.chat_copy_session.is_some() || app.chat_copy_cancelled)
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L'))
    {
        app.clear_observer_secrets();
        app.chat_copy_session = None;
        app.chat_copy_cancelled = false;
        return true;
    }
    validate_selection(app);
    if std::mem::take(&mut app.chat_copy_cancelled) {
        return true;
    }

    if let Some(mut session) = app.chat_copy_session.take() {
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Enter => {
                let (start, end) = selection_bounds(&session);
                let selected: Vec<_> = messages(app, &session.target)
                    .skip(start)
                    .take(end.saturating_sub(start).saturating_add(1))
                    .collect();
                let mut parts = Vec::with_capacity(selected.len());
                let mut missing_filename = false;
                for message in selected {
                    match copy_text_for_message(message) {
                        Some(text) => parts.push(text),
                        None => {
                            missing_filename = true;
                            break;
                        }
                    }
                }
                let feedback = if missing_filename || parts.is_empty() {
                    "No filename to copy"
                } else {
                    match copy(parts.join("\n\n")) {
                        CopyOutcome::Copied => "Copied to clipboard",
                        CopyOutcome::SentToTerminal => "Sent to terminal clipboard",
                        CopyOutcome::Unavailable => "Clipboard unavailable",
                        CopyOutcome::Indeterminate => "Clipboard result unknown",
                    }
                };
                app.chat_copy_feedback = Some(ChatCopyFeedback {
                    target: session.target,
                    text: feedback,
                });
                app.chat_copy_block_enter = true;
                return true;
            }
            KeyCode::Up | KeyCode::Down => {
                let count = messages(app, &session.target).count();
                session.selected_index = if key.code == KeyCode::Up {
                    session.selected_index.saturating_sub(1)
                } else {
                    session
                        .selected_index
                        .saturating_add(1)
                        .min(count.saturating_sub(1))
                };
                if let Some(message) = messages(app, &session.target).nth(session.selected_index) {
                    session.event_id = message.event_id.map(str::to_owned);
                    session.fingerprint = fingerprint(message);
                }
                // Plain Up/Down highlight a single message: the anchor follows the
                // cursor. Shift+Up/Down keep the anchor so the range extends/shrinks.
                if !key.modifiers.contains(KeyModifiers::SHIFT) {
                    session.anchor_index = session.selected_index;
                    session.anchor_event_id = session.event_id.clone();
                    session.anchor_fingerprint = session.fingerprint;
                }
                refresh_range_fingerprints(app, &mut session);
            }
            _ => {}
        }
        app.chat_copy_session = Some(session);
        return true;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
    {
        if let Some(target) = focused_target(app) {
            let first = messages(app, &target)
                .next()
                .map(|message| (message.event_id.map(str::to_owned), fingerprint(message)));
            if let Some((event_id, fingerprint)) = first {
                app.chat_copy_session = Some(ChatCopySession {
                    target,
                    anchor_index: 0,
                    selected_index: 0,
                    fingerprint,
                    event_id: event_id.clone(),
                    anchor_fingerprint: fingerprint,
                    anchor_event_id: event_id,
                    range_fingerprints: vec![fingerprint],
                });
            } else {
                app.chat_copy_feedback = Some(ChatCopyFeedback {
                    target,
                    text: "No messages to copy",
                });
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::models::AdminDispute;
    use crate::ui::helpers::OrderChatListItem;
    use crate::ui::key_handler::chat_helpers::sync_order_chat_draft_to_live_target;
    use crate::ui::{ChatAttachmentType, DisputeChatMessage, UserMode, UserOrderChatMessage};
    use crate::util::solver_dms::SolverDm;
    use mostro_core::prelude::Status;

    pub(crate) fn copy_views() -> [AppState; 6] {
        let mut seller = app_with_messages();
        seller.active_chat_party = ChatParty::Seller;
        [
            app_with_messages(),
            seller,
            app_with_solver_dms(),
            app_with_order_messages(UserChatChannel::Peer),
            app_with_order_messages(UserChatChannel::Solver),
            app_with_observer_messages(),
        ]
    }

    #[test]
    fn chat_copy_matrix_preserves_inputs_and_normal_selection_on_every_exit() {
        for enabled in [false, true] {
            for outcome in [None, Some(false), Some(true)] {
                for mut app in copy_views() {
                    app.admin_chat_input_enabled = enabled;
                    app.order_chat_input_enabled = enabled;
                    app.admin_chat_selected_message_idx = Some(7);
                    app.order_chat_selected_message_idx = Some(7);
                    let inputs = (
                        app.admin_chat_input.clone(),
                        app.order_chat_input.clone(),
                        app.observer_shared_key_input.clone(),
                        app.order_chat_draft_owner,
                    );
                    let mode = std::mem::discriminant(&app.mode);
                    let target = focused_target(&app).unwrap();
                    let expected =
                        copy_text_for_message(messages(&app, &target).next().unwrap()).unwrap();
                    enter_selection(&mut app);
                    press(&mut app, KeyCode::Up);
                    enter_selection(&mut app);
                    assert_eq!(selected_index(&app), Some(0));
                    let exit = if outcome.is_some() {
                        KeyCode::Enter
                    } else {
                        KeyCode::Esc
                    };
                    let mut copied = false;
                    assert!(handle_key_with(
                        &mut app,
                        &KeyEvent::new(exit, KeyModifiers::NONE),
                        |text| {
                            copied = true;
                            assert_eq!(text, expected);
                            outcome.unwrap_or(false)
                        }
                    ));
                    assert_eq!(copied, outcome.is_some());
                    assert!(app.chat_copy_session.is_none());
                    assert_eq!(std::mem::discriminant(&app.mode), mode);
                    assert_eq!(focused_target(&app), Some(target));
                    assert_eq!(
                        (
                            app.admin_chat_input,
                            app.order_chat_input,
                            app.observer_shared_key_input,
                            app.order_chat_draft_owner
                        ),
                        inputs
                    );
                    assert_eq!(app.admin_chat_input_enabled, enabled);
                    assert_eq!(app.order_chat_input_enabled, enabled);
                    assert_eq!(app.admin_chat_selected_message_idx, Some(7));
                    assert_eq!(app.order_chat_selected_message_idx, Some(7));
                }
            }
        }
    }

    #[test]
    fn chat_copy_post_copy_enter_requires_fresh_input() {
        for copied in [false, true] {
            for mut app in copy_views() {
                enter_selection(&mut app);
                let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
                assert!(handle_key_with(&mut app, &enter, |_| copied));
                let feedback = feedback_text(&app);
                for _ in 0..3 {
                    assert!(handle_key_with(&mut app, &enter, |_| panic!(
                        "must not copy again"
                    )));
                    assert_eq!(feedback_text(&app), feedback);
                    assert!(app.chat_copy_session.is_none());
                }
                assert!(!handle_key_with(
                    &mut app,
                    &KeyEvent::new(KeyCode::Null, KeyModifiers::NONE),
                    |_| false
                ));
                assert!(!handle_key_with(&mut app, &enter, |_| false));
            }
        }
    }

    #[test]
    fn chat_copy_matrix_popup_invalidation_consumes_one_key_without_copying() {
        for mut app in copy_views() {
            enter_selection(&mut app);
            app.mode = UiMode::HelpPopup(app.active_tab, Box::new(app.mode.clone()));
            validate_selection(&mut app);
            assert!(app.chat_copy_session.is_none());
            press(&mut app, KeyCode::Enter);
            assert!(!app.chat_copy_cancelled);
            assert!(matches!(app.mode, UiMode::HelpPopup(..)));
            assert!(!handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                |_| panic!("popup must own the next key")
            ));
            assert!(!handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                |_| panic!("popup must prevent entry")
            ));
        }
    }

    #[test]
    fn chat_copy_matrix_long_tokens_keep_measured_selection_visible() {
        use crate::ui::helpers::ChatScrollViewContent;
        use crate::ui::PRIMARY_COLOR;
        use ratatui::{
            backend::TestBackend,
            text::Line,
            widgets::{Paragraph, Wrap},
            Terminal,
        };

        for width in [12, 28, 78] {
            for trim in [false, true] {
                let lines = vec![
                    Line::from(format!("lnbc{}", "q".repeat(512))),
                    Line::from("  \u{65e5}\u{672c}\u{8a9e}  "),
                    Line::from(""),
                    Line::from("last"),
                    Line::from(""),
                ];
                let wrap = Wrap { trim };
                let expected_height = Paragraph::new(lines.clone()).wrap(wrap).line_count(width);
                for selected in [0, 1] {
                    let mut content = ChatScrollViewContent {
                        lines: lines.clone(),
                        content_height: 5,
                        content_width: width,
                        line_start_per_message: vec![0, 3],
                    };
                    let range = content
                        .select_messages_with_wrap(Some(selected..=selected), Some(selected), wrap)
                        .unwrap();
                    assert_eq!(usize::from(content.content_height), expected_height);
                    let offset = content.selection_scroll_offset(range.clone(), 3, 0);
                    if selected == 0 {
                        assert!(range.len() > 3);
                        assert_eq!(offset, 0);
                        assert_eq!(content.selection_scroll_offset(range, 3, u16::MAX), 0);
                    } else {
                        assert!(offset > 0);
                    }
                    let mut terminal = Terminal::new(TestBackend::new(width, 3)).unwrap();
                    terminal
                        .draw(|frame| {
                            frame.render_widget(
                                Paragraph::new(content.lines.clone())
                                    .wrap(wrap)
                                    .scroll((offset, 0)),
                                frame.area(),
                            )
                        })
                        .unwrap();
                    let highlighted: String = terminal
                        .backend()
                        .buffer()
                        .content()
                        .iter()
                        .filter(|cell| cell.bg == PRIMARY_COLOR && cell.symbol() != " ")
                        .map(|cell| cell.symbol())
                        .collect();
                    if selected == 0 {
                        assert!(highlighted.starts_with("lnbc"));
                    } else {
                        assert_eq!(highlighted, "last");
                    }
                }
            }
        }
    }

    #[test]
    fn chat_copy_matrix_feedback_fits_small_views_and_stays_scoped() {
        use crate::ui::tabs::{disputes_in_progress_tab, observer_tab, order_in_progress_tab};
        use ratatui::{backend::TestBackend, Terminal};

        for outcome in [
            CopyOutcome::Copied,
            CopyOutcome::SentToTerminal,
            CopyOutcome::Unavailable,
            CopyOutcome::Indeterminate,
        ] {
            let expected = match outcome {
                CopyOutcome::Copied => "Copied to clipboard",
                CopyOutcome::SentToTerminal => "Sent to terminal clipboard",
                CopyOutcome::Unavailable => "Clipboard unavailable",
                CopyOutcome::Indeterminate => "Clipboard result unknown",
            };
            for mut app in copy_views() {
                enter_selection(&mut app);
                handle_key_with_result(
                    &mut app,
                    &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    |_| match outcome {
                        CopyOutcome::Copied => CopyOutcome::Copied,
                        CopyOutcome::SentToTerminal => CopyOutcome::SentToTerminal,
                        CopyOutcome::Unavailable => CopyOutcome::Unavailable,
                        CopyOutcome::Indeterminate => CopyOutcome::Indeterminate,
                    },
                );
                for (width, height) in [(30, 8), (40, 12), (80, 24)] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal
                        .draw(|frame| match app.active_tab {
                            Tab::Admin(AdminTab::DisputesInProgress) => {
                                disputes_in_progress_tab::render_disputes_in_progress(
                                    frame,
                                    frame.area(),
                                    &mut app,
                                )
                            }
                            Tab::Admin(AdminTab::Observer) => {
                                observer_tab::render_observer_tab(frame, frame.area(), &mut app)
                            }
                            Tab::User(UserTab::MyTrades) => {
                                order_in_progress_tab::render_order_in_progress(
                                    frame,
                                    frame.area(),
                                    &mut app,
                                )
                            }
                            _ => unreachable!(),
                        })
                        .unwrap();
                    let rendered: String = terminal
                        .backend()
                        .buffer()
                        .content()
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect();
                    for word in expected.split_whitespace() {
                        assert!(
                            rendered.contains(word),
                            "{width}x{height}: missing {word} for {:?}",
                            app.active_tab
                        );
                    }
                    assert_eq!(feedback_text(&app), Some(expected));
                }
                let original_tab = app.active_tab;
                app.active_tab = Tab::User(UserTab::Settings);
                assert_eq!(feedback_text(&app), None);
                app.active_tab = original_tab;
                assert!(!handle_key_with(
                    &mut app,
                    &KeyEvent::new(KeyCode::Null, KeyModifiers::NONE),
                    |_| panic!("feedback must not copy")
                ));
                assert_eq!(feedback_text(&app), None);
            }
        }
    }

    #[test]
    fn osc52_fallback_requires_opt_in_and_native_failure() {
        for enabled in [false, true] {
            for native_result in [
                NativeClipboardOutcome::Copied,
                NativeClipboardOutcome::Failed,
                NativeClipboardOutcome::Indeterminate,
            ] {
                for terminal_success in [false, true] {
                    let payload = "  private\n\u{00e9}\x1b\t  ";
                    let mut native_called = false;
                    let mut terminal_called = false;
                    let outcome = copy_chat_text_with(
                        payload.into(),
                        enabled,
                        |text| {
                            native_called = true;
                            assert_eq!(text, payload);
                            native_result
                        },
                        |text| {
                            terminal_called = true;
                            assert_eq!(text, payload);
                            terminal_success
                        },
                    );
                    assert!(native_called);
                    assert_eq!(
                        terminal_called,
                        enabled && native_result == NativeClipboardOutcome::Failed
                    );
                    assert_eq!(
                        outcome,
                        if native_result == NativeClipboardOutcome::Copied {
                            CopyOutcome::Copied
                        } else if native_result == NativeClipboardOutcome::Indeterminate {
                            CopyOutcome::Indeterminate
                        } else if enabled && terminal_success {
                            CopyOutcome::SentToTerminal
                        } else {
                            CopyOutcome::Unavailable
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn osc52_does_not_fallback_when_a_timed_out_worker_later_succeeds() {
        use crate::ui::key_handler::clipboard_worker_outcome;
        use std::sync::mpsc::{channel, RecvTimeoutError};
        use std::time::Duration;

        let (sender, receiver) = channel();
        let outcome = copy_chat_text_with(
            "private text".into(),
            true,
            |_| clipboard_worker_outcome(receiver.recv_timeout(Duration::ZERO)),
            |_| panic!("an unconfirmed native write must not reach another clipboard"),
        );
        assert_eq!(outcome, CopyOutcome::Indeterminate);
        sender.send(true).unwrap();
        assert_eq!(
            clipboard_worker_outcome(receiver.try_recv().map_err(|_| RecvTimeoutError::Timeout)),
            NativeClipboardOutcome::Copied
        );
        assert_eq!(
            clipboard_worker_outcome(Ok(false)),
            NativeClipboardOutcome::Failed
        );
        assert_eq!(
            clipboard_worker_outcome(Err(RecvTimeoutError::Disconnected)),
            NativeClipboardOutcome::Indeterminate
        );
    }

    #[test]
    fn osc52_cap_does_not_limit_native_clipboard() {
        let text = "x".repeat(65 * 1024);
        let outcome = copy_chat_text_with(
            text.clone(),
            true,
            |payload| {
                assert_eq!(payload, text);
                NativeClipboardOutcome::Copied
            },
            |_| panic!("native success must bypass OSC 52"),
        );
        assert_eq!(outcome, CopyOutcome::Copied);
    }

    #[test]
    fn osc52_feedback_distinguishes_sent_from_copied_in_every_chat_view() {
        for mut app in [
            app_with_messages(),
            app_with_solver_dms(),
            app_with_order_messages(UserChatChannel::Peer),
            app_with_observer_messages(),
        ] {
            enter_selection(&mut app);
            assert!(handle_key_with_result(
                &mut app,
                &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                |text| {
                    copy_chat_text_with(text, true, |_| NativeClipboardOutcome::Failed, |_| true)
                }
            ));
            assert!(app.chat_copy_session.is_none());
            assert_eq!(feedback_text(&app), Some("Sent to terminal clipboard"));
            assert!(!handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Null, KeyModifiers::NONE),
                |_| false
            ));
            assert!(feedback_text(&app).is_none());
        }
    }

    pub(crate) fn app_with_observer_messages() -> AppState {
        let mut app = app_with_messages();
        app.active_tab = Tab::Admin(AdminTab::Observer);
        app.mode = UiMode::AdminMode(AdminMode::Normal);
        app.observer_shared_key_input = "a".repeat(64);
        app.observer_messages = vec![
            message(ChatSender::Seller, "  first\n\ttext \u{00e9}\u{754c}  "),
            message(ChatSender::Buyer, "last message"),
        ];
        app
    }

    #[test]
    fn observer_copy_preserves_shared_key_and_copies_unfiltered_raw_messages() {
        for exit in [KeyCode::Enter, KeyCode::Esc] {
            for selected in 0..2 {
                let mut app = app_with_observer_messages();
                app.observer_error = Some("existing error".into());
                let first = copy_text_for_message(CopyMessage {
                    event_id: None,
                    content: &app.observer_messages[0].content,
                    attachment: app.observer_messages[0].attachment.as_ref(),
                    timestamp: app.observer_messages[0].timestamp,
                    sender: 2,
                })
                .unwrap();
                let second = copy_text_for_message(CopyMessage {
                    event_id: None,
                    content: &app.observer_messages[1].content,
                    attachment: app.observer_messages[1].attachment.as_ref(),
                    timestamp: app.observer_messages[1].timestamp,
                    sender: 1,
                })
                .unwrap();
                let expected = if selected == 0 {
                    first
                } else {
                    format!("{first}\n\n{second}")
                };
                let generation = app.observer_fetch_generation;
                enter_selection(&mut app);
                press(&mut app, KeyCode::Up);
                assert_eq!(selected_index(&app), Some(0));
                assert_eq!(selected_range(&app), Some(0..=0));
                if selected == 1 {
                    press_shift(&mut app, KeyCode::Down);
                    press_shift(&mut app, KeyCode::Down);
                }
                assert_eq!(selected_index(&app), Some(selected));
                assert_eq!(selected_range(&app), Some(0..=selected));
                let mut copied = false;
                assert!(handle_key_with(
                    &mut app,
                    &KeyEvent::new(exit, KeyModifiers::NONE),
                    |text| {
                        assert_eq!(text, expected);
                        copied = true;
                        true
                    }
                ));
                assert_eq!(copied, exit == KeyCode::Enter);
                assert!(app.chat_copy_session.is_none());
                assert_eq!(app.observer_shared_key_input, "a".repeat(64));
                assert_eq!(app.observer_error.as_deref(), Some("existing error"));
                assert_eq!(app.observer_fetch_generation, generation);
                assert!(!app.observer_loading);
                assert!(app.observer_inputs_editable());
            }
        }
    }

    #[test]
    fn observer_copy_attachment_uses_filename_without_changing_shared_key() {
        let mut app = app_with_observer_messages();
        app.observer_messages[0].attachment = Some(ChatAttachment {
            blossom_url: "https://example.com/blob".into(),
            filename: "receipt.txt".into(),
            mime_type: None,
            file_type: ChatAttachmentType::File,
            decryption_key: None,
        });
        let expected = copy_text_for_message(CopyMessage {
            event_id: None,
            content: &app.observer_messages[0].content,
            attachment: app.observer_messages[0].attachment.as_ref(),
            timestamp: app.observer_messages[0].timestamp,
            sender: 2,
        })
        .unwrap();
        enter_selection(&mut app);
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, expected);
                true
            }
        ));
        assert_eq!(app.observer_shared_key_input, "a".repeat(64));
    }

    #[test]
    fn observer_copy_ctrl_l_clears_secrets_and_selection() {
        for character in ['l', 'L'] {
            let mut app = app_with_observer_messages();
            app.observer_error = Some("private error".into());
            let generation = app.observer_fetch_generation;
            enter_selection(&mut app);
            assert!(handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL),
                |_| panic!("clear must not copy")
            ));
            assert!(app.chat_copy_session.is_none());
            assert!(!app.chat_copy_cancelled);
            assert!(app.observer_shared_key_input.is_empty());
            assert!(app.observer_messages.is_empty());
            assert!(app.observer_error.is_none());
            assert!(!app.observer_loading);
            assert!(app.observer_fetch_generation > generation);
        }
    }

    #[test]
    fn observer_copy_invalidates_on_refetch_clear_replacement_and_role_change() {
        for change in 0..4 {
            let mut app = app_with_observer_messages();
            enter_selection(&mut app);
            match change {
                0 => {
                    let old_messages = app.observer_messages.clone();
                    app.begin_observer_fetch();
                    app.observer_messages = old_messages;
                    app.observer_loading = false;
                }
                1 => app.clear_observer_secrets(),
                2 => app.observer_messages[0].content = "replacement".into(),
                _ => app.switch_role(UserRole::User),
            }
            validate_selection(&mut app);
            assert!(app.chat_copy_session.is_none());
            press(&mut app, KeyCode::Enter);
        }
    }

    #[test]
    fn observer_copy_empty_loading_and_popups_are_safe() {
        for loading in [false, true] {
            let mut app = app_with_observer_messages();
            app.observer_loading = loading;
            if !loading {
                app.observer_messages.clear();
            }
            enter_selection(&mut app);
            assert!(app.chat_copy_session.is_none());
            assert_eq!(feedback_text(&app), Some("No messages to copy"));
            assert_eq!(app.observer_shared_key_input, "a".repeat(64));
            app.mode = UiMode::HelpPopup(app.active_tab, Box::new(app.mode.clone()));
            assert!(!handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                |_| false
            ));
        }
    }

    pub(crate) fn app_with_solver_dms() -> AppState {
        let mut app = app_with_messages();
        app.admin_show_solver_dms = true;
        app.solver_dms.insert(
            "dispute".into(),
            ["older", "newest"]
                .into_iter()
                .enumerate()
                .map(|(index, id)| SolverDm {
                    event_id: id.into(),
                    sender_pubkey: "sender".into(),
                    recipient_pubkey: "recipient".into(),
                    dispute_id: Some("dispute".into()),
                    subject: "same subject".into(),
                    text: format!(
                        "Dispute dispute - same subject\n  {id}\n\ttext \u{00e9}\u{754c}  "
                    ),
                    created_at: index as i64,
                })
                .collect(),
        );
        app
    }

    #[test]
    fn solver_dm_copy_preserves_selected_event_on_arrival_and_copies_full_text() {
        let mut app = app_with_solver_dms();
        let dm = &app.solver_dms["dispute"][1];
        let expected = copy_text_for_message(CopyMessage {
            event_id: Some(&dm.event_id),
            content: &dm.text,
            attachment: None,
            timestamp: dm.created_at,
            sender: 0,
        })
        .unwrap();
        enter_selection(&mut app);
        assert_eq!(selected_index(&app), Some(0));
        let mut incoming = app.solver_dms["dispute"][1].clone();
        incoming.event_id = "incoming".into();
        incoming.text = "new incoming message".into();
        app.solver_dms.get_mut("dispute").unwrap().push(incoming);
        validate_selection(&mut app);
        assert_eq!(selected_index(&app), Some(1));
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, expected);
                true
            }
        ));
        assert_eq!(feedback_text(&app), Some("Copied to clipboard"));
        assert_eq!(app.admin_chat_input, "draft\n  untouched");
    }

    #[test]
    fn solver_dm_copy_navigation_clamps_and_cancels_without_editing() {
        let mut app = app_with_solver_dms();
        enter_selection(&mut app);
        press(&mut app, KeyCode::Up);
        assert_eq!(selected_index(&app), Some(0));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_index(&app), Some(1));
        for code in [
            KeyCode::Tab,
            KeyCode::Char('x'),
            KeyCode::End,
            KeyCode::PageDown,
            KeyCode::Esc,
        ] {
            press(&mut app, code);
        }
        assert!(app.chat_copy_session.is_none());
        assert!(app.admin_show_solver_dms);
        assert_eq!(app.admin_chat_input, "draft\n  untouched");
    }

    #[test]
    fn solver_dm_copy_cancels_on_removal_replacement_or_context_change() {
        for change in 0..4 {
            let mut app = app_with_solver_dms();
            enter_selection(&mut app);
            match change {
                0 => {
                    app.solver_dms.get_mut("dispute").unwrap().pop();
                }
                1 => app.solver_dms.get_mut("dispute").unwrap()[1].text = "replacement".into(),
                2 => app.admin_show_solver_dms = false,
                _ => app.selected_dispute_id = None,
            }
            validate_selection(&mut app);
            press(&mut app, KeyCode::Enter);
            assert!(app.chat_copy_session.is_none());
            assert_eq!(app.admin_chat_input, "draft\n  untouched");
        }
    }

    #[test]
    fn solver_dm_copy_empty_chat_and_popup_do_not_change_input() {
        let mut app = app_with_solver_dms();
        app.solver_dms.clear();
        enter_selection(&mut app);
        assert!(app.chat_copy_session.is_none());
        assert_eq!(feedback_text(&app), Some("No messages to copy"));
        assert_eq!(app.admin_chat_input, "draft\n  untouched");
        app.mode = UiMode::HelpPopup(app.active_tab, Box::new(app.mode.clone()));
        assert!(!handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            |_| false
        ));
        assert!(app.chat_copy_session.is_none());
    }

    pub(crate) fn app_with_order_messages(channel: UserChatChannel) -> AppState {
        let mut app = AppState::new(UserRole::User);
        app.active_tab = Tab::User(UserTab::MyTrades);
        app.mode = UiMode::UserMode(UserMode::Normal);
        app.active_user_chat_channel = channel;
        app.order_chat_input = "draft\n  untouched".into();
        app.order_chat_draft_owner = Some((Uuid::nil(), channel));
        app.my_trades_maker_book.push(OrderChatListItem {
            order_id: Uuid::nil().to_string(),
            status: Some(Status::Dispute),
            amount: Some(1000),
            fiat: Some((10, "USD".into())),
            trade_index: Some(1),
            payment_method: Some("cash".into()),
            premium: Some(0),
            buyer_trade_pubkey: None,
            seller_trade_pubkey: None,
            buyer_reputation: None,
            seller_reputation: None,
            solver_pubkey: Some("solver".into()),
            dispute_id: None,
        });
        for (chats, name) in [
            (&mut app.order_chats, "peer"),
            (&mut app.user_dispute_chats, "solver"),
        ] {
            chats.insert(
                Uuid::nil().to_string(),
                vec![
                    UserOrderChatMessage {
                        sender: UserChatSender::Peer,
                        content: format!("  {name} first\n\tmessage \u{00e9}\u{754c}  "),
                        timestamp: 1,
                        attachment: None,
                    },
                    UserOrderChatMessage {
                        sender: UserChatSender::You,
                        content: format!("{name} last"),
                        timestamp: 2,
                        attachment: None,
                    },
                ],
            );
        }
        app
    }

    #[test]
    fn my_trades_copy_preserves_draft_owner_and_layer_in_both_channels() {
        for channel in [UserChatChannel::Peer, UserChatChannel::Solver] {
            for input_enabled in [false, true] {
                for exit in [KeyCode::Esc, KeyCode::Enter] {
                    let mut app = app_with_order_messages(channel);
                    app.order_chat_input_enabled = input_enabled;
                    let target = focused_target(&app).unwrap();
                    let expected =
                        copy_text_for_message(messages(&app, &target).next().unwrap()).unwrap();
                    enter_selection(&mut app);
                    assert_eq!(selected_index(&app), Some(0));
                    press(&mut app, KeyCode::Up);
                    press(&mut app, KeyCode::Tab);
                    press(&mut app, KeyCode::Char('x'));
                    let mut copied = false;
                    assert!(handle_key_with(
                        &mut app,
                        &KeyEvent::new(exit, KeyModifiers::NONE),
                        |text| {
                            assert_eq!(text, expected);
                            copied = true;
                            true
                        }
                    ));
                    assert_eq!(copied, exit == KeyCode::Enter);
                    assert!(app.chat_copy_session.is_none());
                    assert_eq!(app.order_chat_input, "draft\n  untouched");
                    assert_eq!(app.order_chat_draft_owner, Some((Uuid::nil(), channel)));
                    assert_eq!(app.order_chat_input_enabled, input_enabled);
                    assert_eq!(app.active_user_chat_channel, channel);
                    assert!(app.mode.user_my_trades_interactive());
                }
            }
        }
    }

    #[test]
    fn my_trades_copy_cancels_when_sidebar_reorders_or_channel_changes() {
        for reorder in [true, false] {
            let mut app = app_with_order_messages(UserChatChannel::Peer);
            enter_selection(&mut app);
            if reorder {
                let mut other = app.my_trades_maker_book[0].clone();
                other.order_id = Uuid::from_u128(2).to_string();
                other.trade_index = Some(2);
                app.my_trades_maker_book.insert(0, other);
            } else {
                app.active_user_chat_channel = UserChatChannel::Solver;
            }
            assert_ne!(
                live_order_chat_draft_target(&app),
                Some((Uuid::nil(), UserChatChannel::Peer))
            );
            press(&mut app, KeyCode::Enter);
            assert!(app.chat_copy_session.is_none());
            sync_order_chat_draft_to_live_target(&mut app);
            assert!(app.order_chat_input.is_empty());
            assert!(app.order_chat_draft_owner.is_none());
        }
    }

    #[test]
    fn my_trades_copy_failure_and_empty_chat_leave_composer_untouched() {
        let mut app = app_with_order_messages(UserChatChannel::Solver);
        enter_selection(&mut app);
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |_| false
        ));
        assert_eq!(feedback_text(&app), Some("Clipboard unavailable"));
        app.user_dispute_chats.clear();
        enter_selection(&mut app);
        assert!(app.chat_copy_session.is_none());
        assert_eq!(feedback_text(&app), Some("No messages to copy"));
        assert_eq!(app.order_chat_input, "draft\n  untouched");
        assert_eq!(
            app.order_chat_draft_owner,
            Some((Uuid::nil(), UserChatChannel::Solver))
        );
    }

    #[test]
    fn my_trades_copy_navigation_copies_last_text_or_attachment_filename() {
        for channel in [UserChatChannel::Peer, UserChatChannel::Solver] {
            for attachment in [false, true] {
                let mut app = app_with_order_messages(channel);
                let chats = if channel == UserChatChannel::Peer {
                    &mut app.order_chats
                } else {
                    &mut app.user_dispute_chats
                };
                let message = &mut chats.get_mut(&Uuid::nil().to_string()).unwrap()[1];
                message.content = "  last\ntext\t  ".into();
                if attachment {
                    message.attachment = Some(ChatAttachment {
                        blossom_url: "https://example.com/blob".into(),
                        filename: "receipt.txt".into(),
                        mime_type: None,
                        file_type: ChatAttachmentType::File,
                        decryption_key: None,
                    });
                }
                let target = focused_target(&app).unwrap();
                let parts: Vec<_> = messages(&app, &target)
                    .take(2)
                    .filter_map(copy_text_for_message)
                    .collect();
                let expected = parts.join("\n\n");
                enter_selection(&mut app);
                press(&mut app, KeyCode::Up);
                assert_eq!(selected_index(&app), Some(0));
                assert_eq!(selected_range(&app), Some(0..=0));
                press_shift(&mut app, KeyCode::Down);
                press_shift(&mut app, KeyCode::Down);
                assert_eq!(selected_index(&app), Some(1));
                assert_eq!(selected_range(&app), Some(0..=1));
                assert!(handle_key_with(
                    &mut app,
                    &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    |text| {
                        assert_eq!(text, expected);
                        true
                    }
                ));
            }
        }
    }

    #[test]
    fn my_trades_copy_ignores_popups_and_cancels_removed_order() {
        let mut app = app_with_order_messages(UserChatChannel::Peer);
        app.mode = UiMode::HelpPopup(app.active_tab, Box::new(app.mode.clone()));
        assert!(!handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            |_| false
        ));
        assert!(app.chat_copy_session.is_none());
        app.mode = UiMode::UserMode(UserMode::Normal);
        enter_selection(&mut app);
        app.my_trades_maker_book.clear();
        press(&mut app, KeyCode::Enter);
        assert!(app.chat_copy_session.is_none());
        sync_order_chat_draft_to_live_target(&mut app);
        assert!(app.order_chat_input.is_empty());
        assert!(app.order_chat_draft_owner.is_none());
    }

    pub(crate) fn app_with_messages() -> AppState {
        let mut app = AppState::new(UserRole::Admin);
        app.active_tab = Tab::Admin(AdminTab::DisputesInProgress);
        app.mode = UiMode::AdminMode(AdminMode::ManagingDispute);
        app.selected_dispute_id = Some("dispute".into());
        app.admin_disputes_in_progress.push(AdminDispute {
            dispute_id: "dispute".into(),
            status: Some("in-progress".into()),
            ..Default::default()
        });
        app.admin_chat_input = "draft\n  untouched".into();
        app.admin_chat_draft_owner = Some(("dispute".into(), ChatParty::Buyer));
        app.admin_dispute_chats.insert(
            "dispute".into(),
            vec![
                message(ChatSender::Seller, "hidden"),
                message(ChatSender::Buyer, "  first\nsecond\tline  "),
                message(ChatSender::Admin, "last"),
            ],
        );
        app
    }

    fn message(sender: ChatSender, content: &str) -> DisputeChatMessage {
        DisputeChatMessage {
            sender,
            content: content.into(),
            timestamp: 1,
            target_party: None,
            attachment: None,
        }
    }

    fn press(app: &mut AppState, code: KeyCode) {
        assert!(handle_key_with(
            app,
            &KeyEvent::new(code, KeyModifiers::NONE),
            |_| { panic!("unexpected clipboard write") }
        ));
    }

    fn enter_selection(app: &mut AppState) {
        assert!(handle_key_with(
            app,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            |_| panic!("entry must not copy"),
        ));
    }

    fn press_shift(app: &mut AppState, code: KeyCode) {
        assert!(handle_key_with(
            app,
            &KeyEvent::new(code, KeyModifiers::SHIFT),
            |_| { panic!("unexpected clipboard write") }
        ));
    }

    #[test]
    fn plain_arrows_move_a_single_highlighted_message() {
        let mut app = app_with_messages();
        enter_selection(&mut app);
        assert_eq!(selected_range(&app), Some(0..=0));
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_index(&app), Some(1));
        assert_eq!(selected_range(&app), Some(1..=1));
        press(&mut app, KeyCode::Up);
        assert_eq!(selected_range(&app), Some(0..=0));
    }

    #[test]
    fn enter_after_plain_down_copies_only_the_highlighted_message() {
        let mut app = app_with_messages();
        enter_selection(&mut app);
        press(&mut app, KeyCode::Down);
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, "last");
                true
            }
        ));
        assert_eq!(feedback_text(&app), Some("Copied to clipboard"));
    }

    #[test]
    fn shift_arrows_extend_the_range_and_a_plain_arrow_collapses_it() {
        let mut app = app_with_messages();
        app.admin_dispute_chats
            .get_mut("dispute")
            .unwrap()
            .push(message(ChatSender::Buyer, "third"));
        enter_selection(&mut app);
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_range(&app), Some(1..=1));
        press_shift(&mut app, KeyCode::Down);
        assert_eq!(selected_index(&app), Some(2));
        assert_eq!(selected_range(&app), Some(1..=2));
        press_shift(&mut app, KeyCode::Up);
        press_shift(&mut app, KeyCode::Up);
        assert_eq!(selected_index(&app), Some(0));
        assert_eq!(selected_range(&app), Some(0..=1));
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, "  first\nsecond\tline  \n\nlast");
                true
            }
        ));

        let mut app = app_with_messages();
        enter_selection(&mut app);
        press_shift(&mut app, KeyCode::Down);
        assert_eq!(selected_range(&app), Some(0..=1));
        press(&mut app, KeyCode::Up);
        assert_eq!(selected_range(&app), Some(0..=0));
    }

    #[test]
    fn chat_copy_filters_navigates_and_preserves_both_input_layers() {
        for input_enabled in [false, true] {
            let mut app = app_with_messages();
            app.admin_chat_input_enabled = input_enabled;
            enter_selection(&mut app);
            assert_eq!(selected_index(&app), Some(0));
            assert_eq!(selected_range(&app), Some(0..=0));
            press(&mut app, KeyCode::Up);
            assert_eq!(selected_index(&app), Some(0));
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::Down);
            assert_eq!(selected_index(&app), Some(1));
            assert_eq!(selected_range(&app), Some(1..=1));
            press(&mut app, KeyCode::Tab);
            press(&mut app, KeyCode::Char('x'));
            enter_selection(&mut app);
            assert_eq!(selected_index(&app), Some(1));
            assert_eq!(selected_range(&app), Some(1..=1));
            press(&mut app, KeyCode::Esc);
            assert!(app.chat_copy_session.is_none());
            assert_eq!(app.admin_chat_input, "draft\n  untouched");
            assert_eq!(app.admin_chat_input_enabled, input_enabled);
            assert_eq!(app.active_chat_party, ChatParty::Buyer);
            assert!(matches!(
                app.mode,
                UiMode::AdminMode(AdminMode::ManagingDispute)
            ));
        }
    }

    #[test]
    fn chat_copy_range_shrinks_back_toward_anchor() {
        let mut app = app_with_messages();
        enter_selection(&mut app);
        press_shift(&mut app, KeyCode::Down);
        assert_eq!(selected_range(&app), Some(0..=1));
        press_shift(&mut app, KeyCode::Up);
        assert_eq!(selected_index(&app), Some(0));
        assert_eq!(selected_range(&app), Some(0..=0));
        let expected = copy_text_for_message(CopyMessage {
            event_id: None,
            content: "  first\nsecond\tline  ",
            attachment: None,
            timestamp: 1,
            sender: 1,
        })
        .unwrap();
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, expected);
                true
            }
        ));
    }

    #[test]
    fn chat_copy_starts_at_first_message_even_when_scrolled() {
        use ratatui::layout::Position;

        let mut app = app_with_messages();
        app.admin_chat_line_starts = vec![0, 12];
        app.admin_chat_scrollview_state
            .set_offset(Position::new(0, 12));
        enter_selection(&mut app);
        assert_eq!(selected_index(&app), Some(0));
        assert_eq!(selected_range(&app), Some(0..=0));
    }

    #[test]
    fn chat_copy_cancels_when_interior_range_message_changes() {
        let mut app = app_with_messages();
        app.admin_dispute_chats
            .get_mut("dispute")
            .unwrap()
            .push(message(ChatSender::Buyer, "middle"));
        app.admin_dispute_chats
            .get_mut("dispute")
            .unwrap()
            .push(message(ChatSender::Admin, "tail"));
        // Buyer pane: first, middle, last(admin), middle(buyer), tail(admin)
        // Visible for buyer: Buyer "first", Admin "last", Buyer "middle", Admin "tail"
        enter_selection(&mut app);
        press_shift(&mut app, KeyCode::Down);
        press_shift(&mut app, KeyCode::Down);
        assert_eq!(selected_range(&app), Some(0..=2));
        // Replace interior display index 1 (Admin "last" = vec index 2).
        app.admin_dispute_chats.get_mut("dispute").unwrap()[2].content = "replaced".into();
        validate_selection(&mut app);
        assert!(app.chat_copy_session.is_none());
        assert!(app.chat_copy_cancelled);
    }

    #[test]
    fn chat_copy_range_with_empty_filename_writes_nothing() {
        let mut app = app_with_messages();
        app.admin_dispute_chats.get_mut("dispute").unwrap()[2].attachment = Some(ChatAttachment {
            blossom_url: "https://example.com/blob".into(),
            filename: String::new(),
            mime_type: None,
            file_type: ChatAttachmentType::File,
            decryption_key: None,
        });
        enter_selection(&mut app);
        press_shift(&mut app, KeyCode::Down);
        assert_eq!(selected_range(&app), Some(0..=1));
        let mut copied = false;
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |_| {
                copied = true;
                true
            }
        ));
        assert!(!copied);
        assert_eq!(feedback_text(&app), Some("No filename to copy"));
    }

    #[test]
    fn chat_copy_exact_text_and_failure_feedback() {
        for succeeds in [true, false] {
            let mut app = app_with_messages();
            let expected = copy_text_for_message(CopyMessage {
                event_id: None,
                content: "  first\nsecond\tline  ",
                attachment: None,
                timestamp: 1,
                sender: 1,
            })
            .unwrap();
            enter_selection(&mut app);
            assert!(handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                |text| {
                    assert_eq!(text, expected);
                    succeeds
                },
            ));
            assert!(app.chat_copy_session.is_none());
            assert_eq!(
                feedback_text(&app),
                Some(if succeeds {
                    "Copied to clipboard"
                } else {
                    "Clipboard unavailable"
                })
            );
            assert_eq!(app.admin_chat_input, "draft\n  untouched");
            assert!(app.admin_chat_input_enabled);
            handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Null, KeyModifiers::NONE),
                |_| false,
            );
            assert!(feedback_text(&app).is_none());
        }
    }

    #[test]
    fn chat_copy_attachment_copies_filename_not_placeholder_or_url() {
        let mut app = app_with_messages();
        app.admin_dispute_chats.get_mut("dispute").unwrap()[1].attachment = Some(ChatAttachment {
            blossom_url: "https://example.com/blob".into(),
            filename: "receipt.txt".into(),
            mime_type: None,
            file_type: ChatAttachmentType::File,
            decryption_key: None,
        });
        enter_selection(&mut app);
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, "receipt.txt");
                true
            }
        ));
    }

    #[test]
    fn chat_copy_append_preserves_selection_but_replacement_cancels() {
        let mut app = app_with_messages();
        enter_selection(&mut app);
        app.admin_dispute_chats
            .get_mut("dispute")
            .unwrap()
            .push(message(ChatSender::Buyer, "new"));
        validate_selection(&mut app);
        assert_eq!(selected_index(&app), Some(0));
        app.admin_dispute_chats.get_mut("dispute").unwrap()[1].content = "replacement".into();
        press(&mut app, KeyCode::Enter);
        assert!(app.chat_copy_session.is_none());
        assert_eq!(app.admin_chat_input, "draft\n  untouched");
    }

    #[test]
    fn chat_copy_empty_chat_and_unfocused_views_are_safe() {
        let mut app = app_with_messages();
        app.admin_dispute_chats.clear();
        enter_selection(&mut app);
        assert!(app.chat_copy_session.is_none());
        assert_eq!(feedback_text(&app), Some("No messages to copy"));
        app.mode = UiMode::HelpPopup(app.active_tab, Box::new(UiMode::Normal));
        assert!(!handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            |_| false
        ));
    }

    #[test]
    fn chat_copy_context_changes_cancel_without_copying() {
        for change in 0..5 {
            let mut app = app_with_messages();
            enter_selection(&mut app);
            match change {
                0 => app.active_chat_party = ChatParty::Seller,
                1 => app.admin_show_solver_dms = true,
                2 => app.selected_dispute_id = None,
                3 => app.admin_disputes_in_progress[0].status = Some("settled".into()),
                _ => app.active_tab = Tab::Admin(AdminTab::Observer),
            }
            press(&mut app, KeyCode::Enter);
            assert!(app.chat_copy_session.is_none());
            assert_eq!(app.admin_chat_input, "draft\n  untouched");
        }
    }

    #[test]
    fn chat_copy_seller_projection_excludes_targeted_admin_messages() {
        let mut app = app_with_messages();
        app.active_chat_party = ChatParty::Seller;
        app.admin_dispute_chats.get_mut("dispute").unwrap()[2].target_party =
            Some(ChatParty::Buyer);
        enter_selection(&mut app);
        press(&mut app, KeyCode::Down);
        assert_eq!(selected_index(&app), Some(0));
        let expected = copy_text_for_message(CopyMessage {
            event_id: None,
            content: "hidden",
            attachment: None,
            timestamp: 1,
            sender: 2,
        })
        .unwrap();
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, expected);
                true
            }
        ));
    }

    #[test]
    fn chat_copy_preserves_unicode_newlines_and_long_tokens() {
        let mut app = app_with_messages();
        let body = format!("  \u{00e9}\u{754c}\r\n\t{}\n", "lnbc1".repeat(200));
        app.admin_dispute_chats.get_mut("dispute").unwrap()[1].content = body.clone();
        let expected = copy_text_for_message(CopyMessage {
            event_id: None,
            content: &body,
            attachment: None,
            timestamp: 1,
            sender: 1,
        })
        .unwrap();
        enter_selection(&mut app);
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, expected);
                true
            }
        ));
    }

    #[test]
    fn chat_copy_missing_attachment_filename_does_not_write_clipboard() {
        let mut app = app_with_messages();
        app.admin_dispute_chats.get_mut("dispute").unwrap()[1].attachment = Some(ChatAttachment {
            blossom_url: "https://example.com/blob".into(),
            filename: String::new(),
            mime_type: None,
            file_type: ChatAttachmentType::File,
            decryption_key: None,
        });
        enter_selection(&mut app);
        press(&mut app, KeyCode::Enter);
        assert_eq!(feedback_text(&app), Some("No filename to copy"));
    }
}
