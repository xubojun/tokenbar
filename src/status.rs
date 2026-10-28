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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    /// 顯示用標籤，例如 `5h`、`7d`
    pub label: String,
    /// 使用率（0–100）
    pub used_pct: f64,
    /// 重置時間（Unix 秒）
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Git {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub changed: u32,
    pub untracked: u32,
}
