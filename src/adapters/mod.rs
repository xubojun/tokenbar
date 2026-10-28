//! 各 Agent 的原始資料 → [`Status`](crate::status::Status)。

pub mod antigravity;
pub mod claude;
pub mod codex;

use serde_json::Value;

/// 依路徑取出巢狀欄位，例如 `get(v, &["model", "display_name"])`。
pub(crate) fn get<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |v, key| v.get(key)).filter(|v| !v.is_null())
}

pub(crate) fn get_str(value: &Value, path: &[&str]) -> Option<String> {
    get(value, path)?.as_str().map(str::to_owned)
}

pub(crate) fn get_f64(value: &Value, path: &[&str]) -> Option<f64> {
    get(value, path)?.as_f64()
}

pub(crate) fn get_i64(value: &Value, path: &[&str]) -> Option<i64> {
    let v = get(value, path)?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

/// 第一個有值的路徑。
pub(crate) fn first_str(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|p| get_str(value, p))
}
