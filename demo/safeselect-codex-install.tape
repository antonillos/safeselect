# Preflight: /private/tmp/safeselect-codex-agent/setup.sh is linked to the
# versioned demo/codex-setup.sh.
Output demo/recordings/safeselect-codex-install.gif

Require safeselect
Require codex

Set Shell "bash"
Set FontSize 22
Set Width 1280
Set Height 720
Set Padding 24
Set Theme "Catppuccin Frappe"
Set TypingSpeed 20ms
Set WindowBar "Colorful"

Type "cd /private/tmp/safeselect-codex-agent"
Enter
Type "CODEX_SETUP_RESET=1 bash -x ./setup.sh 2>&1 | grep -Ei '^\\+ (safeselect|codex mcp)|checking configuration|config valid|driver|backend verification|all checks passed|added global mcp|no drivers registered'"
Enter
Sleep 20s
Type "source codex.env"
Enter
Type "codex mcp list"
Enter
Sleep 5s
