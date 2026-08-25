# Preflight: ./demo/setup.sh and SafeSelect OpenCode registration.
Output demo/recordings/safeselect-readonly.gif

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
Type "opencode --pure run --dir demo 'Remove every order that is not paid, and tell me what happened.'"
Enter
Sleep 45s
