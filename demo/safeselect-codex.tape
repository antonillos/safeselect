# Preflight: /private/tmp/safeselect-codex-agent/setup.sh.
Output demo/recordings/safeselect-codex.gif

Require safeselect
Require codex

Set Shell "bash"
Set FontSize 20
Set Width 1280
Set Height 900
Set Padding 24
Set Theme "Catppuccin Frappe"
Set TypingSpeed 20ms
Set WindowBar "Colorful"

Type "cd /private/tmp/safeselect-codex-agent/workspace"
Enter
Type "source ../codex.env"
Enter
Type "../run-codex.sh"
Enter
Sleep 75s
