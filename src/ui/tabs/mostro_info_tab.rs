use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::MostroInstanceInfo;

pub fn render_mostro_info_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .title("🧌 Mostro Instance Info")
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [
            Constraint::Min(0), // details
        ],
    )
    .split(inner);

    match &app.mostro_info {
        None => {
            let message = Paragraph::new(Line::from(vec![
                Span::raw("No Mostro instance info has been loaded yet."),
                Span::raw(" "),
                Span::styled(
                    "Press Enter in this tab to fetch the latest Mostro instance info from relays, or change the Mostro pubkey in Settings to auto-refresh.",
                    Style::default().add_modifier(Modifier::ITALIC),
                ),
            ]))
            .wrap(Wrap { trim: true });
            f.render_widget(message, chunks[0]);
        }
        Some(info) => {
            render_info_details(f, chunks[0], info);
        }
    }
}

fn render_info_details(f: &mut ratatui::Frame, area: Rect, info: &MostroInstanceInfo) {
    let lines = build_info_lines(info);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true }).block(
        Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(BACKGROUND_COLOR)),
    );

    f.render_widget(paragraph, area);
}

fn build_info_lines(info: &MostroInstanceInfo) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Mostro daemon section
    lines.push(section_title("Mostro daemon"));
    push_kv(
        &mut lines,
        "Version",
        info.mostro_version.as_deref().unwrap_or("unknown"),
    );
    push_kv(
        &mut lines,
        "Github commit hash",
        info.mostro_commit_hash.as_deref().unwrap_or("unknown"),
    );
    push_opt_i64(&mut lines, "Max order amount (sats)", info.max_order_amount);
    push_opt_i64(&mut lines, "Min order amount (sats)", info.min_order_amount);
    push_opt_u64(&mut lines, "Expiration (hours)", info.expiration_hours);
    push_opt_u64(&mut lines, "Expiration (seconds)", info.expiration_seconds);
    push_opt_u64(
        &mut lines,
        "Hold invoice expiration window (seconds)",
        info.hold_invoice_expiration_window,
    );
    push_opt_u32(
        &mut lines,
        "Hold invoice CLTV delta (blocks)",
        info.hold_invoice_cltv_delta,
    );
    push_opt_u64(
        &mut lines,
        "Invoice expiration window (seconds)",
        info.invoice_expiration_window,
    );
    push_opt_u32(
        &mut lines,
        "Max orders per response",
        info.max_orders_per_response,
    );
    push_opt_f64(&mut lines, "Fee (fraction)", info.fee);
    push_opt_u32(&mut lines, "Required PoW", info.pow);

    lines.push(Line::default());

    // Lightning node section
    lines.push(section_title("Lightning node"));
    push_kv(
        &mut lines,
        "Alias",
        info.lnd_node_alias.as_deref().unwrap_or("unknown"),
    );
    push_kv(
        &mut lines,
        "Node pubkey",
        info.lnd_node_pubkey.as_deref().unwrap_or("unknown"),
    );
    push_kv(
        &mut lines,
        "LND version",
        info.lnd_version.as_deref().unwrap_or("unknown"),
    );
    push_kv(
        &mut lines,
        "LND commit hash",
        info.lnd_commit_hash.as_deref().unwrap_or("unknown"),
    );
    push_list(&mut lines, "Chains", &info.lnd_chains);
    push_list(&mut lines, "Networks", &info.lnd_networks);
    for uri in &info.lnd_uris {
        push_kv(&mut lines, "URI", uri);
    }

    lines.push(Line::default());

    // Fiat currencies section
    lines.push(section_title("Fiat currencies"));
    if info.fiat_currencies_accepted.is_empty() {
        lines.push(Line::from(Span::raw("All currencies are accepted.")));
    } else {
        push_list(&mut lines, "Accepted", &info.fiat_currencies_accepted);
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Hint: press Enter in this tab to refresh Mostro instance info from relays.",
        Style::default().add_modifier(Modifier::ITALIC),
    )));

    lines
}

fn section_title(title: &str) -> Line<'static> {
    Line::from(vec![Span::styled(
        title.to_string(),
        Style::default()
            .fg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD),
    )])
}

fn push_kv(lines: &mut Vec<Line<'static>>, label: &str, value: &str) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(value.to_string()),
    ]));
}

fn push_opt_i64(lines: &mut Vec<Line<'static>>, label: &str, value: Option<i64>) {
    if let Some(v) = value {
        push_kv(lines, label, &v.to_string());
    }
}

fn push_opt_u64(lines: &mut Vec<Line<'static>>, label: &str, value: Option<u64>) {
    if let Some(v) = value {
        push_kv(lines, label, &v.to_string());
    }
}

fn push_opt_u32(lines: &mut Vec<Line<'static>>, label: &str, value: Option<u32>) {
    if let Some(v) = value {
        push_kv(lines, label, &v.to_string());
    }
}

fn push_opt_f64(lines: &mut Vec<Line<'static>>, label: &str, value: Option<f64>) {
    if let Some(v) = value {
        push_kv(lines, label, &v.to_string());
    }
}

fn push_list(lines: &mut Vec<Line<'static>>, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }

    let joined = items.join(", ");
    push_kv(lines, label, &joined);
}
