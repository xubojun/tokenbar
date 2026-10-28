//! `tokenbar uninstall --purge`：移除 TokenBar 本身。
//!
//! 一鍵安裝腳本（由 dist 產生）會做三件事，這裡逐一還原：
//! 1. 把執行檔放進 `~/.tokenbar/bin`
//! 2. 把該目錄加進 PATH：Unix 在 shell 設定檔加一行 `. "$HOME/.tokenbar/bin/env"`；
//!    Windows 寫進 `HKCU\Environment` 的 Path
//! 3. 寫一份安裝紀錄（receipt）到 `~/.config/tokenbar` 或 `%LOCALAPPDATA%\tokenbar`

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths::{Paths, home_dir};

/// 安裝腳本可能寫入 PATH 設定的 shell 設定檔（相對於家目錄）。
const RC_FILES: [&str; 6] = [".profile", ".bashrc", ".bash_profile", ".bash_login", ".zshrc", ".zshenv"];

/// 是否為安裝腳本加入的那一行，例如 `. "$HOME/.tokenbar/bin/env"` 或 `source "$HOME/.tokenbar/bin/env"`。
fn is_path_line(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with(". ") || line.starts_with("source ")) && line.contains(".tokenbar/bin/env")
}

/// 移除 rc 檔內容中 TokenBar 加入的 PATH 行（連同安裝腳本在它前面補的空行）。
pub fn strip_path_lines(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if !lines.iter().any(|l| is_path_line(l)) {
        return None;
    }
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    for line in lines {
        if is_path_line(line) {
            if out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            continue;
        }
        out.push(line);
    }
    Some(out.concat())
}

fn clean_rc_files(home: &Path) -> io::Result<Vec<PathBuf>> {
    let mut cleaned = Vec::new();
    for name in RC_FILES {
        let file = home.join(name);
        let Ok(text) = fs::read_to_string(&file) else { continue };
        if let Some(new_text) = strip_path_lines(&text) {
            fs::write(&file, new_text)?;
            cleaned.push(file);
        }
    }
    let fish = home.join(".config/fish/conf.d/tokenbar.env.fish");
    if fish.exists() {
        fs::remove_file(&fish)?;
        cleaned.push(fish);
    }
    Ok(cleaned)
}

#[cfg(windows)]
fn clean_windows_path(dir: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let dir = dir.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$k = 'HKCU:\\Environment'; \
         $v = (Get-Item $k).GetValue('Path', '', 'DoNotExpandEnvironmentNames'); \
         $n = ($v -split ';' | Where-Object {{ $_ -and ($_.TrimEnd('\\') -ne '{dir}'.TrimEnd('\\')) }}) -join ';'; \
         if ($n -ne $v) {{ Set-ItemProperty -Path $k -Name Path -Value $n -Type ExpandString }}"
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if status.success() { Ok(()) } else { Err(io::Error::other("無法從使用者 PATH 移除 TokenBar")) }
}

fn receipt_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(xdg).join("tokenbar"));
    }
    if let Some(local) = env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(local).join("tokenbar"));
    }
    dirs.push(home_dir().join(".config").join("tokenbar"));
    dirs
}

pub fn purge(p: &Paths) -> io::Result<()> {
    let exe = env::current_exe()?;
    let bin_dir = exe.parent().map(Path::to_path_buf);

    for file in clean_rc_files(&home_dir())? {
        println!("✔ 已從 {} 移除 PATH 設定", file.display());
    }
    #[cfg(windows)]
    if let Some(dir) = &bin_dir {
        clean_windows_path(dir)?;
        println!("✔ 已從使用者 PATH 移除 {}", dir.display());
    }

    for dir in receipt_dirs() {
        if dir.join("tokenbar-receipt.json").exists() {
            fs::remove_dir_all(&dir)?;
        }
    }

    // 執行中的程式在 Windows 不能直接刪除，self_replace 會在程式結束後代為刪除。
    self_replace::self_delete_outside_path(&p.state_dir)?;
    if p.state_dir.exists() {
        fs::remove_dir_all(&p.state_dir)?;
    }
    if let Some(dir) = bin_dir.filter(|d| fs::read_dir(d).is_ok_and(|mut it| it.next().is_none())) {
        let _ = fs::remove_dir(dir);
    }
    println!("✔ 已移除 TokenBar（{}）\n\n重新開啟終端機後 PATH 變更才會生效。", exe.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_installer_lines() {
        let rc = "export EDITOR=vim\n\n. \"$HOME/.tokenbar/bin/env\"\nalias ll='ls -l'\n";
        assert_eq!(strip_path_lines(rc).unwrap(), "export EDITOR=vim\nalias ll='ls -l'\n");
        let zsh = "source \"$HOME/.tokenbar/bin/env\"\n";
        assert_eq!(strip_path_lines(zsh).unwrap(), "");
    }

    #[test]
    fn leaves_unrelated_files_alone() {
        assert_eq!(strip_path_lines(". \"$HOME/.cargo/env\"\n"), None);
        assert_eq!(strip_path_lines("# .tokenbar/bin/env 只是註解\n"), None);
    }

    #[test]
    fn cleans_rc_files_on_disk() {
        let home = tempfile::tempdir().unwrap();
        fs::write(home.path().join(".bashrc"), "a\n\n. \"$HOME/.tokenbar/bin/env\"\n").unwrap();
        fs::write(home.path().join(".zshrc"), "b\n").unwrap();
        let cleaned = clean_rc_files(home.path()).unwrap();
        assert_eq!(cleaned, vec![home.path().join(".bashrc")]);
        assert_eq!(fs::read_to_string(home.path().join(".bashrc")).unwrap(), "a\n");
        assert_eq!(fs::read_to_string(home.path().join(".zshrc")).unwrap(), "b\n");
    }
}
