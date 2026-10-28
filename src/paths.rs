//! 各 Agent 設定檔位置，以及寫進設定檔的狀態列指令。

use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub claude_settings: PathBuf,
    pub antigravity_settings: PathBuf,
    pub codex_home: PathBuf,
    pub state_dir: PathBuf,
}

pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

impl Paths {
    pub fn from_env() -> Self {
        Self::for_home(&home_dir())
    }

    pub fn for_home(home: &Path) -> Self {
        Paths {
            claude_settings: env_path("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|| home.join(".claude"))
                .join("settings.json"),
            antigravity_settings: home.join(".gemini").join("antigravity-cli").join("settings.json"),
            codex_home: env_path("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            state_dir: env_path("TOKENBAR_HOME").unwrap_or_else(|| home.join(".tokenbar")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Unix,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) { Platform::Windows } else { Platform::Unix }
    }
}

fn quote(s: &str) -> String {
    if s.contains(|c: char| c.is_whitespace() || "\"&|<>^()'$`".contains(c)) {
        format!("\"{s}\"")
    } else {
        s.to_owned()
    }
}

/// 組出 statusLine 要執行的指令，例如 `/home/me/.tokenbar/bin/tokenbar render claude`。
///
/// Windows 上兩個 CLI 執行指令的方式不同，路徑寫法也不同：
///
/// - Claude Code 透過 Git Bash 執行：反斜線會被當成跳脫字元，所以改用正斜線。
/// - Antigravity 透過 `cmd /c` 執行：正斜線會被當成選項開頭（`C:/Users/...` 會被拆開），
///   所以保留反斜線。另外指令含兩個以上引號時 cmd 會剝掉最外層的一對，所以整段再包一層引號抵銷。
pub fn status_command(exe: &Path, agent: &str, platform: Platform) -> String {
    let mut exe = exe.to_string_lossy().into_owned();
    let via_cmd = platform == Platform::Windows && agent == "antigravity";
    if platform == Platform::Windows {
        exe = exe.trim_start_matches(r"\\?\").to_owned();
        if !via_cmd {
            exe = exe.replace('\\', "/");
        }
    }
    let command = format!("{} render {agent}", quote(&exe));
    if via_cmd && command.contains('"') { format!("\"{command}\"") } else { command }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_command() {
        let exe = Path::new("/home/me/.tokenbar/bin/tokenbar");
        assert_eq!(status_command(exe, "claude", Platform::Unix), "/home/me/.tokenbar/bin/tokenbar render claude");
        let spaced = Path::new("/Users/John Doe/.tokenbar/bin/tokenbar");
        assert_eq!(
            status_command(spaced, "antigravity", Platform::Unix),
            "\"/Users/John Doe/.tokenbar/bin/tokenbar\" render antigravity"
        );
    }

    #[test]
    fn windows_claude_uses_forward_slashes_for_git_bash() {
        let exe = Path::new(r"C:\Users\me\.tokenbar\bin\tokenbar.exe");
        assert_eq!(
            status_command(exe, "claude", Platform::Windows),
            "C:/Users/me/.tokenbar/bin/tokenbar.exe render claude"
        );
    }

    #[test]
    fn windows_antigravity_keeps_backslashes_for_cmd() {
        let exe = Path::new(r"C:\Users\RUNNER~1\.tokenbar\bin\tokenbar.exe");
        assert_eq!(
            status_command(exe, "antigravity", Platform::Windows),
            r"C:\Users\RUNNER~1\.tokenbar\bin\tokenbar.exe render antigravity"
        );
    }

    #[test]
    fn windows_antigravity_survives_cmd_quote_stripping() {
        let exe = Path::new(r"C:\Users\John Doe\.tokenbar\bin\tokenbar.exe");
        assert_eq!(
            status_command(exe, "claude", Platform::Windows),
            "\"C:/Users/John Doe/.tokenbar/bin/tokenbar.exe\" render claude"
        );
        assert_eq!(
            status_command(exe, "antigravity", Platform::Windows),
            r#"""C:\Users\John Doe\.tokenbar\bin\tokenbar.exe" render antigravity""#
        );
    }
}
