use std::sync::OnceLock;
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

/// Startup wordmark loaded from [`static/startup-logo.txt`](../../static/startup-logo.txt).
const STARTUP_LOGO_RAW: &str = include_str!("../../static/startup-logo.txt");

struct StartupLogoArt {
    lines: Vec<String>,
    dots_line_index: usize,
}

static STARTUP_LOGO: OnceLock<StartupLogoArt> = OnceLock::new();

fn parse_startup_logo(raw: &str) -> StartupLogoArt {
    let mut lines: Vec<String> = raw.lines().map(str::trim_end).map(str::to_string).collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    let dots_line_index = lines.iter().rposition(|l| !l.is_empty()).unwrap_or(0);
    StartupLogoArt {
        lines,
        dots_line_index,
    }
}

fn startup_logo() -> &'static StartupLogoArt {
    STARTUP_LOGO.get_or_init(|| parse_startup_logo(STARTUP_LOGO_RAW))
}

/// Width reserved on the last line for up to four dots.
const MAX_DOT_SUFFIX_CHARS: usize = LOADING_DOT_UNIT.len() * 4;

fn max_raw_art_width(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

/// Padded width of every splash frame line (constant across dot counts 1–4).
pub fn logo_line_width() -> usize {
    let logo = startup_logo();
    let raw_max = max_raw_art_width(&logo.lines);
    let last_raw = logo.lines[logo.dots_line_index].chars().count();
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
    let logo = startup_logo();
    let width = logo_line_width();
    let suffix = dot_suffix(dot_count);
    let suffix_len = suffix.chars().count();

    logo.lines
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            if idx == logo.dots_line_index {
                let base = pad_line_to_width(line, width.saturating_sub(suffix_len));
                format!("{base}{suffix}")
            } else {
                pad_line_to_width(line, width)
            }
        })
        .collect()
}

fn splash_fits_wordmark(terminal_width: u16, terminal_height: u16) -> bool {
    let logo = startup_logo();
    let phase_reserve: u16 = 2;
    terminal_width >= logo_line_width() as u16
        && terminal_height >= logo.lines.len() as u16 + phase_reserve
}

fn fill_splash_background(f: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    f.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND_COLOR)),
        area,
    );
}

fn is_logo_highlight_char(c: char) -> bool {
    matches!(
        c,
        '*' | '+'
            | '#'
            | ':'
            | '.'
            | '-'
            | '='
            | '/'
            | '\\'
            | '_'
            | '<'
            | '>'
            | '|'
            | '│'
            | '╱'
            | '╲'
            | '▀'
            | '▄'
    )
}

fn style_loading_line(line: &str) -> Vec<Span<'static>> {
    line.chars()
        .map(|c| {
            if is_logo_highlight_char(c) {
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

    if !splash_fits_wordmark(area.width, area.height) {
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
    fn startup_logo_parses_from_static_file() {
        let logo = startup_logo();
        assert!(!logo.lines.is_empty());
        assert!(logo.lines.len() >= 10);
        assert!(logo.dots_line_index < logo.lines.len());
        assert!(!logo.lines[logo.dots_line_index].is_empty());
    }

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
        let logo = startup_logo();
        let w = logo_line_width();
        for dots in 1..=4u8 {
            let lines = splash_lines_with_dots(dots);
            assert_eq!(
                lines[logo.dots_line_index].chars().count(),
                w,
                "dot count {dots}"
            );
        }
    }
}
