//! 三個 Agent 共用的正規化狀態，由 `adapters` 產生、`render` 消費。

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub cwd: Option<String>,
    /// context window 使用率（0–100）
    pub context: Option<f64>,
    pub limits: Vec<Limit>,
    /// 終端機寬度；未知時為 None，不做縮減
    pub width: Option<usize>,
    /// 資料的時間（Unix 秒）；只有讀取紀錄檔的 Codex 會有，用來標示資料多舊
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    /// 顯示用標籤，例如 `5h`、`7d`
    pub label: String,
    /// 使用率（0–100）
    pub used_pct: f64,
    /// 重置時間（Unix 秒）
    pub resets_at: Option<i64>,
    /// 時間窗長度（秒），例如 5 小時為 18000；用來計算用量速度
    pub window_secs: Option<i64>,
}

/// 依標籤推得的時間窗長度（秒）。
pub fn window_for_label(label: &str) -> Option<i64> {
    match label {
        "5h" => Some(5 * 3600),
        "1d" => Some(86400),
        "7d" => Some(7 * 86400),
        _ => None,
    }
}

impl Limit {
    /// 用量速度：已用額度與已經過時間的百分點差。
    ///
    /// 正值代表用得比時間快，照這個速度會在重置前用完；負值代表還有餘裕。
    /// 例如 7 天的時間窗過了 3.5 天（50%）、已用 56%，速度就是 +6。
    pub fn pace(&self, now: i64) -> Option<f64> {
        let window = self.window_secs.filter(|w| *w > 0)?;
        let remaining = (self.resets_at? - now).clamp(0, window);
        let elapsed_pct = (window - remaining) as f64 / window as f64 * 100.0;
        Some(self.used_pct - elapsed_pct)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Git {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub changed: u32,
    pub untracked: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(used_pct: f64, resets_at: Option<i64>, window_secs: Option<i64>) -> Limit {
        Limit { label: "7d".into(), used_pct, resets_at, window_secs }
    }

    #[test]
    fn pace_compares_usage_with_elapsed_time() {
        let week = 7 * 86400;
        // 過了一半時間、用了 56%：快了 6 個百分點
        assert_eq!(limit(56.0, Some(week / 2), Some(week)).pace(0), Some(6.0));
        // 剛開始就用了 10%
        assert_eq!(limit(10.0, Some(week), Some(week)).pace(0), Some(10.0));
        // 快重置了才用 30%
        assert_eq!(limit(30.0, Some(0), Some(week)).pace(0), Some(-70.0));
    }

    #[test]
    fn pace_needs_window_and_reset() {
        assert_eq!(limit(50.0, None, Some(100)).pace(0), None);
        assert_eq!(limit(50.0, Some(10), None).pace(0), None);
    }
}
