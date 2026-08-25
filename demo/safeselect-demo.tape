# Preflight once before recording:
# ./demo/setup.sh
# safeselect agent install opencode --project "$PWD/demo" --environment postgres --local
Output demo/recordings/safeselect-demo.gif

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
Sleep 30s
