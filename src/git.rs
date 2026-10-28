//! 工作目錄的 git 狀態。三個 Agent 的 stdin 都沒有完整的 git 資訊，所以自己查。
//!
//! 狀態列會被頻繁呼叫，而 `git status` 在大型 repo、網路磁碟或防毒掃描下可能卡住好幾秒
//! （Antigravity 超過 5 秒就中止，連續失敗 30 次還會停用狀態列）。所以：
//! - 每個目錄的結果快取 [`FRESH`]，期間內直接沿用
//! - `git` 最多等 [`TIMEOUT`]；逾時就沿用 [`STALE`] 內的舊結果，再舊就不顯示 git

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::status::Git;
use crate::timeutil;

const FRESH: i64 = 2;
const STALE: i64 = 300;
const TIMEOUT: Duration = Duration::from_millis(1000);

/// 不是 git repo 時寫進快取的內容，避免每次都重跑一次失敗的 git。
const NOT_A_REPO: &str = "-";

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

enum Outcome {
    /// git 的輸出；不是 repo 時為 [`NOT_A_REPO`]
    Done(String),
    TimedOut,
}

/// 執行 `git status`，超過 `timeout` 就終止。
fn run_git(cwd: &Path, timeout: Duration) -> Outcome {
    let mut cmd = Command::new("git");
    cmd.args(["--no-optional-locks", "status", "--porcelain=v2", "--branch"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = cmd.spawn() else { return Outcome::Done(NOT_A_REPO.into()) }; // 沒裝 git

    // 另開執行緒讀輸出：變更很多時輸出可能塞滿管線，git 會卡住等人讀
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = reader.join().unwrap_or_default();
                return if status.success() {
                    Outcome::Done(String::from_utf8_lossy(&output).into_owned())
                } else {
                    Outcome::Done(NOT_A_REPO.into())
                };
            }
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(2)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Outcome::TimedOut;
            }
        }
    }
}

fn cache_file(cache_dir: &Path, cwd: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    cache_dir.join(format!("git-{:016x}", hasher.finish()))
}

/// 快取檔格式：第一行是寫入時間（Unix 秒），其餘是 git 的輸出。
fn read_cache(file: &Path) -> Option<(i64, String)> {
    let text = fs::read_to_string(file).ok()?;
    let (time, output) = text.split_once('\n')?;
    Some((time.parse().ok()?, output.to_owned()))
}

fn write_cache(file: &Path, now: i64, output: &str) {
    let Some(dir) = file.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    // 多個 session 可能同時寫，先寫各自的暫存檔再改名
    let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
    if fs::write(&tmp, format!("{now}\n{output}")).is_ok() && fs::rename(&tmp, file).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

fn parse_cached(output: &str) -> Option<Git> {
    if output == NOT_A_REPO { None } else { parse_status(output) }
}

pub fn read(cwd: &Path, cache_dir: &Path) -> Option<Git> {
    read_with(cwd, cache_dir, timeutil::now(), TIMEOUT)
}

fn read_with(cwd: &Path, cache_dir: &Path, now: i64, timeout: Duration) -> Option<Git> {
    let file = cache_file(cache_dir, cwd);
    let cached = read_cache(&file);
    if let Some((time, output)) = &cached
        && (0..FRESH).contains(&(now - time))
    {
        return parse_cached(output);
    }
    match run_git(cwd, timeout) {
        Outcome::Done(output) => {
            write_cache(&file, now, &output);
            parse_cached(&output)
        }
        Outcome::TimedOut => {
            cached.filter(|(time, _)| now - time < STALE).and_then(|(_, output)| parse_cached(&output))
        }
    }
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

    fn init_repo(dir: &Path) -> bool {
        Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(dir).status().is_ok_and(|s| s.success())
    }

    #[test]
    fn caches_results_per_directory() {
        let repo = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        if !init_repo(repo.path()) {
            return; // 測試環境沒有 git
        }
        fs::write(repo.path().join("a.txt"), "a").unwrap();
        let now = 1_000_000;
        let git = read_with(repo.path(), cache.path(), now, TIMEOUT).expect("repo");
        assert_eq!((git.branch.as_str(), git.untracked), ("main", 1));

        // 快取期間內不重跑 git：新增的檔案還看不到
        fs::write(repo.path().join("b.txt"), "b").unwrap();
        assert_eq!(read_with(repo.path(), cache.path(), now + 1, TIMEOUT).unwrap().untracked, 1);
        // 過期後重新讀取
        assert_eq!(read_with(repo.path(), cache.path(), now + FRESH, TIMEOUT).unwrap().untracked, 2);
    }

    #[test]
    fn non_repo_is_cached_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        assert_eq!(read_with(dir.path(), cache.path(), 0, TIMEOUT), None);
        assert_eq!(read_cache(&cache_file(cache.path(), dir.path())).map(|(_, o)| o).as_deref(), Some(NOT_A_REPO));
    }

    #[test]
    fn timeout_falls_back_to_stale_cache() {
        let repo = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let file = cache_file(cache.path(), repo.path());
        write_cache(&file, 100, "# branch.oid abc\n# branch.head cached\n");
        // 逾時設為 0：一定逾時，改用 STALE 內的快取
        assert_eq!(read_with(repo.path(), cache.path(), 100 + 10, Duration::ZERO).unwrap().branch, "cached");
        // 快取太舊就不顯示
        assert_eq!(read_with(repo.path(), cache.path(), 100 + STALE, Duration::ZERO), None);
    }
}
