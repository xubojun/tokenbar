//! `tokenbar update`：更新到 GitHub 上的最新版本。
//!
//! 直接重跑官方安裝腳本（與一鍵安裝相同），所以平台判斷、下載與校驗都沿用它。
//! 各 CLI 的設定指向固定的執行檔位置，更新後不需要重新設定。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::paths::home_dir;

const REPO: &str = "https://github.com/xubojun/tokenbar";

/// 解析 `v1.2.3` 或 `1.2.3`。
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// 從 `https://github.com/<owner>/<repo>/releases/tag/v0.1.2` 取出 `v0.1.2`。
pub fn tag_from_url(url: &str) -> Option<&str> {
    let tag = url.trim().rsplit_once("/releases/tag/")?.1;
    (!tag.is_empty()).then_some(tag)
}

/// 透過 `releases/latest` 的轉址查出最新版本，不呼叫 GitHub API（避免未登入時的頻率限制）。
fn latest_tag() -> Result<String, String> {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = Command::new("curl")
        .args(["-fsSIL", "-o", null, "-w", "%{url_effective}", &format!("{REPO}/releases/latest")])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("無法執行 curl：{e}"))?;
    if !output.status.success() {
        return Err("無法連線到 GitHub 查詢最新版本".into());
    }
    let url = String::from_utf8_lossy(&output.stdout);
    tag_from_url(&url).map(str::to_owned).ok_or_else(|| format!("無法從 {url} 判斷最新版本"))
}

fn installer_command(tag: &str) -> Command {
    let base = format!("{REPO}/releases/download/{tag}");
    let mut cmd = if cfg!(windows) {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!("irm {base}/tokenbar-installer.ps1 | iex"),
        ]);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &format!("curl -fsSL {base}/tokenbar-installer.sh | sh")]);
        cmd
    };
    // 已經在 PATH 上了，不必再改 shell 設定檔
    cmd.env("TOKENBAR_NO_MODIFY_PATH", "1");
    cmd
}

fn default_install_dir() -> PathBuf {
    home_dir().join(".tokenbar").join("bin")
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// 上次更新留下的舊執行檔（Windows 無法刪除執行中的程式，只能下次再清）。
pub fn remove_leftovers() {
    if let Ok(exe) = env::current_exe() {
        let _ = fs::remove_file(exe.with_extension("old"));
    }
}

pub fn update(check_only: bool) -> Result<(), String> {
    let current = env!("CARGO_PKG_VERSION");
    let tag = latest_tag()?;
    let latest = parse_version(&tag).ok_or_else(|| format!("無法解析版本號 {tag}"))?;
    if latest <= parse_version(current).expect("crate version is semver") {
        println!("已是最新版本（{current}）");
        return Ok(());
    }
    if check_only {
        println!("有新版本：{current} → {}\n執行 tokenbar update 即可更新。", tag.trim_start_matches('v'));
        return Ok(());
    }

    let exe = env::current_exe().map_err(|e| format!("找不到 tokenbar 執行檔位置：{e}"))?;
    let install_dir = default_install_dir();
    if !exe.parent().is_some_and(|dir| same_dir(dir, &install_dir)) {
        return Err(format!(
            "目前的 tokenbar（{}）不是用安裝腳本裝在 {} 的，請用原本的安裝方式更新。",
            exe.display(),
            install_dir.display()
        ));
    }

    println!("更新 tokenbar：{current} → {}", tag.trim_start_matches('v'));
    // 先把執行中的程式改名讓出位置：Windows 不能覆寫執行中的 exe，Linux 也可能回報 Text file busy
    let old = exe.with_extension("old");
    let _ = fs::remove_file(&old);
    fs::rename(&exe, &old).map_err(|e| format!("無法替換執行檔：{e}"))?;

    let status = installer_command(&tag).status();
    if !status.as_ref().is_ok_and(|s| s.success()) || !exe.exists() {
        let _ = fs::rename(&old, &exe);
        return Err(match status {
            Ok(_) => "安裝腳本執行失敗，已保留原本的版本".into(),
            Err(e) => format!("無法執行安裝腳本：{e}，已保留原本的版本"),
        });
    }
    remove_leftovers();
    println!("\n✔ 已更新到 {}，重新啟動各 CLI 後生效。", tag.trim_start_matches('v'));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("v0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("1.10.0"), Some((1, 10, 0)));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3-beta"), None);
        assert!(parse_version("v0.1.10") > parse_version("v0.1.9"));
    }

    #[test]
    fn extracts_tag_from_redirect() {
        assert_eq!(tag_from_url("https://github.com/xubojun/tokenbar/releases/tag/v0.1.2"), Some("v0.1.2"));
        assert_eq!(tag_from_url("https://github.com/xubojun/tokenbar/releases"), None);
        assert_eq!(tag_from_url("https://github.com/xubojun/tokenbar/releases/tag/"), None);
    }
}
