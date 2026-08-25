# Preflight: ./demo/setup.sh and SafeSelect OpenCode registration for MongoDB.
Output demo/recordings/safeselect-mongodb.gif

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
Type "opencode --pure run --dir demo 'Which security products are currently available? Give me their names, SKUs, and prices.'"
Enter
Sleep 60s
