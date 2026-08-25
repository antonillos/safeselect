# Preflight once before recording: ./demo/setup.sh
Output demo/recordings/safeselect-demo.gif

Require safeselect
Require python3

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
Type "echo 'SafeSelect MCP — agents can look, they cannot mutate.'"
Enter
Sleep 2s

Type "./demo/show-postgres.sh"
Enter
Sleep 3s

Type "./demo/show-mongodb.sh"
Enter
Sleep 3s

Type "./demo/show-rejection.sh"
Enter
Sleep 3s

Type "echo 'SafeSelect MCP: local, read-only, fail-closed.'"
Enter
Sleep 2s
