//! 工作目錄的 git 狀態。三個 Agent 的 stdin 都沒有完整的 git 資訊，所以自己查。

use std::path::Path;
use std::process::{Command, Stdio};

use crate::status::Git;

/// 解析 `git status --porcelain=v2 --branch` 的輸出。
pub fn parse_status(output: &str) -> Option<Git> {
    let mut git = Git::default();
    let mut oid = "";
    for line in output.lines() {
        if let Some(v) = line.strip_prefix("# branch.oid ") {
            oid = v;
        } else if let Some(v) = line.strip_prefix("# branch.head ") {
            git.branch = v.to_owned();
        } else if let Some(v) = line.strip_prefix("# branch.ab ") {
            let mut it = v.split_whitespace();
            git.ahead = it.next().and_then(|s| s.trim_start_matches('+').parse().ok()).unwrap_or(0);
            git.behind = it.next().and_then(|s| s.trim_start_matches('-').parse().ok()).unwrap_or(0);
        } else if line.starts_with("1 ") || line.starts_with("2 ") || line.starts_with("u ") {
            git.changed += 1;
        } else if line.starts_with("? ") {
            git.untracked += 1;
        }
    }
    if git.branch == "(detached)" {
        git.branch = if oid.len() >= 7 { oid[..7].to_owned() } else { "detached".into() };
    }
    (!git.branch.is_empty()).then_some(git)
}

pub fn read(cwd: &Path) -> Option<Git> {
    let mut cmd = Command::new("git");
    cmd.args(["--no-optional-locks", "status", "--porcelain=v2", "--branch"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None; // 不是 git repo
    }
    parse_status(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_and_counts() {
        let out = "# branch.oid 0123456789abcdef\n# branch.head main\n# branch.upstream origin/main\n\
                   # branch.ab +2 -1\n1 .M N... 100644 100644 100644 a b src/x.rs\n\
                   2 R. N... 100644 100644 100644 a b R100 y z\n? new.txt\n? other.txt\n";
        assert_eq!(
            parse_status(out),
            Some(Git { branch: "main".into(), ahead: 2, behind: 1, changed: 2, untracked: 2 })
        );
    }

    #[test]
    fn detached_head_shows_short_hash() {
        let out = "# branch.oid 0123456789abcdef\n# branch.head (detached)\n";
        assert_eq!(parse_status(out).unwrap().branch, "0123456");
    }

    #[test]
    fn empty_output_is_none() {
        assert_eq!(parse_status(""), None);
    }
}
