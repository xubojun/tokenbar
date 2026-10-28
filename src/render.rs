//! 把正規化狀態畫成兩行狀態列：
//!
//! ```text
//! Opus 5.5 │ high │ my-project │ main ↑2 ●3 ?1
//! ctx ███▊░░░░░░  38% │ 5h ██████▋░░░  66% ↻ 1h 46m │ 7d █████▊░░░░  58% ↻ 2d 12h
//! ```

use std::fmt::Write;
use std::path::Path;

use crate::status::{Git, Status};

const PARTIALS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
const SEP: &str = " │ ";

#[derive(Clone, Copy)]
enum Style {
    Bold,
    Dim,
    Red,
    Green,
    Yellow,
    Cyan,
    Magenta,
}

impl Style {
    fn code(self) -> &'static str {
        match self {
            Style::Bold => "1",
            Style::Dim => "2",
            Style::Red => "31",
            Style::Green => "32",
            Style::Yellow => "33",
            Style::Cyan => "36",
            Style::Magenta => "35",
        }
    }
}

struct Painter {
    color: bool,
}

impl Painter {
    fn paint(&self, style: Style, text: &str) -> String {
        if self.color { format!("\x1b[{}m{text}\x1b[0m", style.code()) } else { text.to_string() }
    }
}

/// 依使用率挑顏色：<60% 綠、<85% 黃、其餘紅。
fn level(pct: f64) -> Style {
    if pct >= 85.0 {
        Style::Red
    } else if pct >= 60.0 {
        Style::Yellow
    } else {
        Style::Green
    }
}

/// 以八分之一格的精度畫出長度為 `width` 的用量條。
pub fn bar(pct: f64, width: usize) -> String {
    let pct = pct.clamp(0.0, 100.0);
    let eighths = (pct / 100.0 * width as f64 * 8.0).round() as usize;
    let full = eighths / 8;
    let partial = PARTIALS[eighths % 8];
    let empty = width - full - usize::from(!partial.is_empty());
    format!("{}{partial}{}", "█".repeat(full), "░".repeat(empty))
}

/// 把秒數格式化為 `2d 12h`、`1h 46m`、`5m`。
pub fn duration(seconds: i64) -> String {
    if seconds <= 0 {
        return "0m".into();
    }
    let (d, h, m) = (seconds / 86400, seconds % 86400 / 3600, seconds % 3600 / 60);
    match (d, h, m) {
        (d, 0, _) if d > 0 => format!("{d}d"),
        (d, h, _) if d > 0 => format!("{d}d {h}h"),
        (_, h, 0) if h > 0 => format!("{h}h"),
        (_, h, m) if h > 0 => format!("{h}h {m}m"),
        (_, _, m) => format!("{}m", m.max(1)),
    }
}

/// 去掉 ANSI 色碼後的可見字元數（狀態列只用到寬度 1 的字元）。
pub fn visible_len(text: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in text.chars() {
        match (in_escape, c) {
            (false, '\x1b') => in_escape = true,
            (true, 'm') => in_escape = false,
            (true, _) => {}
            (false, _) => len += 1,
        }
    }
    len
}

struct Layout {
    bar_width: usize,
    show_reset: bool,
}

fn meter(p: &Painter, label: &str, pct: f64, resets_at: Option<i64>, layout: &Layout, now: i64) -> String {
    let mut out = format!(
        "{} {} {:>3}%",
        p.paint(Style::Dim, label),
        p.paint(level(pct), &bar(pct, layout.bar_width)),
        pct.round() as i64
    );
    if let (true, Some(at)) = (layout.show_reset, resets_at) {
        let _ = write!(out, " {}", p.paint(Style::Dim, &format!("↻ {}", duration(at - now))));
    }
    out
}

fn header(status: &Status, git: Option<&Git>, p: &Painter) -> String {
    let mut parts = Vec::new();
    if let Some(model) = &status.model {
        parts.push(p.paint(Style::Bold, model));
    }
    if let Some(effort) = &status.effort {
        parts.push(p.paint(Style::Magenta, effort));
    }
    if let Some(cwd) = &status.cwd {
        let name = Path::new(cwd).file_name().and_then(|n| n.to_str()).unwrap_or(cwd);
        parts.push(p.paint(Style::Cyan, name));
    }
    if let Some(git) = git {
        let mut text = git.branch.clone();
        for (symbol, n) in [("↑", git.ahead), ("↓", git.behind), ("●", git.changed), ("?", git.untracked)] {
            if n > 0 {
                let _ = write!(text, " {symbol}{n}");
            }
        }
        parts.push(p.paint(Style::Green, &text));
    }
    parts.join(SEP)
}

fn meters(status: &Status, p: &Painter, layout: &Layout, now: i64) -> String {
    let mut segments = Vec::new();
    if let Some(ctx) = status.context {
        segments.push(meter(p, "ctx", ctx, None, layout, now));
    }
    for limit in &status.limits {
        segments.push(meter(p, &limit.label, limit.used_pct, limit.resets_at, layout, now));
    }
    segments.join(SEP)
}

/// 畫出狀態列。寬度不足時依序：拿掉重置時間 → 縮短用量條。
pub fn render(status: &Status, git: Option<&Git>, color: bool, now: i64) -> String {
    let p = Painter { color };
    let head = header(status, git, &p);

    let layouts = [
        Layout { bar_width: 10, show_reset: true },
        Layout { bar_width: 10, show_reset: false },
        Layout { bar_width: 5, show_reset: false },
    ];
    let mut line = String::new();
    for layout in &layouts {
        line = meters(status, &p, layout, now);
        if status.width.is_none_or(|w| visible_len(&line) <= w) {
            break;
        }
    }

    [head, line].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::Limit;

    #[test]
    fn bar_uses_eighths() {
        assert_eq!(bar(0.0, 10), "░░░░░░░░░░");
        assert_eq!(bar(100.0, 10), "██████████");
        assert_eq!(bar(38.0, 10), "███▊░░░░░░");
        assert_eq!(bar(150.0, 5), "█████");
        assert_eq!(bar(-3.0, 5), "░░░░░");
    }

    #[test]
    fn bar_always_has_requested_width() {
        for pct in 0..=100 {
            assert_eq!(bar(pct as f64, 10).chars().count(), 10, "pct={pct}");
        }
    }

    #[test]
    fn durations() {
        assert_eq!(duration(-5), "0m");
        assert_eq!(duration(30), "1m");
        assert_eq!(duration(46 * 60), "46m");
        assert_eq!(duration(3600), "1h");
        assert_eq!(duration(3600 + 46 * 60), "1h 46m");
        assert_eq!(duration(2 * 86400), "2d");
        assert_eq!(duration(2 * 86400 + 12 * 3600 + 30 * 60), "2d 12h");
    }

    fn sample() -> Status {
        Status {
            model: Some("Opus 5.5".into()),
            effort: Some("high".into()),
            cwd: Some("/home/me/my-project".into()),
            context: Some(38.0),
            limits: vec![
                Limit { label: "5h".into(), used_pct: 66.0, resets_at: Some(1000 + 3600 + 46 * 60) },
                Limit { label: "7d".into(), used_pct: 58.0, resets_at: Some(1000 + 2 * 86400 + 12 * 3600) },
            ],
            width: None,
        }
    }

    #[test]
    fn renders_two_lines() {
        let git = Git { branch: "main".into(), ahead: 2, changed: 3, untracked: 1, ..Default::default() };
        let out = render(&sample(), Some(&git), false, 1000);
        assert_eq!(
            out,
            "Opus 5.5 │ high │ my-project │ main ↑2 ●3 ?1\n\
             ctx ███▊░░░░░░  38% │ 5h ██████▋░░░  66% ↻ 1h 46m │ 7d █████▊░░░░  58% ↻ 2d 12h"
        );
    }

    #[test]
    fn shrinks_to_fit_width() {
        let mut status = sample();
        status.width = Some(60);
        let out = render(&status, None, true, 1000);
        let meters = out.lines().last().unwrap();
        assert!(visible_len(meters) <= 60, "{meters}");
        assert!(!meters.contains('↻'));
    }

    #[test]
    fn visible_len_ignores_ansi() {
        assert_eq!(visible_len("\x1b[31m██\x1b[0m ab"), 5);
    }
}
