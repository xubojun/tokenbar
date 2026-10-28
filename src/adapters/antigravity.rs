//! Antigravity CLI statusLine 的 stdin JSON。
//! 欄位說明：<https://antigravity.google/docs/cli/statusline/>
//!
//! 額度（`quota`）是「bucket id → 狀態」的對照表，bucket 名稱由伺服器決定。實際收到的例子：
//!
//! ```text
//! gemini-5h, gemini-weekly, gemini-argon-wtu-weekly   ← Gemini 模型（含個別模型自己的額度）
//! 3p-5h, 3p-weekly                                    ← 第三方模型
//! ```
//!
//! 所以只取目前模型那一家（`gemini-` 或 `3p-`）的 bucket，依名稱推測時間窗，
//! 同一個時間窗有多個 bucket 時顯示用得最多的那個——它最先把你卡住。

use serde_json::Value;

use super::{first_str, get, get_f64, get_i64, get_str};
use crate::status::{Limit, Status};
use crate::timeutil;

const ORDER: [&str; 3] = ["5h", "1d", "7d"];

pub fn bucket_label(id: &str) -> String {
    let name = id.to_lowercase();
    if name.contains("week") || name.contains("7d") {
        "7d".into()
    } else if name.contains("5h") || name.contains("five-hour") || name.contains("five_hour") {
        "5h".into()
    } else if name.contains("day") || name.contains("daily") || name.contains("1d") {
        "1d".into()
    } else {
        name.strip_prefix("gemini-").unwrap_or(&name).chars().take(10).collect()
    }
}

fn rank(label: &str) -> usize {
    ORDER.iter().position(|l| *l == label).unwrap_or(ORDER.len())
}

/// 目前模型對應的 bucket 前綴：Gemini 模型用 `gemini-`，其他（第三方）模型用 `3p-`。
fn family_prefix(model: Option<&str>) -> Option<&'static str> {
    let model = model?.to_lowercase();
    Some(if model.contains("gemini") { "gemini-" } else { "3p-" })
}

fn quota_limits(quota: &serde_json::Map<String, Value>, model: Option<&str>, now: i64) -> Vec<Limit> {
    let mut all = Vec::new();
    for (id, bucket) in quota {
        if bucket.get("disabled").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        // 只有剩餘量（remaining_amount）沒有總量時算不出百分比，略過。
        let Some(remaining) = get_f64(bucket, &["remaining_fraction"]) else { continue };
        let resets_at = get_str(bucket, &["reset_time"])
            .and_then(|t| timeutil::parse_rfc3339(&t))
            .or_else(|| get_i64(bucket, &["reset_in_seconds"]).map(|s| now + s));
        all.push((id.as_str(), Limit { label: bucket_label(id), used_pct: (1.0 - remaining) * 100.0, resets_at }));
    }

    // 有目前模型那一家的 bucket 就只看那一家；沒有就全部一起比較。
    if let Some(prefix) = family_prefix(model)
        && all.iter().any(|(id, _)| id.starts_with(prefix))
    {
        all.retain(|(id, _)| id.starts_with(prefix));
    }

    let mut limits: Vec<Limit> = Vec::new();
    for (_, limit) in all {
        match limits.iter_mut().find(|l| l.label == limit.label) {
            Some(existing) if existing.used_pct >= limit.used_pct => {}
            Some(existing) => *existing = limit,
            None => limits.push(limit),
        }
    }
    limits.sort_by(|a, b| rank(&a.label).cmp(&rank(&b.label)).then_with(|| a.label.cmp(&b.label)));
    limits
}

pub fn from_payload(payload: &Value, now: i64) -> Status {
    let model = first_str(payload, &[&["model", "display_name"], &["model", "id"]]);
    let limits = match get(payload, &["quota"]) {
        Some(Value::Object(quota)) => quota_limits(quota, model.as_deref(), now),
        _ => Vec::new(),
    };

    Status {
        model,
        effort: get_str(payload, &["model", "effort"]),
        cwd: first_str(payload, &[&["workspace", "current_dir"], &["cwd"]]),
        context: get(payload, &["context_window"])
            .map(|_| get_f64(payload, &["context_window", "used_percentage"]).unwrap_or(0.0)),
        limits,
        width: get_i64(payload, &["terminal_width"]).and_then(|w| usize::try_from(w).ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn labels() {
        assert_eq!(bucket_label("gemini-weekly"), "7d");
        assert_eq!(bucket_label("gemini-5h"), "5h");
        assert_eq!(bucket_label("gemini-daily"), "1d");
        assert_eq!(bucket_label("gemini-something-long"), "something-");
    }

    #[test]
    fn full_payload() {
        let status = from_payload(
            &json!({
                "model": { "id": "gemini-3-pro", "display_name": "Gemini 3 Pro", "effort": "high" },
                "workspace": { "current_dir": "/w/app" },
                "context_window": { "used_percentage": 12.5 },
                "terminal_width": 120,
                "quota": {
                    "gemini-weekly": { "remaining_fraction": 0.75, "reset_time": "2026-07-06T07:50:32Z" },
                    "gemini-5h": { "remaining_fraction": 0.5, "reset_in_seconds": 600 },
                    "gemini-off": { "remaining_fraction": 1.0, "disabled": true },
                    "gemini-credits": { "remaining_amount": 42 }
                }
            }),
            1_000,
        );
        assert_eq!(status.model.as_deref(), Some("Gemini 3 Pro"));
        assert_eq!(status.effort.as_deref(), Some("high"));
        assert_eq!(status.context, Some(12.5));
        assert_eq!(status.width, Some(120));
        assert_eq!(
            status.limits,
            vec![
                Limit { label: "5h".into(), used_pct: 50.0, resets_at: Some(1_600) },
                Limit { label: "7d".into(), used_pct: 25.0, resets_at: Some(1_783_324_232) },
            ]
        );
    }

    /// 依 Antigravity CLI 1.4.9 實際送出的 quota 結構
    fn real_quota() -> Value {
        json!({
            "3p-5h": { "remaining_fraction": 1, "reset_in_seconds": 17999 },
            "3p-weekly": { "remaining_fraction": 0.5, "reset_in_seconds": 604799 },
            "gemini-5h": { "remaining_fraction": 0.885, "reset_in_seconds": 106 },
            "gemini-argon-wtu-weekly": { "remaining_fraction": 0.889, "reset_in_seconds": 590609 },
            "gemini-weekly": { "remaining_fraction": 0.967, "reset_in_seconds": 586906 }
        })
    }

    #[test]
    fn gemini_model_shows_most_used_gemini_bucket_per_window() {
        let status =
            from_payload(&json!({ "model": { "display_name": "Gemini 4 Argon (Medium)" }, "quota": real_quota() }), 0);
        let got: Vec<_> = status.limits.iter().map(|l| (l.label.as_str(), l.used_pct.round(), l.resets_at)).collect();
        assert_eq!(got, vec![("5h", 12.0, Some(106)), ("7d", 11.0, Some(590609))]);
    }

    #[test]
    fn third_party_model_shows_3p_buckets() {
        let status = from_payload(&json!({ "model": { "display_name": "Claude Opus 5.5" }, "quota": real_quota() }), 0);
        let got: Vec<_> = status.limits.iter().map(|l| (l.label.as_str(), l.used_pct.round())).collect();
        assert_eq!(got, vec![("5h", 0.0), ("7d", 50.0)]);
    }

    #[test]
    fn minimal_payload() {
        let status = from_payload(&json!({ "model": null, "cwd": "/w" }), 0);
        assert_eq!(status.model, None);
        assert_eq!(status.context, None);
        assert!(status.limits.is_empty());
    }
}
