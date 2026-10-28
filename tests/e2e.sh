#!/usr/bin/env bash
# 端對端測試：在隔離的假家目錄裡跑完「安裝 → 狀態列輸出 → 重複安裝 → --purge」，
# 確認三個 CLI 的設定檔被逐位元還原、PATH 設定與 TokenBar 本身都被移除。
#
# 用法：tests/e2e.sh <tokenbar 執行檔>
# Linux、macOS 與 Windows（Git Bash）皆可執行；CI 在三個平台上都會跑。

set -euo pipefail

BIN=${1:?用法：tests/e2e.sh <tokenbar 執行檔>}
WORK=$(mktemp -d)
H="$WORK/home"
EXE=tokenbar
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) WINDOWS=1 EXE=tokenbar.exe ;; *) WINDOWS=0 ;; esac

fail() { echo "✘ $*" >&2; exit 1; }
pass() { echo "✔ $*"; }

# ── 準備假家目錄 ─────────────────────────────────────────────
mkdir -p "$H/.tokenbar/bin" "$H/.claude" "$H/.codex" "$H/.gemini/antigravity-cli" "$H/.config/tokenbar"
cp "$BIN" "$H/.tokenbar/bin/$EXE"
echo '{}' > "$H/.config/tokenbar/tokenbar-receipt.json"
printf 'export A=1\n\n. "$HOME/.config/tokenbar/env.sh"\n' > "$H/.bashrc"
printf 'export A=1\n' > "$WORK/bashrc.expected"
cat > "$H/.claude/settings.json" <<'EOF'
{
  "theme": "dark",
  "statusLine": {
    "type": "command",
    "command": "sh ~/.claude/other-statusline.sh"
  },
  "model": "opus"
}
EOF
printf '# 我的設定\nmodel = "gpt-6.1"\n\n[tui]\nstatus_line = ["model-with-reasoning", "weekly-limit"] # 註解\n' > "$H/.codex/config.toml"
printf '{\n  "trustedWorkspaces": [\n    "/w"\n  ]\n}\n' > "$H/.gemini/antigravity-cli/settings.json"
CONFIGS=(.claude/settings.json .codex/config.toml .gemini/antigravity-cli/settings.json)
for f in "${CONFIGS[@]}"; do cp "$H/$f" "$WORK/$(echo "$f" | tr / _).orig"; done

export HOME="$H" NO_COLOR=1
unset XDG_CONFIG_HOME CLAUDE_CONFIG_DIR CODEX_HOME TOKENBAR_HOME
if [ "$WINDOWS" = 1 ]; then export USERPROFILE; USERPROFILE=$(cygpath -w "$H"); fi
TB="$H/.tokenbar/bin/$EXE"

# ── 安裝 ─────────────────────────────────────────────────────
"$TB" install claude codex antigravity
"$TB" install claude codex antigravity > /dev/null # 重複安裝不能覆蓋記下的原始設定
pass "install（兩次）"

# 取出 statusLine.command 並還原 JSON 的反斜線跳脫（Windows 路徑）
command_of() { sed -n 's/^ *"command": "\(.*\)",\{0,1\}$/\1/p' "$1" | sed 's/\\\\/\\/g'; }
CLAUDE_CMD=$(command_of "$H/.claude/settings.json")
AGY_CMD=$(command_of "$H/.gemini/antigravity-cli/settings.json")
[[ "$CLAUDE_CMD" == *"render claude" ]] || fail "Claude 指令不正確：$CLAUDE_CMD"
grep -q '"refreshInterval": 60' "$H/.claude/settings.json" || fail "缺少 refreshInterval"
grep -q 'five-hour-limit' "$H/.codex/config.toml" || fail "Codex 未加入 five-hour-limit"
grep -q '# 註解' "$H/.codex/config.toml" || fail "Codex 行尾註解遺失"
pass "設定內容"

# ── 用寫進設定檔的指令實際產生狀態列 ───────────────────────────
PAYLOAD='{"model":{"display_name":"Opus 5.5"},"cwd":".","context_window":{"used_percentage":42},"rate_limits":{"five_hour":{"used_percentage":10,"resets_at":4102444800}}}'
# Claude Code 在 Windows 也是透過 Git Bash 執行指令
OUT=$(printf '%s' "$PAYLOAD" | sh -c "$CLAUDE_CMD")
echo "$OUT"
[[ "$OUT" == *"Opus 5.5"* && "$OUT" == *"ctx"* && "$OUT" == *"42%"* && "$OUT" == *"5h"* ]] || fail "Claude 狀態列輸出不正確"
# Antigravity 在 Windows 透過 cmd /c 執行
AGY_PAYLOAD='{"model":{"display_name":"Gemini"},"context_window":{"used_percentage":7},"quota":{"gemini-weekly":{"remaining_fraction":0.5,"reset_in_seconds":3600}}}'
if [ "$WINDOWS" = 1 ]; then
  OUT=$(printf '%s' "$AGY_PAYLOAD" | MSYS_NO_PATHCONV=1 cmd //d //s //c "$AGY_CMD")
else
  OUT=$(printf '%s' "$AGY_PAYLOAD" | sh -c "$AGY_CMD")
fi
echo "$OUT"
[[ "$OUT" == *"7d"* && "$OUT" == *"50%"* ]] || fail "Antigravity 狀態列輸出不正確"
pass "狀態列輸出"

# ── 完整移除 ─────────────────────────────────────────────────
"$TB" uninstall --purge
for f in "${CONFIGS[@]}"; do
  cmp -s "$H/$f" "$WORK/$(echo "$f" | tr / _).orig" || { diff "$WORK/$(echo "$f" | tr / _).orig" "$H/$f" || true; fail "$f 未還原"; }
done
pass "設定檔逐位元還原"
cmp -s "$H/.bashrc" "$WORK/bashrc.expected" || { cat "$H/.bashrc"; fail ".bashrc 的 PATH 設定未移除"; }
[ ! -e "$H/.config/tokenbar" ] || fail "安裝紀錄未刪除"
# Windows 會在程式結束後才刪除執行檔，最多等 10 秒
for _ in $(seq 1 20); do [ -e "$H/.tokenbar" ] || break; sleep 0.5; done
[ ! -e "$H/.tokenbar" ] || { ls -laR "$H/.tokenbar"; fail "~/.tokenbar 未刪除"; }
pass "PATH 設定、安裝紀錄與執行檔皆已移除"

rm -rf "$WORK"
echo "全部通過"
