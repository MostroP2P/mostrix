use std::time::Instant;

use ratatui::layout::{Alignment, Constraint, Flex, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::ui::helpers::render_centered_lines;
use crate::ui::{BACKGROUND_COLOR, PRIMARY_COLOR};

pub const SPLASH_TICK_MS: u64 = 150;
pub const SPLASH_DOT_CYCLE_MS: u64 = 400;
pub const SPLASH_MIN_DISPLAY_MS: u64 = 3000;

/// One animated dot (diamond), including a leading space separator.
pub const LOADING_DOT_UNIT: &str = " <>";

/// Multi-line Mostro wordmark (from project logo); dots animate on the last row.
pub const MOSTRO_LOADING_LINES: &[&str] = &[
    "                        __         .__         .__         .__                    .___.__",
    "  _____   ____  _______╱  │________│__│__  ___ │__│ ______ │  │   _________     __│ _╱│__│ ____    ____",
    " ╱     ╲ ╱  _ ╲╱  ___╱╲   __╲_  __ ╲  ╲  ╲╱  ╱ │  │╱  ___╱ │  │  ╱  _ ╲__  ╲   ╱ __ │ │  │╱    ╲  ╱ ___╲",
    "│  Y Y  (  <_> )___ ╲  │  │  │  │ ╲╱  │>    <  │  │╲___ ╲  │  │_(  <_> ) __ ╲_╱ ╱_╱ │ │  │   │  ╲╱ ╱_╱  >",
    "│__│_│  ╱╲____╱____  > │__│  │__│  │__╱__╱╲_ ╲ │__╱____  > │____╱╲____(____  ╱╲____ │ │__│___│  ╱╲___  ╱",
    "      ╲╱           ╲╱                       ╲╱         ╲╱                  ╲╱      ╲╱         ╲╱╱_____╱",
];

/// Width reserved on the last line for up to four dots.
const MAX_DOT_SUFFIX_CHARS: usize = LOADING_DOT_UNIT.len() * 4;

/// Last line index in [`MOSTRO_LOADING_LINES`] that receives the animated dots.
const LOADING_DOTS_LINE_INDEX: usize = 5;

fn max_raw_art_width() -> usize {
    MOSTRO_LOADING_LINES
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

/// Padded width of every splash frame line (constant across dot counts 1–4).
pub fn logo_line_width() -> usize {
    let raw_max = max_raw_art_width();
    let last_raw = MOSTRO_LOADING_LINES[LOADING_DOTS_LINE_INDEX]
        .chars()
        .count();
    raw_max.max(last_raw + MAX_DOT_SUFFIX_CHARS)
}

fn pad_line_to_width(line: &str, width: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() >= width {
        chars.into_iter().take(width).collect()
    } else {
        let mut out = line.to_string();
        out.push_str(&" ".repeat(width - chars.len()));
        out
    }
}

/// Builds the suffix for `dot_count` dots (1–4). Values outside 1..=4 clamp to 1..=4.
pub fn dot_suffix(dot_count: u8) -> String {
    let n = dot_count.clamp(1, 4) as usize;
    LOADING_DOT_UNIT.repeat(n)
}

/// Maps elapsed time since splash start to a dot count in 1..=4.
pub fn dot_count_from_elapsed(started: &Instant) -> u8 {
    let elapsed = started.elapsed().as_millis() as u64;
    let frame = (elapsed / SPLASH_DOT_CYCLE_MS) % 4;
    (frame + 1) as u8
}

/// Returns wordmark lines with dots appended on the last row (fixed total width).
pub fn splash_lines_with_dots(dot_count: u8) -> Vec<String> {
    let width = logo_line_width();
    let suffix = dot_suffix(dot_count);
    let suffix_len = suffix.chars().count();

    MOSTRO_LOADING_LINES
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            if idx == LOADING_DOTS_LINE_INDEX {
                let base = pad_line_to_width(line, width.saturating_sub(suffix_len));
                format!("{base}{suffix}")
            } else {
                pad_line_to_width(line, width)
            }
        })
        .collect()
}

fn splash_fits_wordmark(terminal_width: u16) -> bool {
    terminal_width >= logo_line_width() as u16
}

fn fill_splash_background(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    f.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND_COLOR)),
        area,
    );
}

fn style_loading_line(line: &str) -> Vec<Span<'static>> {
    line.chars()
        .map(|c| {
            let highlight = matches!(
                c,
                '/' | '\\' | '_' | '<' | '>' | '|' | '=' | '│' | '╱' | '╲' | '▀' | '▄'
            );
            if highlight {
                Span::styled(
                    c.to_string(),
                    Style::default()
                        .fg(PRIMARY_COLOR)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::styled(c.to_string(), Style::default().fg(Color::White))
            }
        })
        .collect()
}

fn render_compact_splash(f: &mut ratatui::Frame, dot_count: u8, phase: &str) {
    let area = f.area();
    fill_splash_background(f, area);

    let text = format!(
        "mostro is loading{}{}",
        dot_suffix(dot_count),
        if phase.is_empty() {
            String::new()
        } else {
            format!("\n\n{phase}")
        }
    );

    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::White));

    let [block] = Layout::vertical([Constraint::Min(1)])
        .flex(Flex::Center)
        .areas(area);
    f.render_widget(paragraph, block);
}

/// Full-screen startup splash with animated dots and optional phase subtitle.
pub fn render_startup_splash(f: &mut ratatui::Frame, dot_count: u8, phase: &str) {
    let area = f.area();
    fill_splash_background(f, area);

    if !splash_fits_wordmark(area.width) {
        render_compact_splash(f, dot_count, phase);
        return;
    }

    let art_owned = splash_lines_with_dots(dot_count);
    let art_lines: Vec<&str> = art_owned.iter().map(String::as_str).collect();

    let phase_height: u16 = if phase.is_empty() { 0 } else { 2 };
    let art_height = art_lines.len() as u16;
    let total_height = art_height.saturating_add(phase_height);

    let [center_block] = Layout::vertical([Constraint::Length(total_height)])
        .flex(Flex::Center)
        .areas(area);

    let chunks = if phase.is_empty() {
        Layout::vertical([Constraint::Length(art_height)]).split(center_block)
    } else {
        Layout::vertical([
            Constraint::Length(art_height),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(center_block)
    };

    render_centered_lines(f, chunks[0], &art_lines, style_loading_line);

    if !phase.is_empty() && chunks.len() > 2 {
        let phase_line = Line::from(Span::styled(phase, Style::default().fg(Color::Gray)));
        f.render_widget(
            Paragraph::new(phase_line).alignment(Alignment::Center),
            chunks[2],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_lines_share_max_width() {
        let w = logo_line_width();
        for line in splash_lines_with_dots(4) {
            assert_eq!(line.chars().count(), w);
        }
    }

    #[test]
    fn dot_suffix_lengths_are_monotonic() {
        let w1 = dot_suffix(1).chars().count();
        let w4 = dot_suffix(4).chars().count();
        assert!(w4 > w1);
        assert_eq!(w4, LOADING_DOT_UNIT.len() * 4);
    }

    #[test]
    fn dot_count_from_elapsed_in_range() {
        let started = Instant::now();
        let n = dot_count_from_elapsed(&started);
        assert!((1..=4).contains(&n));
    }

    #[test]
    fn splash_last_line_width_stable_across_dot_counts() {
        let w = logo_line_width();
        for dots in 1..=4u8 {
            let lines = splash_lines_with_dots(dots);
            assert_eq!(lines[LOADING_DOTS_LINE_INDEX].chars().count(), w);
        }
    }
}
