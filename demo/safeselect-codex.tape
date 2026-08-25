# Preflight: /private/tmp/safeselect-codex-agent/setup.sh.
Output demo/recordings/safeselect-codex.gif

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
Type "source codex.env"
Enter
Type "codex exec --ephemeral --cd /private/tmp/safeselect-codex-agent --approve-for-me --skip-git-repo-check 'Which three customers have the most recent paid orders? Give me their names and order totals.'"
Enter
Sleep 60s
