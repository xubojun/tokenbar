mod adapters;
mod git;
mod install;
mod paths;
mod purge;
mod render;
mod status;
mod timeutil;
mod update;

use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use install::{Agent, Removal};
use paths::{Paths, Platform};
use status::Status;

const HELP: &str = "\
TokenBar — Claude Code / Codex CLI / Antigravity CLI 的用量狀態列

用法：
  tokenbar install [agent...]      寫入狀態列設定（預設：偵測到的全部）
  tokenbar uninstall [agent...]    還原狀態列設定（預設：全部）
      --purge                      一併移除 TokenBar 本身
  tokenbar codex [選項]            顯示最新 Codex session 的用量
      --watch                      持續更新
      --interval <秒>              更新間隔，預設 2
      --cwd <目錄>                 只看在此目錄啟動的 session
  tokenbar update                  更新到最新版本
      --check                      只檢查是否有新版本
  tokenbar render <claude|antigravity>
                                   從 stdin 讀取 JSON 並輸出狀態列（由各 CLI 呼叫）
  tokenbar preview                 用範例資料預覽
  tokenbar --version

agent：claude、codex、antigravity

環境變數：NO_COLOR 停用顏色；CLAUDE_CONFIG_DIR、CODEX_HOME、TOKENBAR_HOME 覆寫路徑。
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (cmd, rest) = args.split_first().map_or(("", &[][..]), |(c, r)| (c.as_str(), r));
    let result = match cmd {
        "render" => return render_cmd(rest),
        "codex" => codex_cmd(rest),
        "install" => install_cmd(rest),
        "uninstall" => uninstall_cmd(rest),
        "preview" => preview_cmd(),
        "update" => match rest {
            [] => update::update(false),
            [flag] if flag == "--check" => update::update(true),
            _ => Err("update 只支援 --check".into()),
        },
        "-V" | "--version" | "version" => {
            println!("tokenbar {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "" | "-h" | "--help" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("未知的指令：{other}\n\n{HELP}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("tokenbar：{msg}");
            ExitCode::FAILURE
        }
    }
}

fn color_enabled(forced_off: bool) -> bool {
    !forced_off && env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

fn render_status(status: &Status, color: bool) -> String {
    let cache = Paths::from_env().state_dir.join("cache");
    let git = status.cwd.as_deref().and_then(|cwd| git::read(Path::new(cwd), &cache));
    render::render(status, git.as_ref(), color, timeutil::now())
}

/// 由各 CLI 頻繁呼叫：不論輸入為何都輸出內容並以 0 結束，
/// 避免 Antigravity 連續失敗 30 次後停用狀態列。
fn render_cmd(args: &[String]) -> ExitCode {
    let agent = args.first().map(String::as_str).unwrap_or("claude");
    let mut input = String::new();
    let _ = io::stdin().read_to_string(&mut input);
    let payload = serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
    let status = match agent {
        "antigravity" => adapters::antigravity::from_payload(&payload, timeutil::now()),
        "claude" => adapters::claude::from_payload(&payload),
        other => {
            println!("tokenbar：不支援的 agent「{other}」");
            return ExitCode::SUCCESS;
        }
    };
    println!("{}", render_status(&status, color_enabled(false)));
    ExitCode::SUCCESS
}

fn take_value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<&'a String, String> {
    it.next().ok_or_else(|| format!("{flag} 需要一個值"))
}

fn codex_cmd(args: &[String]) -> Result<(), String> {
    let (mut watch, mut interval, mut cwd, mut no_color) = (false, 2.0_f64, None::<PathBuf>, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--watch" | "-w" => watch = true,
            "--interval" => {
                interval = take_value(&mut it, arg)?.parse().map_err(|_| "--interval 必須是數字".to_string())?;
            }
            "--cwd" => cwd = Some(PathBuf::from(take_value(&mut it, arg)?)),
            "--no-color" => no_color = true,
            other => return Err(format!("codex 不支援的選項：{other}")),
        }
    }
    let home = Paths::from_env().codex_home;
    let color = color_enabled(no_color) && io::stdout().is_terminal();
    let mut drawn_lines = 0;

    loop {
        let found = adapters::codex::find_latest(&home, cwd.as_deref());
        let text = match &found {
            Some(session) => {
                let mut status = adapters::codex::from_session(session, timeutil::now());
                status.width = terminal_width();
                render_status(&status, color)
            }
            None if cwd.is_some() => "找不到在這個目錄啟動的 Codex session".to_string(),
            None => format!("找不到 Codex session（{}）", home.join("sessions").display()),
        };
        if !watch {
            println!("{text}");
            return if found.is_some() { Ok(()) } else { Err("沒有可顯示的資料".into()) };
        }

        // 只重畫自己上次輸出的那幾行，不清除整個畫面
        let mut out = io::stdout().lock();
        if drawn_lines > 0 {
            let _ = write!(out, "\x1b[{drawn_lines}F\x1b[J");
        }
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
        drawn_lines = text.lines().count();
        thread::sleep(Duration::from_secs_f64(interval.max(0.5)));
    }
}

/// 終端機寬度：優先用 COLUMNS，其次查詢實際的終端機。
fn terminal_width() -> Option<usize> {
    env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .or_else(|| terminal_size::terminal_size().map(|(w, _)| usize::from(w.0)))
}

/// 在 PATH 上找得到這個指令嗎（Windows 會試 PATHEXT 的副檔名）。
fn on_path(name: &str) -> bool {
    let Some(path) = env::var_os("PATH") else { return false };
    let exts: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into()).split(';').map(str::to_owned).collect()
    } else {
        vec![String::new()]
    };
    env::split_paths(&path).any(|dir| exts.iter().any(|ext| dir.join(format!("{name}{ext}")).is_file()))
}

fn detected(agent: Agent, p: &Paths) -> bool {
    let (dir, bin) = match agent {
        Agent::Claude => (p.claude_settings.parent().map(Path::to_path_buf), "claude"),
        Agent::Codex => (Some(p.codex_home.clone()), "codex"),
        Agent::Antigravity => (p.antigravity_settings.parent().map(Path::to_path_buf), "agy"),
    };
    dir.is_some_and(|d| d.is_dir()) || on_path(bin)
}

fn parse_agents(args: &[String]) -> Result<Vec<Agent>, String> {
    args.iter()
        .map(|a| Agent::parse(a).ok_or_else(|| format!("未知的 agent：{a}（可用：claude、codex、antigravity）")))
        .collect()
}

fn install_cmd(args: &[String]) -> Result<(), String> {
    let p = Paths::from_env();
    let mut agents = parse_agents(args)?;
    if agents.is_empty() {
        agents = Agent::ALL.into_iter().filter(|a| detected(*a, &p)).collect();
        if agents.is_empty() {
            return Err(
                "沒有偵測到 Claude Code、Codex CLI 或 Antigravity CLI；可用 tokenbar install <agent> 指定。".into()
            );
        }
    }
    let exe = env::current_exe().map_err(|e| format!("找不到 tokenbar 執行檔位置：{e}"))?;
    update::remove_leftovers();

    let mut failed = false;
    for agent in agents {
        let result = match agent {
            Agent::Codex => install::codex::install(&p.codex_home, &p.state_dir),
            Agent::Claude | Agent::Antigravity => {
                let file = if agent == Agent::Claude { &p.claude_settings } else { &p.antigravity_settings };
                let command = paths::status_command(&exe, agent.key(), Platform::current());
                install::json_settings::install(agent, file, &command, &p.state_dir)
            }
        };
        match result {
            Ok(done) => {
                println!("✔ {agent}：已寫入 {}", done.file.display());
                if let Some(backup) = done.backup {
                    println!("  備份：{}", backup.display());
                }
                if done.replaced_other {
                    println!("  已取代原本的狀態列，tokenbar uninstall 時會還原。");
                }
                if agent == Agent::Codex {
                    println!(
                        "  Codex 無法執行外部指令，已改用內建項目 context-used / five-hour-limit / weekly-limit。"
                    );
                    println!("  想看用量條：另開視窗執行 tokenbar codex --watch");
                }
            }
            Err(err) => {
                failed = true;
                println!("✘ {agent}：{err}");
            }
        }
    }
    println!("\n重新啟動各 CLI 後生效。");
    if failed { Err("部分 agent 安裝失敗".into()) } else { Ok(()) }
}

fn uninstall_cmd(args: &[String]) -> Result<(), String> {
    let purge = args.iter().any(|a| a == "--purge");
    let rest: Vec<String> = args.iter().filter(|a| *a != "--purge").cloned().collect();
    let mut agents = parse_agents(&rest)?;
    if agents.is_empty() || purge {
        agents = Agent::ALL.to_vec();
    }
    let p = Paths::from_env();

    let mut failed = false;
    for agent in agents {
        let result = match agent {
            Agent::Codex => install::codex::uninstall(&p.codex_home, &p.state_dir),
            Agent::Claude => install::json_settings::uninstall(agent, &p.claude_settings, &p.state_dir),
            Agent::Antigravity => install::json_settings::uninstall(agent, &p.antigravity_settings, &p.state_dir),
        };
        match result {
            Ok(Removal::Restored) => println!("✔ {agent}：已還原原本的狀態列設定"),
            Ok(Removal::Removed) => println!("✔ {agent}：已移除狀態列設定"),
            Ok(Removal::NotInstalled) => println!("- {agent}：未安裝，略過"),
            Err(err) => {
                failed = true;
                println!("✘ {agent}：{err}");
            }
        }
    }
    if failed {
        return Err("部分 agent 還原失敗，未移除 TokenBar 本身".into());
    }
    if purge {
        purge::purge(&p).map_err(|e| format!("移除 TokenBar 本身失敗：{e}"))?;
    }
    Ok(())
}

fn preview_cmd() -> Result<(), String> {
    let now = timeutil::now();
    let samples = [
        (
            "Claude Code",
            adapters::claude::from_payload(&serde_json::json!({
                "model": { "display_name": "Opus 5.5" },
                "effort": { "level": "high" },
                "workspace": { "current_dir": "my-project" },
                "context_window": { "used_percentage": 38 },
                "rate_limits": {
                    "five_hour": { "used_percentage": 66, "resets_at": now + 6360 },
                    "seven_day": { "used_percentage": 58, "resets_at": now + 217800 }
                }
            })),
        ),
        (
            "Antigravity CLI",
            adapters::antigravity::from_payload(
                &serde_json::json!({
                    "model": { "display_name": "Gemini 3 Pro", "effort": "high" },
                    "cwd": "my-project",
                    "context_window": { "used_percentage": 21 },
                    "quota": { "gemini-weekly": { "remaining_fraction": 0.62, "reset_in_seconds": 302400 } }
                }),
                now,
            ),
        ),
    ];
    let color = color_enabled(false) && io::stdout().is_terminal();
    for (name, status) in samples {
        println!("── {name}");
        println!("{}\n", render::render(&status, None, color, now));
    }
    Ok(())
}
