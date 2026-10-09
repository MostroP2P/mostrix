use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sha2::{Digest, Sha256};

use crate::ui::helpers::{message_visible_for_party, selected_filtered_dispute};
use crate::ui::key_handler::handle_clipboard_copy;
use crate::ui::{
    AdminMode, AdminTab, AppState, ChatParty, ChatSender, DisputeChatMessage, DisputeFilter, Tab,
    UiMode, UserRole,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatCopyTarget {
    Dispute { id: String, party: ChatParty },
}

pub(crate) struct ChatCopySession {
    target: ChatCopyTarget,
    selected_index: usize,
    fingerprint: [u8; 32],
}

pub(crate) struct ChatCopyFeedback {
    target: ChatCopyTarget,
    text: &'static str,
}

fn focused_target(app: &AppState) -> Option<ChatCopyTarget> {
    if app.user_role != UserRole::Admin
        || app.active_tab != Tab::Admin(AdminTab::DisputesInProgress)
        || !matches!(app.mode, UiMode::AdminMode(AdminMode::ManagingDispute))
        || app.admin_show_solver_dms
        || app.dispute_filter != DisputeFilter::InProgress
    {
        return None;
    }
    selected_filtered_dispute(app)
        .filter(|dispute| {
            !dispute.is_finalized()
                && app.selected_dispute_id.as_deref() == Some(dispute.dispute_id.as_str())
        })
        .map(|dispute| ChatCopyTarget::Dispute {
            id: dispute.dispute_id,
            party: app.active_chat_party,
        })
}

fn messages<'a>(
    app: &'a AppState,
    target: &ChatCopyTarget,
) -> impl Iterator<Item = &'a DisputeChatMessage> {
    let ChatCopyTarget::Dispute { id, party } = target;
    let party = *party;
    app.admin_dispute_chats
        .get(id)
        .into_iter()
        .flatten()
        .filter(move |message| message_visible_for_party(message, party))
}

fn fingerprint(message: &DisputeChatMessage) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(message.timestamp.to_le_bytes());
    digest.update([match message.sender {
        ChatSender::Admin => 0,
        ChatSender::Buyer => 1,
        ChatSender::Seller => 2,
    }]);
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
    let valid = app.chat_copy_session.as_ref().is_none_or(|session| {
        focused_target(app).as_ref() == Some(&session.target)
            && messages(app, &session.target)
                .nth(session.selected_index)
                .is_some_and(|message| fingerprint(message) == session.fingerprint)
    });
    if !valid {
        app.chat_copy_session = None;
    }
}

pub(crate) fn selected_index(app: &AppState) -> Option<usize> {
    app.chat_copy_session
        .as_ref()
        .map(|session| session.selected_index)
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
    let was_selecting = app.chat_copy_session.is_some();
    validate_selection(app);
    if was_selecting && app.chat_copy_session.is_none() {
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
                        None => Some(message.content.clone()),
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
            let first = messages(app, &target).next().map(fingerprint);
            if let Some(fingerprint) = first {
                app.chat_copy_session = Some(ChatCopySession {
                    target,
                    selected_index: 0,
                    fingerprint,
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
    use crate::ui::{ChatAttachment, ChatAttachmentType};

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
