//! My Trades send-attachment file picker (Ctrl+O) using `ratatui-explorer`.

use std::path::PathBuf;

use anyhow::Result;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::FrameExt as _;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui_explorer::{File, FileExplorer, FileExplorerBuilder, Theme};

use crate::ui::helpers::create_centered_popup;
use crate::ui::{AppState, UiMode, UserMode, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::attachment_extension_allowed;

pub const SEND_ATTACHMENT_PICKER_HINT: &str =
    "Enter: Send file  |  Esc: Cancel  |  h/j/k/l: Navigate  |  Ctrl+H: hidden";

fn picker_start_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn mostrix_explorer_theme() -> Theme {
    Theme::default()
        .add_default_title()
        .with_title_bottom(|fe| format!("[{} items]", fe.files().len()).into())
        .with_block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR)),
        )
        .with_highlight_item_style(
            Style::default()
                .fg(PRIMARY_COLOR)
                .bg(BACKGROUND_COLOR)
                .add_modifier(Modifier::BOLD),
        )
        .with_highlight_dir_style(
            Style::default()
                .fg(Color::Cyan)
                .bg(BACKGROUND_COLOR)
                .add_modifier(Modifier::BOLD),
        )
        .with_highlight_symbol("> ")
}

/// Builds a file explorer filtered to attachment-allowed extensions (dirs always visible).
pub fn build_send_attachment_explorer() -> Result<FileExplorer> {
    let start = picker_start_dir();
    FileExplorerBuilder::default()
        .working_dir(&start)
        .theme(mostrix_explorer_theme())
        .filter_map(|file| {
            if file.is_dir || attachment_extension_allowed(&file.path) {
                Some(file)
            } else {
                None
            }
        })
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// True when the highlighted entry is a regular file (not `..` or a directory).
pub fn explorer_selection_is_sendable_file(file: &File) -> bool {
    !file.is_dir && file.name != ".."
}

/// Opens the send-attachment picker for `order_id`.
pub fn open_user_send_attachment_picker(app: &mut AppState, order_id: String) -> Result<()> {
    let explorer = build_send_attachment_explorer()?;
    app.user_send_attachment_explorer = Some(explorer);
    app.mode = UiMode::UserSendAttachmentPicker(order_id);
    Ok(())
}

/// Closes the picker and returns to My Trades normal mode.
pub fn close_user_send_attachment_picker(app: &mut AppState) {
    app.user_send_attachment_explorer = None;
    app.mode = UiMode::UserMode(UserMode::Normal);
}

/// Renders the file explorer modal when picker mode is active.
pub fn render_user_send_attachment_picker(f: &mut ratatui::Frame, app: &AppState) {
    let Some(explorer) = app.user_send_attachment_explorer.as_ref() else {
        return;
    };

    let area = f.area();
    let popup_width = area.width.saturating_mul(4) / 5;
    let popup_height = area.height.saturating_mul(7) / 10;
    let popup = create_centered_popup(area, popup_width.max(40), popup_height.max(12));

    f.render_widget(Clear, popup);
    f.render_widget_ref(explorer.widget(), popup);

    if popup.height > 2 {
        let hint_area = ratatui::layout::Rect {
            x: popup.x,
            y: popup.y + popup.height.saturating_sub(1),
            width: popup.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                SEND_ATTACHMENT_PICKER_HINT,
                Style::default().fg(Color::DarkGray).bg(BACKGROUND_COLOR),
            )),
            hint_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn build_explorer_filters_extensions_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("mostrix_picker_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ok.png"), b"x").unwrap();
        fs::write(dir.join("bad.exe"), b"x").unwrap();

        let explorer = FileExplorerBuilder::default()
            .working_dir(&dir)
            .filter_map(|file| {
                if file.is_dir || attachment_extension_allowed(&file.path) {
                    Some(file)
                } else {
                    None
                }
            })
            .build()
            .unwrap();

        let names: Vec<String> = explorer.files().iter().map(|f| f.name.clone()).collect();
        assert!(names.iter().any(|n| n == "ok.png"));
        assert!(!names.iter().any(|n| n == "bad.exe"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn explorer_selection_is_sendable_file_skips_parent_and_dirs() {
        let parent = File {
            name: "..".into(),
            path: PathBuf::from("/"),
            is_dir: true,
            is_hidden: false,
            file_type: None,
        };
        assert!(!explorer_selection_is_sendable_file(&parent));

        let file = File {
            name: "x.png".into(),
            path: PathBuf::from("/x.png"),
            is_dir: false,
            is_hidden: false,
            file_type: None,
        };
        assert!(explorer_selection_is_sendable_file(&file));
    }
}
