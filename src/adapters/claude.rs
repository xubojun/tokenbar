//! Claude Code statusLine 的 stdin JSON。
//! 欄位說明：<https://code.claude.com/docs/en/statusline>

use serde_json::Value;

use super::{first_str, get, get_f64, get_i64, get_str};
use crate::status::{Limit, Status, window_for_label};

pub fn from_payload(payload: &Value) -> Status {
    let mut limits = Vec::new();
    for (label, key) in [("5h", "five_hour"), ("7d", "seven_day"), ("$", "spend_limit")] {
        if let Some(used_pct) = get_f64(payload, &["rate_limits", key, "used_percentage"]) {
            limits.push(Limit {
                label: label.into(),
                used_pct,
                resets_at: get_i64(payload, &["rate_limits", key, "resets_at"]),
                window_secs: window_for_label(label),
            });
        }
    }

    Status {
        model: first_str(payload, &[&["model", "display_name"], &["model", "id"]]),
        effort: get_str(payload, &["effort", "level"]),
        cwd: first_str(payload, &[&["workspace", "current_dir"], &["cwd"]]),
        // 第一次回應前 used_percentage 為 null，此時視為 0%
        context: get(payload, &["context_window"])
            .map(|_| get_f64(payload, &["context_window", "used_percentage"]).unwrap_or(0.0)),
        limits,
        width: None,
        updated_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_payload() {
        let status = from_payload(&json!({
            "model": { "id": "claude-opus-5-5", "display_name": "Opus 5.5" },
            "effort": { "level": "high" },
            "workspace": { "current_dir": "/w/app", "project_dir": "/w" },
            "context_window": { "used_percentage": 8, "context_window_size": 200000 },
            "rate_limits": {
                "five_hour": { "used_percentage": 23.5, "resets_at": 1738425600 },
                "seven_day": { "used_percentage": 41.2, "resets_at": 1738857600 }
            }
        }));
        assert_eq!(status.model.as_deref(), Some("Opus 5.5"));
        assert_eq!(status.effort.as_deref(), Some("high"));
        assert_eq!(status.cwd.as_deref(), Some("/w/app"));
        assert_eq!(status.context, Some(8.0));
        assert_eq!(status.limits.len(), 2);
        assert_eq!(
            status.limits[0],
            Limit { label: "5h".into(), used_pct: 23.5, resets_at: Some(1738425600), window_secs: Some(18000) }
        );
        assert_eq!(status.limits[1].label, "7d");
    }

    #[test]
    fn api_key_users_have_no_rate_limits() {
        let status = from_payload(&json!({
            "model": { "id": "claude-sonnet-5-5" },
            "cwd": "/w",
            "context_window": { "used_percentage": null }
        }));
        assert_eq!(status.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(status.context, Some(0.0));
        assert!(status.limits.is_empty());
    }
}
