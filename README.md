# tokenbar

給 **Claude Code**、**Codex CLI**、**Antigravity CLI** 用的用量狀態列。context 使用率、5 小時額度、每週額度，一眼看完。

```text
Opus 5.5 │ high │ my-project │ main ↑2 ●3 ?1
ctx ███▊░░░░░░  38% │ 5h ██████▋░░░  66% ↻ 1h 46m │ 7d █████▊░░░░  58% ↻ 2d 12h
```

- **快**：Rust 原生執行檔，每次刷新約 3 ms（其中約 2 ms 是 `git status`），比用 shell 腳本解析 JSON 快十倍以上
- **一份程式碼、三個平台**：Windows、macOS、Linux（x64 與 ARM64），不需要 Node、Python 或 jq
- **一行安裝、一行移除**：安裝前自動備份設定檔，移除時還原成你原本的狀態列
- **一套顯示、三個 CLI**：同樣的雙行版面，自動適應各 CLI 提供的資料

## 目錄

- [支援狀況](#支援狀況)
- [安裝](#安裝)
- [使用](#使用)
- [各 CLI 說明](#各-cli-說明)
- [看懂狀態列](#看懂狀態列)
- [更新](#更新)
- [移除](#移除)
- [安裝時改了什麼](#安裝時改了什麼)
- [環境變數](#環境變數)
- [疑難排解](#疑難排解)
- [從原始碼建置](#從原始碼建置)
- [授權](#授權)

## 支援狀況

| CLI | 顯示方式 | ctx | 5 小時 | 每週 |
|---|---|:-:|:-:|:-:|
| Claude Code | 自訂狀態列 | ✓ | ✓ | ✓ |
| Antigravity CLI | 自訂狀態列 | ✓ | ✓ | ✓ |
| Codex CLI | 內建狀態列項目＋`tokenbar codex` 獨立視窗 | ✓ | ✓ | ✓ |

5 小時與每週額度只有訂閱方案才有。用 API Key 的話只會顯示 ctx。

| 平台 | 架構 |
|---|---|
| Linux | x64、ARM64（靜態連結，不挑發行版） |
| macOS | Intel、Apple Silicon |
| Windows | x64、ARM64 |

## 安裝

一行指令會下載對應平台的執行檔，並自動寫入偵測到的 CLI 設定。

**Linux / macOS**

```sh
curl -fsSL https://github.com/xubojun/tokenbar/releases/latest/download/tokenbar-installer.sh | sh && ~/.tokenbar/bin/tokenbar install
```

**Windows**（CMD 或 PowerShell 皆可）

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/xubojun/tokenbar/releases/latest/download/tokenbar-installer.ps1 | iex; & (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.tokenbar\bin\tokenbar.exe') install"
```

這行指令在 CMD 和 PowerShell 裡都能直接貼上執行，所以刻意不用 `$HOME` 這類變數，也不在內層用雙引號。

完成後**重新啟動** Claude Code、Codex CLI、Antigravity CLI 即可看到狀態列。

### 分兩步安裝

想先看安裝腳本再執行：

```sh
curl -fsSL https://github.com/xubojun/tokenbar/releases/latest/download/tokenbar-installer.sh -o tokenbar-installer.sh
less tokenbar-installer.sh
sh tokenbar-installer.sh
```

安裝腳本做三件事：

1. 把執行檔放到 `~/.tokenbar/bin`
2. 把該目錄加進 PATH
3. 寫一份安裝紀錄

它**不會**修改任何 CLI 設定。開新的終端機後，再執行：

```sh
tokenbar install
```

### 只裝在特定 CLI

```sh
tokenbar install claude            # 只裝 Claude Code
tokenbar install codex antigravity # 只裝 Codex CLI 與 Antigravity CLI
```

不指定時，會偵測已安裝的 CLI 並全部寫入。偵測方式是：設定目錄存在，或 PATH 上找得到 `claude`、`codex`、`agy`。

### 安裝特定版本

把網址中的 `latest/download` 換成 `download/<版本>`，例如：

```sh
curl -fsSL https://github.com/xubojun/tokenbar/releases/download/v0.1.0/tokenbar-installer.sh | sh
```

## 使用

裝好以後不用再做任何事，各 CLI 會自動呼叫 tokenbar。其他指令：

| 指令 | 說明 |
|---|---|
| `tokenbar install [agent...]` | 寫入狀態列設定 |
| `tokenbar uninstall [agent...]` | 還原狀態列設定 |
| `tokenbar uninstall --purge` | 還原全部設定並移除 tokenbar 本身 |
| `tokenbar codex` | 顯示最新 Codex session 的用量 |
| `tokenbar codex --watch` | 持續更新 |
| `tokenbar preview` | 用範例資料預覽外觀 |
| `tokenbar --version` | 顯示版本 |
| `tokenbar --help` | 顯示說明 |

`agent` 可以是 `claude`、`codex`、`antigravity`。

## 各 CLI 說明

### Claude Code

寫入 `~/.claude/settings.json` 的 `statusLine`。Claude Code 每次狀態變化都會把 session 資料以 JSON 傳給 tokenbar，tokenbar 畫出兩行狀態列。

### Antigravity CLI

寫入 `~/.gemini/antigravity-cli/settings.json` 的 `statusLine`。

Antigravity 的額度分成好幾個桶，例如 `gemini-5h`、`gemini-weekly`、特定模型的 `gemini-*-weekly`，以及第三方模型的 `3p-5h`、`3p-weekly`。tokenbar 的處理方式：

- 只顯示**目前所用模型那一家**的額度（Gemini 模型看 `gemini-*`，其他模型看 `3p-*`）
- 同一個時間窗有多個桶時，顯示**用量最高**的那一個，因為它會最先把你卡住

### Codex CLI

Codex 的狀態列**不能執行外部程式**，只能從內建項目挑選。所以 tokenbar 分兩部分支援：

**1. 內建狀態列**

`tokenbar install codex` 會在 `~/.codex/config.toml` 的 `[tui] status_line` 加入以下三個項目，原有的項目、註解、排版都會保留：

- `context-used`
- `five-hour-limit`
- `weekly-limit`

**2. 用量條視窗**

另開一個終端機或 tmux 窗格，執行：

```sh
tokenbar codex --watch
```

它會讀取 `~/.codex/sessions` 裡最新的 session 紀錄，每 2 秒更新一次。其他選項：

```sh
tokenbar codex --watch --interval 5       # 每 5 秒更新
tokenbar codex --cwd ~/projects/my-app    # 只看在這個目錄啟動的 session
```

放進 tmux 狀態列：

```tmux
set -g status-right '#(tokenbar codex --no-color | tail -1)'
set -g status-interval 5
```

## 看懂狀態列

```text
Opus 5.5 │ high │ my-project │ main ↑2 ↓1 ●3 ?1
ctx ███▊░░░░░░  38% │ 5h ██████▋░░░  66% ↻ 1h 46m │ 7d █████▊░░░░  58% ↻ 2d 12h
```

**第一行**

| 項目 | 意義 |
|---|---|
| `Opus 5.5` | 目前模型 |
| `high` | 推理強度 |
| `my-project` | 目前目錄 |
| `main` | git 分支 |
| `↑2` / `↓1` | 領先／落後上游 2 個／1 個 commit |
| `●3` | 3 個已修改的檔案 |
| `?1` | 1 個未追蹤的檔案 |

**第二行**

| 項目 | 意義 |
|---|---|
| `ctx` | context window 使用率 |
| `5h` | 5 小時額度使用率 |
| `7d` | 每週額度使用率 |
| `↻ 1h 46m` | 距離額度重置的時間 |

**顏色**：綠色低於 60%，黃色 60%–85%，紅色 85% 以上。

終端機太窄時，會先拿掉重置時間，再把用量條縮短。

## 更新

重新執行[安裝](#安裝)的那一行即可。新的執行檔會覆蓋舊的，各 CLI 的設定指向同一個位置，不需要重新設定。

## 移除

**一行全部移除**：還原所有 CLI 的狀態列設定，並刪除 tokenbar 本身。

```sh
tokenbar uninstall --purge
```

找不到 `tokenbar` 指令時，用完整路徑：

```sh
~/.tokenbar/bin/tokenbar uninstall --purge          # Linux / macOS
```

```powershell
& "$HOME\.tokenbar\bin\tokenbar.exe" uninstall --purge   # Windows
```

`--purge` 會做這些事：

1. 還原 Claude Code、Codex CLI、Antigravity CLI 的狀態列設定
   - 安裝前用的是別的狀態列工具，就還原成它
   - 安裝前沒有狀態列，就把設定移除
2. 從 PATH 移除 `~/.tokenbar/bin`
   - Linux / macOS：刪掉 `.profile`、`.bashrc`、`.zshrc` 等檔案裡安裝腳本加的那一行，以及 fish 的設定檔
   - Windows：從使用者 PATH 移除
3. 刪除安裝紀錄、`~/.tokenbar` 目錄與執行檔

**只移除某個 CLI 的設定**，保留 tokenbar：

```sh
tokenbar uninstall codex
```

**手動移除**（tokenbar 已經無法執行時）：

1. 刪掉 `~/.claude/settings.json` 和 `~/.gemini/antigravity-cli/settings.json` 中的 `statusLine`。也可以直接用同目錄下的 `settings.json.tokenbar-bak` 覆蓋回去，但要注意：這份備份是**最後一次執行 `tokenbar install` 時**的內容，之後對設定檔的其他修改會一併被蓋掉。
2. 刪掉 `~/.codex/config.toml` 中 `[tui] status_line` 加入的三個項目
3. 刪除 `~/.tokenbar` 目錄
4. 刪除 `~/.config/tokenbar` 目錄，以及 `.profile`、`.bashrc`、`.zshrc` 中的 `. "$HOME/.config/tokenbar/env.sh"` 這一行（fish 則刪除 `~/.config/fish/conf.d/tokenbar.env.fish`；Windows 從使用者 PATH 移除 `%USERPROFILE%\.tokenbar\bin`）

## 安裝時改了什麼

tokenbar 只動狀態列這一個設定，其他內容與順序都保持原樣。

| 檔案 | 修改內容 | 備份 |
|---|---|---|
| `~/.claude/settings.json` | `statusLine` | `settings.json.tokenbar-bak` |
| `~/.gemini/antigravity-cli/settings.json` | `statusLine` | `settings.json.tokenbar-bak` |
| `~/.codex/config.toml` | `[tui] status_line` | `config.toml.tokenbar-bak` |
| `~/.tokenbar/state.json` | 記錄安裝前的狀態列，供移除時還原 | — |

安全措施：

- 設定檔格式有誤時**不會覆寫**，會直接回報錯誤
- 寫入時先寫暫存檔再改名，中斷也不會留下寫到一半的檔案
- 重複執行 `tokenbar install` 不會覆蓋第一次記下的原始設定

## 環境變數

| 變數 | 作用 |
|---|---|
| `NO_COLOR` | 設為任意值即停用顏色 |
| `CLAUDE_CONFIG_DIR` | Claude Code 設定目錄（預設 `~/.claude`） |
| `CODEX_HOME` | Codex 資料目錄（預設 `~/.codex`） |
| `TOKENBAR_HOME` | tokenbar 狀態目錄（預設 `~/.tokenbar`） |

## 疑難排解

**狀態列沒出現**

- 確認已重新啟動該 CLI
- 直接執行 `tokenbar preview`，確認執行檔正常
- Claude Code 可以用 `claude --debug` 查看狀態列指令的錯誤訊息

**只看到 ctx，沒有 5h／7d**

額度資料只有訂閱方案才有，用 API Key 時不會提供。Claude Code 要在第一次回應之後才會帶入額度。

**Codex 視窗顯示的模型或用量不是我正在用的那個 session**

`tokenbar codex` 預設看最新的 session。同時開多個時，用 `--cwd` 指定目錄。

**Windows 上狀態列顯示亂碼**

請使用 Windows Terminal。舊版主控台對 Unicode 方塊字元的支援不完整。

## 從原始碼建置

需要 Rust 1.88 以上。

```sh
git clone https://github.com/xubojun/tokenbar.git
cd tokenbar
cargo build --release
./target/release/tokenbar install
```

開發時常用的指令：

```sh
cargo test                                  # 單元測試
cargo clippy --all-targets -- -D warnings   # 靜態檢查
cargo fmt                                   # 格式化
```

發布流程由 [dist](https://github.com/axodotdev/cargo-dist) 產生：推送 `v*` 標籤後，GitHub Actions 會編譯全部平台並建立 Release 與安裝腳本。

## 授權

[MIT](LICENSE)
