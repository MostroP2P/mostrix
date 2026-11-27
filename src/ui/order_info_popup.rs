//! Read-only snapshot of one order from `AdminOrderInfo` (admin / solvers).
//!
//! Database and Lightning values are shown side by side and never merged:
//! the point of the query is to see when they disagree. Disagreements found by
//! [`order_info_warnings`] open the popup ("Check"), then the parts that
//! matter most for a dispute (status, escrow, payout) so they stay visible on
//! short terminals; the rest is reachable by scrolling.

use std::str::FromStr;

use crossterm::event::KeyCode;
use mostro_core::prelude::{BondState, HoldInvoiceState, OrderInfo, PayoutState, Status};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use serde::Serialize;
use uuid::Uuid;

use crate::ui::helpers::{self, format_local_timestamp, render_table_list_scrollbar};
use crate::ui::{AppState, BACKGROUND_COLOR, PRIMARY_COLOR};

pub const ORDER_INFO_HINT: &str = "↑↓ PgUp/PgDn Scroll · Esc/Enter Close";
const PAGE: u16 = 10;
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M";

/// Move the snapshot viewport for a scroll key. The upper bound is applied
/// when rendering, where the wrapped height is known.
pub fn scroll_order_info(scroll: &mut u16, code: KeyCode) {
    *scroll = match code {
        KeyCode::Up => scroll.saturating_sub(1),
        KeyCode::Down => scroll.saturating_add(1),
        KeyCode::PageUp => scroll.saturating_sub(PAGE),
        KeyCode::PageDown => scroll.saturating_add(PAGE),
        _ => *scroll,
    };
}

/// Kebab-case wire name of a mostro-core enum (e.g. `pending-payout`).
fn wire_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "?".to_string())
}

fn time_or(ts: Option<i64>, none: &str) -> String {
    ts.and_then(|t| format_local_timestamp(t, TIME_FORMAT))
        .unwrap_or_else(|| none.to_string())
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(PRIMARY_COLOR)
            .add_modifier(Modifier::BOLD),
    ))
}

fn field(label: &str, value: impl Into<String>, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label}: "), Style::default().fg(Color::Gray)),
        Span::styled(value.into(), Style::default().fg(color)),
    ])
}

/// Stored status: parsed when this build knows it, raw text otherwise.
fn status_label(raw: &str) -> (String, Color) {
    match Status::from_str(raw) {
        Ok(_) => (raw.to_string(), Color::White),
        Err(_) => (format!("{raw} (unknown to this build)"), Color::Yellow),
    }
}

fn hold_invoice_label(state: HoldInvoiceState) -> (&'static str, Color) {
    match state {
        HoldInvoiceState::Open => ("open (not paid yet)", Color::Gray),
        HoldInvoiceState::Accepted => ("accepted (funds locked)", Color::Green),
        HoldInvoiceState::Settled => ("settled (funds claimed)", Color::Green),
        HoldInvoiceState::Canceled => ("canceled (funds back to seller)", Color::Yellow),
        HoldInvoiceState::NotFound => ("not found on the Lightning node", Color::Red),
        HoldInvoiceState::Unavailable => ("unavailable (Lightning node not answering)", Color::Red),
        HoldInvoiceState::Unknown => ("unknown to this build", Color::Yellow),
    }
}

fn payout_label(state: PayoutState) -> (&'static str, Color) {
    match state {
        PayoutState::InFlight => ("in flight", Color::Yellow),
        PayoutState::Succeeded => ("succeeded (buyer paid)", Color::Green),
        PayoutState::Failed => ("failed", Color::Red),
        PayoutState::NotFound => ("not found on the Lightning node", Color::Yellow),
        PayoutState::Unavailable => ("unavailable (Lightning node not answering)", Color::Red),
        PayoutState::Unknown => ("unknown to this build", Color::Yellow),
    }
}

fn parties(buyer: bool, seller: bool) -> String {
    match (buyer, seller) {
        (true, true) => "buyer and seller".to_string(),
        (true, false) => "buyer".to_string(),
        (false, true) => "seller".to_string(),
        (false, false) => "none".to_string(),
    }
}

/// How serious an [`OrderInfoWarning`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningLevel {
    /// The database and Lightning contradict each other, or money is stuck.
    Danger,
    /// Part of the state is unknown (e.g. the Lightning node did not answer).
    Caution,
    /// Worth knowing, nothing is wrong.
    Note,
}

/// One thing the admin should look at before acting on the order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderInfoWarning {
    pub level: WarningLevel,
    pub text: String,
}

impl OrderInfoWarning {
    fn new(level: WarningLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }
}

/// Points where the stored status and the Lightning node disagree, or where
/// funds need attention. Only clear contradictions are reported: a state that
/// can legitimately lag behind the status (e.g. a seller paying right now) is
/// not. Most serious first.
pub fn order_info_warnings(info: &OrderInfo) -> Vec<OrderInfoWarning> {
    use WarningLevel::{Caution, Danger, Note};
    let mut out = Vec::new();
    let status = Status::from_str(&info.status).ok();

    // Without a known status the escrow cannot be compared with it.
    if status.is_none() {
        out.push(OrderInfoWarning::new(
            Caution,
            format!(
                "Status {} is unknown to this build: the escrow was not checked against it.",
                info.status
            ),
        ));
    }

    if info.hold_invoice.is_none() && info.cashu_escrow.is_none() {
        // A running or disputed trade always has an escrow (Lightning or Cashu).
        if let Some(Status::Active | Status::FiatSent | Status::Dispute) = status {
            out.push(OrderInfoWarning::new(
                Danger,
                format!(
                    "Status is {} but the order has no escrow (no hold invoice, no Cashu escrow).",
                    info.status
                ),
            ));
        }
    }

    if let Some(hold) = &info.hold_invoice {
        let state = hold.state;
        let label = hold_invoice_label(state).0;
        match state {
            HoldInvoiceState::Unavailable => out.push(OrderInfoWarning::new(
                Caution,
                "The Lightning node did not answer: the hold invoice state is unknown.",
            )),
            HoldInvoiceState::Unknown => out.push(OrderInfoWarning::new(
                Caution,
                "The hold invoice state is unknown to this build: check the escrow before acting.",
            )),
            _ => match status {
                // The escrow must be locked while the trade runs or is disputed.
                Some(Status::Active | Status::FiatSent | Status::Dispute)
                    if state != HoldInvoiceState::Accepted =>
                {
                    out.push(OrderInfoWarning::new(
                        Danger,
                        format!(
                            "Status is {} but the hold invoice is {label}: the escrow is not locked, \
                             so settling or canceling it will fail.",
                            info.status
                        ),
                    ));
                }
                // These statuses mean the seller's funds were claimed.
                Some(
                    Status::SettledHoldInvoice
                    | Status::Success
                    | Status::SettledByAdmin
                    | Status::CompletedByAdmin,
                ) if state != HoldInvoiceState::Settled => {
                    out.push(OrderInfoWarning::new(
                        Danger,
                        format!(
                            "Status is {} but the hold invoice is {label}, not settled.",
                            info.status
                        ),
                    ));
                }
                Some(
                    Status::Canceled
                    | Status::CanceledByAdmin
                    | Status::CooperativelyCanceled
                    | Status::Expired,
                ) => match state {
                    HoldInvoiceState::Accepted => out.push(OrderInfoWarning::new(
                        Danger,
                        format!(
                            "Status is {} but the seller's funds are still locked in the hold invoice.",
                            info.status
                        ),
                    )),
                    HoldInvoiceState::Settled => out.push(OrderInfoWarning::new(
                        Danger,
                        format!(
                            "Status is {} but the hold invoice was settled (funds claimed).",
                            info.status
                        ),
                    )),
                    _ => {}
                },
                _ => {}
            },
        }
    }

    let payout_state = info.payout.as_ref().map(|p| p.state);
    match payout_state {
        Some(PayoutState::Failed) => out.push(OrderInfoWarning::new(
            Danger,
            format!(
                "The payment to the buyer failed (attempts: {}).",
                info.payment_attempts
            ),
        )),
        Some(PayoutState::Unavailable) => out.push(OrderInfoWarning::new(
            Caution,
            "The Lightning node did not answer: the payment to the buyer is unknown.",
        )),
        Some(PayoutState::Unknown) => out.push(OrderInfoWarning::new(
            Caution,
            "The payment to the buyer is in a state unknown to this build.",
        )),
        _ => {}
    }
    // The database flag stands on its own: an unreachable node must not hide
    // a failure Mostro already recorded. Skipped when Lightning reports it too.
    if info.failed_payment && payout_state != Some(PayoutState::Failed) {
        out.push(OrderInfoWarning::new(
            Danger,
            format!(
                "The last payment to the buyer failed (attempts: {}).",
                info.payment_attempts
            ),
        ));
    }

    for bond in &info.bonds {
        let who = wire_name(&bond.role);
        match bond.state {
            BondState::Failed => out.push(OrderInfoWarning::new(
                Danger,
                format!(
                    "The {who} bond payout ({} sats) failed and needs operator attention.",
                    bond.amount
                ),
            )),
            BondState::PendingPayout => out.push(OrderInfoWarning::new(
                Note,
                format!(
                    "The {who} bond ({} sats) was slashed; its payout is pending.",
                    bond.amount
                ),
            )),
            _ => {}
        }
    }

    out.sort_by_key(|w| match w.level {
        Danger => 0,
        Caution => 1,
        Note => 2,
    });
    out
}

/// Background `AdminOrderInfo` started when the Resolve review opens, so the
/// admin sees the escrow state before paying the buyer or refunding the seller.
#[derive(Debug, Clone)]
pub struct FinalizationOrderCheck {
    pub order_id: Uuid,
    /// Request number; only the matching reply is stored.
    pub seq: u64,
    pub state: OrderCheckState,
}

#[derive(Debug, Clone)]
pub enum OrderCheckState {
    Loading,
    Ready(Box<OrderInfo>),
    Failed(String),
}

/// One-line "Mostro check" for the Resolve review and its confirm step: the
/// most serious warning, or the escrow state when nothing is wrong.
pub fn order_check_summary(state: &OrderCheckState) -> (String, Color) {
    match state {
        OrderCheckState::Loading => (
            "Mostro check: asking for the real order state…".to_string(),
            Color::Gray,
        ),
        OrderCheckState::Failed(message) => {
            (format!("⚠ Mostro check failed: {message}"), Color::Yellow)
        }
        OrderCheckState::Ready(info) => {
            let warnings = order_info_warnings(info);
            let more = match warnings.len() {
                0 | 1 => String::new(),
                n => format!(" (+{} more)", n - 1),
            };
            if let Some(first) = warnings.first() {
                let (icon, color) = match first.level {
                    WarningLevel::Danger => ("⛔", Color::Red),
                    WarningLevel::Caution => ("⚠", Color::Yellow),
                    WarningLevel::Note => ("ℹ", Color::Cyan),
                };
                if first.level != WarningLevel::Note {
                    return (format!("{icon} Mostro check: {}{more}", first.text), color);
                }
            }
            let escrow = match (&info.hold_invoice, &info.cashu_escrow) {
                (Some(hold), _) => format!("hold invoice {}", hold_invoice_label(hold.state).0),
                (None, Some(_)) => "Cashu escrow".to_string(),
                (None, None) => "no hold invoice".to_string(),
            };
            (
                format!("✅ Mostro check: status {}, {escrow}{more}", info.status),
                Color::Green,
            )
        }
    }
}

/// The Resolve-review check for `order_id`, if one was started for it.
pub fn order_check_for<'a>(app: &'a AppState, order_id: &str) -> Option<&'a OrderCheckState> {
    app.finalization_order_check
        .as_ref()
        .filter(|c| c.order_id.to_string() == order_id)
        .map(|c| &c.state)
}

/// Wrapped one-line summary paragraph for the Resolve review and confirm step.
pub fn order_check_paragraph(state: &OrderCheckState) -> Paragraph<'static> {
    let (text, color) = order_check_summary(state);
    Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )))
    .wrap(Wrap { trim: true })
}

fn warning_line(warning: &OrderInfoWarning) -> Line<'static> {
    let (icon, color) = match warning.level {
        WarningLevel::Danger => ("⛔", Color::Red),
        WarningLevel::Caution => ("⚠", Color::Yellow),
        WarningLevel::Note => ("ℹ", Color::Cyan),
    };
    Line::from(Span::styled(
        format!("  {icon} {}", warning.text),
        Style::default().fg(color),
    ))
}

/// All snapshot lines, essential sections first.
pub fn order_info_lines(info: &OrderInfo) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let warnings = order_info_warnings(info);
    if !warnings.is_empty() {
        lines.push(section("Check"));
        lines.extend(warnings.iter().map(warning_line));
        lines.push(Line::from(""));
    }

    let (status, status_color) = status_label(&info.status);
    lines.push(section("Order (database)"));
    lines.push(field("Status", status, status_color));
    lines.push(field(
        "Dispute opened by",
        parties(info.buyer_dispute, info.seller_dispute),
        Color::White,
    ));
    lines.push(field(
        "Cooperative cancel by",
        parties(info.buyer_cooperativecancel, info.seller_cooperativecancel),
        Color::White,
    ));
    lines.push(Line::from(""));

    lines.push(section("Escrow"));
    match (&info.hold_invoice, &info.cashu_escrow) {
        (Some(hold), _) => {
            let (label, color) = hold_invoice_label(hold.state);
            lines.push(field("Hold invoice (Lightning)", label, color));
            lines.push(field(
                "Held since (database)",
                time_or(hold.held_at, "not held"),
                Color::White,
            ));
        }
        (None, Some(cashu)) => {
            lines.push(field("Cashu mint", cashu.mint_url.clone(), Color::White));
            lines.push(field(
                "Locked at",
                time_or(cashu.locked_at, "not locked"),
                Color::White,
            ));
        }
        (None, None) => lines.push(field("Hold invoice", "none yet", Color::Gray)),
    }
    if let Some(ts) = info.hold_settled_at {
        lines.push(field(
            "Settled at (database)",
            time_or(Some(ts), "-"),
            Color::White,
        ));
    }
    lines.push(Line::from(""));

    lines.push(section("Payout to buyer"));
    match &info.payout {
        Some(payout) => {
            let (label, color) = payout_label(payout.state);
            lines.push(field("Payment (Lightning)", label, color));
            if let Some(reason) = &payout.failure_reason {
                lines.push(field("Failure reason", wire_name(reason), Color::Red));
            }
            if payout.resolved_at.is_some() {
                lines.push(field(
                    "Resolved at",
                    time_or(payout.resolved_at, "-"),
                    Color::White,
                ));
            }
        }
        None => lines.push(field("Payment", "none in progress", Color::Gray)),
    }
    lines.push(field(
        "Last attempt failed (database)",
        yes_no(info.failed_payment),
        if info.failed_payment {
            Color::Red
        } else {
            Color::White
        },
    ));
    lines.push(field(
        "Payment attempts",
        info.payment_attempts.to_string(),
        Color::White,
    ));
    lines.push(Line::from(""));

    lines.push(section("Bonds"));
    if info.bonds.is_empty() {
        lines.push(field("Bonds", "none", Color::Gray));
    }
    for bond in &info.bonds {
        let mut value = format!("{} sats · {}", bond.amount, wire_name(&bond.state));
        if let Some(reason) = &bond.slash_reason {
            value.push_str(&format!(" · slashed: {}", wire_name(reason)));
        }
        if let Some(root) = bond.root_order_id {
            value.push_str(&format!(" · root order {root}"));
        }
        let label = format!("{} {}", wire_name(&bond.role), wire_name(&bond.kind));
        lines.push(field(&label, value, Color::White));
    }
    lines.push(Line::from(""));

    lines.push(section("Details"));
    lines.push(field("Order ID", info.id.to_string(), Color::White));
    lines.push(field("Kind", info.kind.clone(), Color::White));
    lines.push(field(
        "Amount",
        format!("{} sats", info.amount),
        Color::White,
    ));
    let fiat = match (info.min_amount, info.max_amount) {
        (Some(min), Some(max)) => format!(
            "{} {} (range {min}-{max})",
            info.fiat_amount, info.fiat_code
        ),
        _ => format!("{} {}", info.fiat_amount, info.fiat_code),
    };
    lines.push(field("Fiat", fiat, Color::White));
    lines.push(field(
        "Payment method",
        info.payment_method.clone(),
        Color::White,
    ));
    lines.push(field("Premium", format!("{}%", info.premium), Color::White));
    lines.push(field(
        "Mostro fee",
        format!("{} sats", info.fee),
        Color::White,
    ));
    lines.push(field(
        "Routing fee",
        format!("{} sats", info.routing_fee),
        Color::White,
    ));
    if let Some(parent) = info.range_parent_id {
        lines.push(field("Range parent", parent.to_string(), Color::White));
    }
    lines.push(field(
        "Created",
        time_or(Some(info.created_at), "-"),
        Color::White,
    ));
    lines.push(field(
        "Taken",
        time_or(info.taken_at, "not taken"),
        Color::White,
    ));
    lines.push(field(
        "Expires",
        time_or(Some(info.expires_at), "-"),
        Color::White,
    ));
    lines.push(field(
        "Buyer trade key",
        info.buyer_trade_pubkey
            .clone()
            .unwrap_or_else(|| "-".into()),
        Color::White,
    ));
    lines.push(field(
        "Seller trade key",
        info.seller_trade_pubkey
            .clone()
            .unwrap_or_else(|| "-".into()),
        Color::White,
    ));
    lines
}

/// Render the snapshot popup. Clamps `scroll` to the wrapped content height.
pub fn render_order_info_popup(f: &mut ratatui::Frame, info: &OrderInfo, scroll: &mut u16) {
    let area = f.area();
    let popup = helpers::create_centered_popup(area, 80, 30);
    f.render_widget(Clear, popup);

    let short_id: String = info.id.to_string().chars().take(8).collect();
    let block = Block::default()
        .title(format!("🔎 Order state · {short_id}"))
        .borders(Borders::ALL)
        .style(Style::default().bg(BACKGROUND_COLOR).fg(PRIMARY_COLOR));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Very short terminals: give every row to the content, drop the hint.
    let (content_area, hint_area) = if inner.height >= 4 {
        let chunks = Layout::new(
            Direction::Vertical,
            [Constraint::Min(1), Constraint::Length(1)],
        )
        .split(inner);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner, None)
    };

    // Leave one column for the scrollbar.
    let text_width = content_area.width.saturating_sub(1).max(1);
    let paragraph = Paragraph::new(order_info_lines(info)).wrap(Wrap { trim: false });
    let total = paragraph.line_count(text_width);
    let visible = content_area.height as usize;
    let max_scroll = total.saturating_sub(visible).min(u16::MAX as usize) as u16;
    *scroll = (*scroll).min(max_scroll);

    let text_area = ratatui::layout::Rect {
        width: text_width,
        ..content_area
    };
    f.render_widget(paragraph.scroll((*scroll, 0)), text_area);
    // The helper skips the block's top border row; offset the track by one.
    let track = ratatui::layout::Rect {
        y: content_area.y.saturating_sub(1),
        height: content_area.height + 1,
        ..content_area
    };
    render_table_list_scrollbar(f, track, total, visible, 0, *scroll as usize);

    if let Some(hint_area) = hint_area {
        f.render_widget(
            Paragraph::new(Span::styled(
                ORDER_INFO_HINT,
                Style::default().fg(Color::DarkGray),
            ))
            .alignment(ratatui::layout::Alignment::Center),
            hint_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mostro_core::prelude::{
        BondInfo, BondKind, BondRole, BondState, CashuEscrowInfo, HoldInvoiceInfo,
        PayoutFailureReason, PayoutInfo,
    };
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    use crate::models::AdminDispute;
    use crate::ui::dispute_finalization_confirm::render_finalization_confirm;
    use crate::ui::UserRole;
    use crate::util::order_utils::BondSlashChoice;

    fn sample() -> OrderInfo {
        OrderInfo {
            id: Uuid::parse_str("308e1272-d5f4-47e6-bd97-3504baea9c23").unwrap(),
            kind: "sell".into(),
            status: "dispute".into(),
            amount: 25_000,
            fiat_code: "EUR".into(),
            fiat_amount: 20,
            payment_method: "SEPA".into(),
            buyer_dispute: true,
            hold_invoice: Some(HoldInvoiceInfo {
                state: HoldInvoiceState::Canceled,
                held_at: Some(1_700_000_000),
            }),
            payout: Some(PayoutInfo {
                state: PayoutState::Failed,
                failure_reason: Some(PayoutFailureReason::NoRoute),
                resolved_at: None,
            }),
            bonds: vec![BondInfo {
                role: BondRole::Taker,
                amount: 500,
                state: BondState::PendingPayout,
                slash_reason: None,
                kind: BondKind::Bond,
                root_order_id: None,
            }],
            ..Default::default()
        }
    }

    fn text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render(info: &OrderInfo, w: u16, h: u16, scroll: &mut u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| render_order_info_popup(f, info, scroll))
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        flat
    }

    #[test]
    fn database_and_lightning_values_are_shown_side_by_side() {
        let t = text(&order_info_lines(&sample()));
        assert!(t.contains("Status: dispute"), "{t}");
        assert!(t.contains("Dispute opened by: buyer"), "{t}");
        assert!(t.contains("Hold invoice (Lightning): canceled (funds back to seller)"));
        assert!(t.contains("Payment (Lightning): failed"));
        assert!(t.contains("Failure reason: no-route"));
        assert!(t.contains("taker bond: 500 sats · pending-payout"), "{t}");
    }

    #[test]
    fn unknown_status_is_shown_raw_and_cashu_escrow_without_hold_invoice() {
        let info = OrderInfo {
            status: "some-future-status".into(),
            cashu_escrow: Some(CashuEscrowInfo {
                mint_url: "https://mint.example".into(),
                locked_at: None,
            }),
            ..Default::default()
        };
        let t = text(&order_info_lines(&info));
        assert!(t.contains("some-future-status (unknown to this build)"));
        assert!(t.contains("Cashu mint: https://mint.example"));
        assert!(t.contains("Locked at: not locked"));
        assert!(!t.contains("Hold invoice"));
        assert!(t.contains("Payment: none in progress"));
        assert!(t.contains("Bonds: none"));
    }

    #[test]
    fn popup_shows_status_and_escrow_first_and_clamps_scroll() {
        let mut scroll = u16::MAX;
        let screen = render(&sample(), 100, 40, &mut scroll);
        assert!(screen.contains("Order state · 308e1272"), "{screen}");
        assert!(screen.contains(ORDER_INFO_HINT));
        // Clamped to the end: the last detail line is visible.
        assert!(screen.contains("Seller trade key"), "{screen}");
        assert!(scroll > 0);

        let mut top = 0;
        let screen = render(&sample(), 100, 40, &mut top);
        assert!(screen.contains("Status: dispute"));
        assert!(screen.contains("canceled (funds back to seller)"));
    }

    #[test]
    fn popup_stays_readable_on_narrow_short_terminal() {
        let mut scroll = 0;
        let screen = render(&sample(), 40, 5, &mut scroll);
        assert!(screen.contains("Check"), "warnings come first: {screen}");
        assert!(
            !screen.contains("Scroll"),
            "hint dropped when too short: {screen}"
        );

        let consistent = OrderInfo {
            status: "dispute".into(),
            hold_invoice: Some(HoldInvoiceInfo {
                state: HoldInvoiceState::Accepted,
                held_at: None,
            }),
            ..Default::default()
        };
        let mut scroll = 0;
        let screen = render(&consistent, 40, 6, &mut scroll);
        assert!(screen.contains("Status: dispute"), "{screen}");
        assert!(screen.contains("Esc/Enter Close"), "{screen}");
    }

    fn with_hold(status: &str, state: HoldInvoiceState) -> OrderInfo {
        OrderInfo {
            status: status.into(),
            hold_invoice: Some(HoldInvoiceInfo {
                state,
                held_at: None,
            }),
            ..Default::default()
        }
    }

    fn levels(info: &OrderInfo) -> Vec<WarningLevel> {
        order_info_warnings(info).iter().map(|w| w.level).collect()
    }

    #[test]
    fn consistent_states_raise_no_warning() {
        for (status, state) in [
            ("dispute", HoldInvoiceState::Accepted),
            ("active", HoldInvoiceState::Accepted),
            ("success", HoldInvoiceState::Settled),
            ("settled-by-admin", HoldInvoiceState::Settled),
            ("canceled-by-admin", HoldInvoiceState::Canceled),
            ("expired", HoldInvoiceState::Open),
            // May legitimately lag behind: not a contradiction.
            ("waiting-payment", HoldInvoiceState::Accepted),
        ] {
            assert!(
                levels(&with_hold(status, state)).is_empty(),
                "{status} + {state:?}"
            );
        }
        // Cashu order: no hold invoice to compare.
        assert!(levels(&OrderInfo {
            status: "dispute".into(),
            cashu_escrow: Some(CashuEscrowInfo {
                mint_url: "https://mint.example".into(),
                locked_at: Some(1),
            }),
            ..Default::default()
        })
        .is_empty());
        // Not taken yet: no escrow is expected.
        assert!(levels(&OrderInfo {
            status: "pending".into(),
            ..Default::default()
        })
        .is_empty());
    }

    #[test]
    fn unverifiable_states_are_cautions_not_green() {
        // Hold invoice state this build does not know.
        let info = with_hold("dispute", HoldInvoiceState::Unknown);
        assert_eq!(levels(&info), vec![WarningLevel::Caution]);
        let (text, color) = order_check_summary(&OrderCheckState::Ready(Box::new(info)));
        assert!(text.starts_with("⚠"), "{text}");
        assert_eq!(color, Color::Yellow);

        // Status this build does not know: the escrow is not compared with it.
        let info = with_hold("brand-new-status", HoldInvoiceState::Accepted);
        let warnings = order_info_warnings(&info);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(warnings[0].level, WarningLevel::Caution);
        assert!(warnings[0].text.contains("brand-new-status"));

        // Payout state this build does not know.
        let mut info = with_hold("settled-hold-invoice", HoldInvoiceState::Settled);
        info.payout = Some(PayoutInfo {
            state: PayoutState::Unknown,
            failure_reason: None,
            resolved_at: None,
        });
        assert_eq!(levels(&info), vec![WarningLevel::Caution]);
    }

    #[test]
    fn running_trade_without_any_escrow_is_a_danger() {
        for status in ["active", "fiat-sent", "dispute"] {
            let info = OrderInfo {
                status: status.into(),
                ..Default::default()
            };
            let warnings = order_info_warnings(&info);
            assert_eq!(warnings.len(), 1, "{status}: {warnings:?}");
            assert_eq!(warnings[0].level, WarningLevel::Danger);
            assert!(warnings[0].text.contains("no escrow"), "{:?}", warnings[0]);
            let (text, color) = order_check_summary(&OrderCheckState::Ready(Box::new(info)));
            assert!(text.starts_with("⛔"), "{text}");
            assert_eq!(color, Color::Red);
        }
    }

    #[test]
    fn recorded_payment_failure_survives_an_unreachable_node() {
        let mut info = with_hold("settled-hold-invoice", HoldInvoiceState::Settled);
        info.failed_payment = true;
        info.payment_attempts = 2;
        info.payout = Some(PayoutInfo {
            state: PayoutState::Unavailable,
            failure_reason: None,
            resolved_at: None,
        });
        let warnings = order_info_warnings(&info);
        assert_eq!(
            warnings.iter().map(|w| w.level).collect::<Vec<_>>(),
            vec![WarningLevel::Danger, WarningLevel::Caution]
        );
        assert!(
            warnings[0].text.contains("attempts: 2"),
            "{:?}",
            warnings[0]
        );

        // Without the database flag only the caution remains.
        info.failed_payment = false;
        assert_eq!(levels(&info), vec![WarningLevel::Caution]);

        // Lightning already reports the failure: a single warning, not two.
        info.failed_payment = true;
        info.payout.as_mut().unwrap().state = PayoutState::Failed;
        assert_eq!(levels(&info), vec![WarningLevel::Danger]);
    }

    #[test]
    fn status_and_hold_invoice_contradictions_are_dangers() {
        for (status, state, needle) in [
            (
                "dispute",
                HoldInvoiceState::Canceled,
                "escrow is not locked",
            ),
            (
                "fiat-sent",
                HoldInvoiceState::NotFound,
                "escrow is not locked",
            ),
            ("dispute", HoldInvoiceState::Settled, "escrow is not locked"),
            ("success", HoldInvoiceState::Accepted, "not settled"),
            (
                "settled-hold-invoice",
                HoldInvoiceState::Canceled,
                "not settled",
            ),
            (
                "canceled-by-admin",
                HoldInvoiceState::Accepted,
                "still locked",
            ),
            (
                "cooperatively-canceled",
                HoldInvoiceState::Settled,
                "was settled",
            ),
        ] {
            let warnings = order_info_warnings(&with_hold(status, state));
            assert_eq!(warnings.len(), 1, "{status} + {state:?}: {warnings:?}");
            assert_eq!(warnings[0].level, WarningLevel::Danger);
            assert!(warnings[0].text.contains(needle), "{:?}", warnings[0]);
        }
    }

    #[test]
    fn unreachable_lightning_node_is_a_caution() {
        let mut info = with_hold("dispute", HoldInvoiceState::Unavailable);
        info.payout = Some(PayoutInfo {
            state: PayoutState::Unavailable,
            failure_reason: None,
            resolved_at: None,
        });
        assert_eq!(levels(&info), vec![WarningLevel::Caution; 2]);
    }

    #[test]
    fn payout_and_bond_problems_are_reported_most_serious_first() {
        let mut info = OrderInfo {
            status: "settled-hold-invoice".into(),
            failed_payment: true,
            payment_attempts: 3,
            ..Default::default()
        };
        let warnings = order_info_warnings(&info);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].text.contains("attempts: 3"));

        info.bonds = vec![
            BondInfo {
                role: BondRole::Maker,
                amount: 700,
                state: BondState::PendingPayout,
                slash_reason: None,
                kind: BondKind::Bond,
                root_order_id: None,
            },
            BondInfo {
                role: BondRole::Taker,
                amount: 500,
                state: BondState::Failed,
                slash_reason: None,
                kind: BondKind::Bond,
                root_order_id: None,
            },
        ];
        let warnings = order_info_warnings(&info);
        assert_eq!(
            warnings.iter().map(|w| w.level).collect::<Vec<_>>(),
            vec![
                WarningLevel::Danger,
                WarningLevel::Danger,
                WarningLevel::Note
            ]
        );
        assert!(warnings[1]
            .text
            .contains("taker bond payout (500 sats) failed"));
        assert!(warnings[2]
            .text
            .contains("maker bond (700 sats) was slashed"));
    }

    #[test]
    fn review_summary_names_the_worst_problem_or_the_escrow_state() {
        let (text, color) = order_check_summary(&OrderCheckState::Loading);
        assert!(text.contains("asking"));
        assert_eq!(color, Color::Gray);

        let (text, color) =
            order_check_summary(&OrderCheckState::Failed("Mostro has no order".into()));
        assert_eq!(text, "⚠ Mostro check failed: Mostro has no order");
        assert_eq!(color, Color::Yellow);

        let ok = with_hold("dispute", HoldInvoiceState::Accepted);
        let (text, color) = order_check_summary(&OrderCheckState::Ready(Box::new(ok)));
        assert_eq!(
            text,
            "✅ Mostro check: status dispute, hold invoice accepted (funds locked)"
        );
        assert_eq!(color, Color::Green);

        // sample(): canceled hold invoice in dispute + failed payout + pending bond.
        let (text, color) = order_check_summary(&OrderCheckState::Ready(Box::new(sample())));
        assert!(
            text.starts_with("⛔ Mostro check: Status is dispute"),
            "{text}"
        );
        assert!(text.ends_with("(+2 more)"), "{text}");
        assert_eq!(color, Color::Red);

        // A note alone (pending bond payout) is not a problem: escrow state shown.
        let mut noted = with_hold("dispute", HoldInvoiceState::Accepted);
        noted.bonds = sample().bonds;
        let (text, color) = order_check_summary(&OrderCheckState::Ready(Box::new(noted)));
        assert!(text.starts_with("✅") && !text.contains("more"), "{text}");
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn confirm_step_shows_the_mostro_check_above_yes_no() {
        let order_id = Uuid::from_u128(77);
        let dispute_id = Uuid::from_u128(78);
        let mut app = AppState::new(UserRole::Admin);
        app.admin_disputes_in_progress = vec![AdminDispute {
            id: order_id.to_string(),
            dispute_id: dispute_id.to_string(),
            ..Default::default()
        }];
        let mut bad = with_hold("dispute", HoldInvoiceState::Canceled);
        bad.id = order_id;
        app.finalization_order_check = Some(FinalizationOrderCheck {
            order_id,
            seq: 1,
            state: OrderCheckState::Ready(Box::new(bad)),
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|f| {
                render_finalization_confirm(f, &app, &dispute_id, true, BondSlashChoice::None, true)
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut flat = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                flat.push_str(buf[(x, y)].symbol());
            }
            flat.push('\n');
        }
        assert!(flat.contains("Mostro check"), "{flat}");
        assert!(flat.contains("YES") && flat.contains("NO"), "{flat}");
    }

    #[test]
    fn check_section_opens_the_snapshot_only_when_needed() {
        let t = text(&order_info_lines(&sample()));
        assert!(t.starts_with("Check\n  ⛔"), "{t}");
        let consistent = text(&order_info_lines(&with_hold(
            "dispute",
            HoldInvoiceState::Accepted,
        )));
        assert!(!consistent.contains("Check"), "{consistent}");
        assert!(consistent.starts_with("Order (database)"));
    }

    #[test]
    fn scroll_keys_move_by_line_and_page_without_underflow() {
        let mut s = 0;
        scroll_order_info(&mut s, KeyCode::Up);
        assert_eq!(s, 0);
        scroll_order_info(&mut s, KeyCode::PageDown);
        assert_eq!(s, PAGE);
        scroll_order_info(&mut s, KeyCode::Up);
        assert_eq!(s, PAGE - 1);
        scroll_order_info(&mut s, KeyCode::PageUp);
        assert_eq!(s, 0);
    }
}
