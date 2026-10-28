//! Claude Code 與 Antigravity CLI 都在 settings.json 的 `statusLine` 指定外部指令，
//! 兩者的安裝／移除流程相同，只差設定檔位置與 `statusLine` 內容。

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::{Agent, Error, Installed, Removal, Result, state};

/// 用來辨識「這個 statusLine 是 TokenBar 寫的」。
const MARKER: &str = " render ";

pub fn status_line(agent: Agent, command: &str) -> Value {
    match agent {
        Agent::Antigravity => json!({ "type": "command", "command": command, "enabled": true }),
        _ => json!({ "type": "command", "command": command, "padding": 0 }),
    }
}

pub fn is_ours(status_line: Option<&Value>) -> bool {
    status_line
        .and_then(|v| v.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|c| c.to_lowercase().contains("tokenbar") && c.contains(MARKER))
}

fn read_settings(file: &Path) -> Result<Map<String, Value>> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(err) => return Err(err.into()),
    };
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    // 解析失敗時絕不覆寫，避免毀掉使用者的設定。
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(Error(format!("{} 不是 JSON 物件，請先修正此檔再安裝。", file.display()))),
        Err(err) => Err(Error(format!("無法解析 {}：{err}。請先修正此檔再安裝。", file.display()))),
    }
}

/// 先寫暫存檔再改名，避免寫到一半中斷造成設定檔損毀。
pub fn write_atomic(file: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("tokenbar-tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, file)?;
    Ok(())
}

fn write_settings(file: &Path, settings: &Map<String, Value>) -> Result<()> {
    let text = serde_json::to_string_pretty(settings).expect("settings are serializable");
    write_atomic(file, &format!("{text}\n"))
}

pub fn backup(file: &Path) -> Result<Option<PathBuf>> {
    if !file.exists() {
        return Ok(None);
    }
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".tokenbar-bak");
    let backup = file.with_file_name(name);
    fs::copy(file, &backup)?;
    Ok(Some(backup))
}

pub fn install(agent: Agent, file: &Path, command: &str, state_dir: &Path) -> Result<Installed> {
    let mut settings = read_settings(file)?;
    let backup = backup(file)?;

    // 記下原本其他工具的 statusLine；重複安裝時不覆蓋第一次記下的值。
    let existing = settings.get("statusLine").cloned();
    let replaced_other = existing.is_some() && !is_ours(existing.as_ref());
    if replaced_other {
        let mut st = state::read(state_dir);
        st.insert(agent.key().into(), json!({ "previousStatusLine": existing }));
        state::write(state_dir, &st)?;
    }

    settings.insert("statusLine".into(), status_line(agent, command));
    write_settings(file, &settings)?;
    Ok(Installed { file: file.to_path_buf(), backup, replaced_other })
}

pub fn uninstall(agent: Agent, file: &Path, state_dir: &Path) -> Result<Removal> {
    if !file.exists() {
        return Ok(Removal::NotInstalled);
    }
    let mut settings = read_settings(file)?;
    if !is_ours(settings.get("statusLine")) {
        return Ok(Removal::NotInstalled);
    }

    let mut st = state::read(state_dir);
    let previous = st.remove(agent.key()).and_then(|v| v.get("previousStatusLine").cloned()).filter(|v| !v.is_null());
    let removal = match previous {
        Some(previous) => {
            settings.insert("statusLine".into(), previous);
            Removal::Restored
        }
        None => {
            settings.remove("statusLine");
            Removal::Removed
        }
    };
    state::write(state_dir, &st)?;
    write_settings(file, &settings)?;
    Ok(removal)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "/home/me/.tokenbar/bin/tokenbar render claude";

    fn read(file: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
    }

    #[test]
    fn installs_into_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".claude/settings.json");
        let res = install(Agent::Claude, &file, CMD, &dir.path().join("state")).unwrap();
        assert!(res.backup.is_none());
        assert_eq!(read(&file)["statusLine"]["command"], CMD);
    }

    #[test]
    fn keeps_other_keys_in_order_and_restores_previous() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let file = dir.path().join("settings.json");
        let original = r#"{
  "theme": "dark",
  "statusLine": { "type": "command", "command": "~/other-statusline.sh" },
  "model": "opus"
}"#;
        fs::write(&file, original).unwrap();

        let res = install(Agent::Claude, &file, CMD, &state_dir).unwrap();
        assert!(res.replaced_other);
        assert_eq!(fs::read_to_string(res.backup.unwrap()).unwrap(), original);
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.find("theme").unwrap() < text.find("statusLine").unwrap());
        assert!(text.find("statusLine").unwrap() < text.find("model").unwrap());

        // 重複安裝不會把「原本的設定」覆蓋成 TokenBar 自己
        install(Agent::Claude, &file, CMD, &state_dir).unwrap();

        assert_eq!(uninstall(Agent::Claude, &file, &state_dir).unwrap(), Removal::Restored);
        let restored = read(&file);
        assert_eq!(restored["statusLine"]["command"], "~/other-statusline.sh");
        assert_eq!(restored["theme"], "dark");
        assert_eq!(uninstall(Agent::Claude, &file, &state_dir).unwrap(), Removal::NotInstalled);
    }

    #[test]
    fn uninstall_removes_when_nothing_to_restore() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join("state");
        let file = dir.path().join("settings.json");
        fs::write(&file, r#"{ "trustedWorkspaces": ["/w"] }"#).unwrap();
        install(Agent::Antigravity, &file, "tokenbar render antigravity", &state_dir).unwrap();
        assert_eq!(read(&file)["statusLine"]["enabled"], true);
        assert_eq!(uninstall(Agent::Antigravity, &file, &state_dir).unwrap(), Removal::Removed);
        assert_eq!(read(&file), json!({ "trustedWorkspaces": ["/w"] }));
    }

    #[test]
    fn refuses_to_overwrite_broken_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        fs::write(&file, "{ broken").unwrap();
        assert!(install(Agent::Claude, &file, CMD, dir.path()).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{ broken");
    }

    #[test]
    fn leaves_foreign_status_line_alone_on_uninstall() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let text = r#"{"statusLine":{"type":"command","command":"ccstatusline"}}"#;
        fs::write(&file, text).unwrap();
        assert_eq!(uninstall(Agent::Claude, &file, dir.path()).unwrap(), Removal::NotInstalled);
        assert_eq!(fs::read_to_string(&file).unwrap(), text);
    }
}
