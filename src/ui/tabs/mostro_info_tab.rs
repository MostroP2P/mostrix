use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};
use crate::util::{
    format_instance_info_age, transport_from_instance, MostroInstanceInfo, Transport,
};

pub fn render_mostro_info_tab(f: &mut ratatui::Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .title("🧌 Mostro Instance Info")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PRIMARY_COLOR))
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

    if let Some(ts) = &info.last_updated {
        lines.push(Line::from(vec![
            Span::styled(
                "Last updated: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format_instance_info_age(ts)),
        ]));
        if info.is_stale() {
            lines.push(Line::from(Span::styled(
                "⚠ This data is older than 7 days and may be outdated",
                Style::default().fg(Color::Yellow),
            )));
        }
        lines.push(Line::default());
    }

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
    let protocol_version = match info.protocol_version {
        Some(v) => v.to_string(),
        None => "unknown".to_string(),
    };
    push_kv(&mut lines, "Protocol version", &protocol_version);
    if info.protocol_version == Some(1) {
        lines.push(Line::from(Span::styled(
            "This instance advertises protocol v1 (GiftWrap), which Mostrix no longer supports.",
            Style::default().fg(Color::Yellow),
        )));
    }
    push_kv(
        &mut lines,
        "Wire transport",
        transport_display_label(transport_from_instance(Some(info))),
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
    if info.pow_first_contact.is_some() {
        push_opt_u32(
            &mut lines,
            "Required PoW (first contact, v2)",
            info.pow_first_contact,
        );
    }
    push_kv(
        &mut lines,
        "Anti-abuse bonds",
        match info.bond_enabled {
            Some(true) => "enabled",
            Some(false) => "disabled",
            None => "unknown",
        },
    );

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

fn transport_display_label(_transport: Transport) -> &'static str {
    "NIP-44 direct"
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_text(info: &MostroInstanceInfo) -> String {
        build_info_lines(info)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn v1_protocol_version_shows_unsupported_warning() {
        let info = MostroInstanceInfo {
            protocol_version: Some(1),
            ..Default::default()
        };
        let text = lines_text(&info);
        assert!(text.contains("Protocol version: 1"));
        assert!(text.contains("Wire transport: NIP-44 direct"));
        assert!(text.contains(
            "This instance advertises protocol v1 (GiftWrap), which Mostrix no longer supports."
        ));
    }

    #[test]
    fn v2_protocol_version_has_no_unsupported_warning() {
        let info = MostroInstanceInfo {
            protocol_version: Some(2),
            ..Default::default()
        };
        let text = lines_text(&info);
        assert!(text.contains("Protocol version: 2"));
        assert!(text.contains("Wire transport: NIP-44 direct"));
        assert!(!text.contains("no longer supports"));
    }
}
