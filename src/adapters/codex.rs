//! Codex CLI 的 session 紀錄（`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`）。
//!
//! Codex 的狀態列不能執行外部指令，所以 `tokenbar codex` 從 session 紀錄讀取最新的
//! `token_count` 事件，在獨立視窗或 tmux 狀態列顯示。
//!
//! context 使用率的算法與 Codex 一致：取 `last_token_usage.total_tokens`，分子分母都先扣掉
//! 12,000 個基礎 tokens。參見 openai/codex 的 `codex-rs/protocol/src/protocol.rs`
//! （`percent_of_context_window_remaining`）與 `codex-rs/tui/src/chatwidget.rs`。

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{get, get_f64, get_i64, get_str};
use crate::status::{Limit, Status};
use crate::timeutil;

/// Codex 計算 context 使用率時會扣掉固定的基礎提示詞用量；沿用同樣算法，
/// 讓數字與 Codex 內建的 `context-used` 一致。
const BASELINE_TOKENS: f64 = 12_000.0;
const TAIL_BYTES: u64 = 512 * 1024;
const MAX_FILES: usize = 50;

#[derive(Debug, Default)]
pub struct Session {
    pub meta: Option<Value>,
    pub turn_context: Option<Value>,
    /// 最新的 token_count 事件（context 使用率）
    pub token_count: Option<Value>,
    /// 最新一筆有值的 rate_limits。最新的 token_count 不一定帶額度資料，所以分開找。
    pub rate_limits: Option<Value>,
    /// 最新 token_count 事件的時間（Unix 秒）
    pub updated_at: Option<i64>,
}

pub fn window_label(minutes: i64) -> String {
    match minutes {
        300 => "5h".into(),
        10080 => "7d".into(),
        m if m > 0 && m % 1440 == 0 => format!("{}d", m / 1440),
        m if m > 0 && m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

pub fn context_used_pct(tokens: f64, window: f64) -> f64 {
    if window <= BASELINE_TOKENS {
        return 0.0;
    }
    ((tokens - BASELINE_TOKENS).max(0.0) / (window - BASELINE_TOKENS) * 100.0).min(100.0)
}

pub fn from_session(session: &Session, now: i64) -> Status {
    let empty = Value::Null;
    let tc = session.token_count.as_ref().unwrap_or(&empty);
    let rl = session.rate_limits.as_ref().unwrap_or(&empty);

    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let Some(used) = get_f64(rl, &[key, "used_percent"]) else { continue };
        let minutes = get_i64(rl, &[key, "window_minutes"]);
        let resets_at = get_i64(rl, &[key, "resets_at"]);
        // 紀錄裡的數字可能已經過了重置時間，此時視為已歸零。
        let expired = resets_at.is_some_and(|at| at <= now);
        windows.push((
            minutes.unwrap_or(i64::MAX),
            Limit {
                label: minutes.map_or_else(|| "?".into(), window_label),
                used_pct: if expired { 0.0 } else { used },
                resets_at: if expired { None } else { resets_at },
                window_secs: minutes.map(|m| m * 60),
            },
        ));
    }
    windows.sort_by_key(|(minutes, _)| *minutes);

    let context = get_f64(tc, &["info", "model_context_window"]).map(|window| {
        let tokens = get_f64(tc, &["info", "last_token_usage", "total_tokens"]).unwrap_or(0.0);
        context_used_pct(tokens, window)
    });

    let turn = session.turn_context.as_ref().unwrap_or(&empty);
    let meta = session.meta.as_ref().unwrap_or(&empty);
    Status {
        model: get_str(turn, &["model"]),
        effort: get_str(turn, &["effort"]),
        cwd: get_str(turn, &["cwd"]).or_else(|| get_str(meta, &["cwd"])),
        context,
        limits: windows.into_iter().map(|(_, l)| l).collect(),
        width: None,
        updated_at: session.updated_at,
    }
}

/// 由新到舊列出 rollout 檔案。
pub fn list_rollouts(home: &Path) -> Vec<PathBuf> {
    fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
        let mut dirs: Vec<_> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        dirs.sort_unstable_by(|a, b| b.cmp(a));
        dirs
    }

    let mut result = Vec::new();
    for year in sorted_dirs(&home.join("sessions")) {
        for month in sorted_dirs(&year) {
            for day in sorted_dirs(&month) {
                let mut files: Vec<_> = fs::read_dir(&day)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| {
                        let name = e.file_name();
                        let name = name.to_string_lossy();
                        name.starts_with("rollout-") && name.ends_with(".jsonl")
                    })
                    .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                    .collect();
                files.sort_unstable_by_key(|f| std::cmp::Reverse(f.0));
                result.extend(files.into_iter().map(|(_, p)| p));
                if result.len() >= MAX_FILES {
                    result.truncate(MAX_FILES);
                    return result;
                }
            }
        }
    }
    result
}

fn read_first_line(file: &Path) -> Option<Value> {
    let mut line = String::new();
    BufReader::new(File::open(file).ok()?).read_line(&mut line).ok()?;
    serde_json::from_str(&line).ok()
}

fn read_tail(file: &Path) -> Option<String> {
    let mut f = File::open(file).ok()?;
    let size = f.metadata().ok()?.len();
    let start = size.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((size - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    // 從檔案中間開始讀時，第一行可能被截斷
    Some(if start > 0 { text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_owned()) } else { text })
}

/// 解析一個 rollout 檔，取出 session_meta 與最新的 turn_context、token_count、rate_limits。
pub fn read_session(file: &Path) -> Session {
    let meta = read_first_line(file)
        .filter(|v| v.get("type").and_then(Value::as_str) == Some("session_meta"))
        .and_then(|v| v.get("payload").cloned());
    let mut session = Session { meta, ..Default::default() };

    let Some(tail) = read_tail(file) else { return session };
    for line in tail.lines().rev() {
        if session.turn_context.is_some() && session.token_count.is_some() && session.rate_limits.is_some() {
            break;
        }
        // 先用字串篩選，避免逐行解析整個大檔
        let want_turn = session.turn_context.is_none() && line.contains("\"turn_context\"");
        let want_count =
            (session.token_count.is_none() || session.rate_limits.is_none()) && line.contains("\"token_count\"");
        if !want_turn && !want_count {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        let kind = entry.get("type").and_then(Value::as_str);
        if want_turn && kind == Some("turn_context") {
            session.turn_context = entry.get("payload").cloned();
        } else if want_count
            && kind == Some("event_msg")
            && get_str(&entry, &["payload", "type"]).as_deref() == Some("token_count")
        {
            if session.token_count.is_none() {
                session.token_count = get(&entry, &["payload"]).cloned();
                session.updated_at = get_str(&entry, &["timestamp"]).and_then(|t| timeutil::parse_rfc3339(&t));
            }
            if session.rate_limits.is_none() {
                session.rate_limits = get(&entry, &["payload", "rate_limits"]).cloned();
            }
        }
    }
    session
}

/// 找出最新的 session；給了 `cwd` 時只看在該目錄啟動的 session。
pub fn find_latest(home: &Path, cwd: Option<&Path>) -> Option<Session> {
    list_rollouts(home).into_iter().find_map(|file| {
        if let Some(cwd) = cwd {
            let meta = read_first_line(&file)?;
            let started = get_str(&meta, &["payload", "cwd"])?;
            if !same_path(Path::new(&started), cwd) {
                return None;
            }
        }
        Some(read_session(&file))
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn labels() {
        assert_eq!(window_label(300), "5h");
        assert_eq!(window_label(10080), "7d");
        assert_eq!(window_label(1440), "1d");
        assert_eq!(window_label(120), "2h");
        assert_eq!(window_label(45), "45m");
    }

    #[test]
    fn context_matches_codex_formula() {
        assert_eq!(context_used_pct(12_000.0, 272_000.0), 0.0);
        assert_eq!(context_used_pct(142_000.0, 272_000.0), 50.0);
        assert_eq!(context_used_pct(999_999.0, 272_000.0), 100.0);
        assert_eq!(context_used_pct(5.0, 0.0), 0.0);
    }

    fn token_count(primary: Value, secondary: Value) -> Value {
        json!({
            "type": "token_count",
            "info": { "last_token_usage": { "total_tokens": 142000 }, "model_context_window": 272000 },
            "rate_limits": { "primary": primary, "secondary": secondary }
        })
    }

    fn rate_limits(primary: Value, secondary: Value) -> Option<Value> {
        Some(json!({ "primary": primary, "secondary": secondary }))
    }

    #[test]
    fn sorts_windows_and_drops_expired() {
        let session = Session {
            turn_context: Some(json!({ "model": "gpt-6.1", "cwd": "/w/app" })),
            token_count: Some(token_count(Value::Null, Value::Null)),
            rate_limits: rate_limits(
                json!({ "used_percent": 40.0, "window_minutes": 10080, "resets_at": 5000 }),
                json!({ "used_percent": 90.0, "window_minutes": 300, "resets_at": 500 }),
            ),
            ..Default::default()
        };
        let status = from_session(&session, 1000);
        assert_eq!(status.model.as_deref(), Some("gpt-6.1"));
        assert_eq!(status.context, Some(50.0));
        assert_eq!(
            status.limits,
            vec![
                Limit { label: "5h".into(), used_pct: 0.0, resets_at: None, window_secs: Some(18000) },
                Limit { label: "7d".into(), used_pct: 40.0, resets_at: Some(5000), window_secs: Some(604800) },
            ]
        );
    }

    #[test]
    fn reads_latest_session_from_disk() {
        let home = tempfile::tempdir().unwrap();
        let day = home.path().join("sessions/2026/10/28");
        fs::create_dir_all(&day).unwrap();
        let mut f = File::create(day.join("rollout-2026-10-28T05-36-47-abc.jsonl")).unwrap();
        let lines = [
            json!({ "type": "session_meta", "payload": { "cwd": "/w/app" } }),
            json!({ "type": "turn_context", "payload": { "model": "old-model" } }),
            json!({ "type": "event_msg", "payload": token_count(json!({ "used_percent": 1.0 }), Value::Null) }),
            json!({ "type": "turn_context", "payload": { "model": "gpt-6.1" } }),
            json!({ "type": "event_msg", "payload": token_count(json!({ "used_percent": 7.0, "window_minutes": 10080 }), Value::Null) }),
            // 最新一筆沒有額度資料：context 用這筆，額度沿用上一筆
            json!({
                "timestamp": "2026-10-28T06:39:33.291Z",
                "type": "event_msg",
                "payload": {
                    "type": "token_count",
                    "info": { "last_token_usage": { "total_tokens": 12000 }, "model_context_window": 272000 },
                    "rate_limits": null
                }
            }),
            json!({ "type": "response_item", "payload": { "type": "message" } }),
        ];
        for line in lines {
            writeln!(f, "{line}").unwrap();
        }
        drop(f);

        let session = find_latest(home.path(), None).expect("session");
        let status = from_session(&session, 0);
        assert_eq!(status.model.as_deref(), Some("gpt-6.1"));
        assert_eq!(status.cwd.as_deref(), Some("/w/app"));
        assert_eq!(status.limits[0].used_pct, 7.0);
        assert_eq!(status.context, Some(0.0));
        assert_eq!(status.updated_at, timeutil::parse_rfc3339("2026-10-28T06:39:33Z"));
        assert!(find_latest(home.path(), Some(Path::new("/elsewhere"))).is_none());
    }
}
