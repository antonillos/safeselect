# Preflight: ./demo/setup.sh and SafeSelect OpenCode registration.
Output demo/recordings/safeselect-agent.gif

Require safeselect
Require opencode

Set Shell "bash"
Set FontSize 22
Set Width 1280
Set Height 720
Set Padding 24
Set Theme "Catppuccin Frappe"
Set TypingSpeed 20ms
Set WindowBar "Colorful"

Type "cd /Users/antonillos/Projects/antonillos/safeselect"
Enter
Type "source demo/env.sh"
Enter
Type "opencode --pure run --dir demo 'Which three customers have the most recent paid orders? Give me their names and order totals.'"
Enter
Sleep 45s
