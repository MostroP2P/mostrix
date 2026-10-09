use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ui::helpers::{message_visible_for_party, selected_filtered_dispute};
use crate::ui::key_handler::chat_helpers::live_order_chat_draft_target;
use crate::ui::key_handler::handle_clipboard_copy;
use crate::ui::{
    AdminMode, AdminTab, AppState, ChatAttachment, ChatParty, ChatSender, DisputeFilter, Tab,
    UiMode, UserChatChannel, UserChatSender, UserRole, UserTab,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatCopyTarget {
    Dispute { id: String, party: ChatParty },
    Order { id: Uuid, channel: UserChatChannel },
    SolverDm { dispute_id: String },
    Observer { generation: u64 },
}

pub(crate) struct ChatCopySession {
    target: ChatCopyTarget,
    selected_index: usize,
    fingerprint: [u8; 32],
    event_id: Option<String>,
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

pub(crate) fn validate_selection(app: &mut AppState) {
    let Some(mut session) = app.chat_copy_session.take() else {
        return;
    };
    let index = if let Some(event_id) = &session.event_id {
        messages(app, &session.target)
            .position(|message| message.event_id == Some(event_id.as_str()))
    } else {
        Some(session.selected_index)
    };
    let valid_index = index.filter(|index| {
        focused_target(app).as_ref() == Some(&session.target)
            && messages(app, &session.target)
                .nth(*index)
                .is_some_and(|message| fingerprint(message) == session.fingerprint)
    });
    if let Some(index) = valid_index {
        session.selected_index = index;
        app.chat_copy_session = Some(session);
    } else {
        app.chat_copy_cancelled = true;
    }
}

pub(crate) fn selected_index(app: &AppState) -> Option<usize> {
    app.chat_copy_session
        .as_ref()
        .map(|session| session.selected_index)
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

pub(crate) fn handle_key(app: &mut AppState, key: &KeyEvent) -> bool {
    handle_key_with(app, key, handle_clipboard_copy)
}

pub(crate) fn handle_key_with(
    app: &mut AppState,
    key: &KeyEvent,
    copy: impl FnOnce(String) -> bool,
) -> bool {
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
                let text = messages(app, &session.target)
                    .nth(session.selected_index)
                    .and_then(|message| match &message.attachment {
                        Some(attachment) => {
                            (!attachment.filename.is_empty()).then(|| attachment.filename.clone())
                        }
                        None => Some(message.content.to_owned()),
                    });
                let feedback = match text {
                    Some(text) => {
                        if copy(text) {
                            "Copied to clipboard"
                        } else {
                            "Clipboard unavailable"
                        }
                    }
                    None => "No filename to copy",
                };
                app.chat_copy_feedback = Some(ChatCopyFeedback {
                    target: session.target,
                    text: feedback,
                });
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
                    selected_index: 0,
                    fingerprint,
                    event_id,
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
                let expected = app.observer_messages[selected].content.clone();
                let generation = app.observer_fetch_generation;
                enter_selection(&mut app);
                press(&mut app, KeyCode::Up);
                assert_eq!(selected_index(&app), Some(0));
                if selected == 1 {
                    press(&mut app, KeyCode::Down);
                    press(&mut app, KeyCode::Down);
                }
                assert_eq!(selected_index(&app), Some(selected));
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
        enter_selection(&mut app);
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, "receipt.txt");
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
        let expected = app.solver_dms["dispute"][1].text.clone();
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
                    let expected = match channel {
                        UserChatChannel::Peer => {
                            app.order_chats[&Uuid::nil().to_string()][0].content.clone()
                        }
                        UserChatChannel::Solver => app.user_dispute_chats[&Uuid::nil().to_string()]
                            [0]
                        .content
                        .clone(),
                    };
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
                enter_selection(&mut app);
                press(&mut app, KeyCode::Up);
                assert_eq!(selected_index(&app), Some(0));
                press(&mut app, KeyCode::Down);
                press(&mut app, KeyCode::Down);
                assert_eq!(selected_index(&app), Some(1));
                assert!(handle_key_with(
                    &mut app,
                    &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    |text| {
                        assert_eq!(
                            text,
                            if attachment {
                                "receipt.txt"
                            } else {
                                "  last\ntext\t  "
                            }
                        );
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

    #[test]
    fn chat_copy_filters_navigates_and_preserves_both_input_layers() {
        for input_enabled in [false, true] {
            let mut app = app_with_messages();
            app.admin_chat_input_enabled = input_enabled;
            enter_selection(&mut app);
            assert_eq!(selected_index(&app), Some(0));
            press(&mut app, KeyCode::Up);
            assert_eq!(selected_index(&app), Some(0));
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::Down);
            assert_eq!(selected_index(&app), Some(1));
            press(&mut app, KeyCode::Tab);
            press(&mut app, KeyCode::Char('x'));
            enter_selection(&mut app);
            assert_eq!(selected_index(&app), Some(1));
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
    fn chat_copy_exact_text_and_failure_feedback() {
        for succeeds in [true, false] {
            let mut app = app_with_messages();
            enter_selection(&mut app);
            assert!(handle_key_with(
                &mut app,
                &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                |text| {
                    assert_eq!(text, "  first\nsecond\tline  ");
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
        assert!(handle_key_with(
            &mut app,
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            |text| {
                assert_eq!(text, "hidden");
                true
            }
        ));
    }

    #[test]
    fn chat_copy_preserves_unicode_newlines_and_long_tokens() {
        let mut app = app_with_messages();
        let expected = format!("  \u{00e9}\u{754c}\r\n\t{}\n", "lnbc1".repeat(200));
        app.admin_dispute_chats.get_mut("dispute").unwrap()[1].content = expected.clone();
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
