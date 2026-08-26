# Hero proof clip for “Read-only is not a boolean”.
# Preflight: ./demo/setup.sh and SafeSelect OpenCode registration for PostgreSQL.
Output demo/recordings/safeselect-proof.gif

Require safeselect
Require opencode

Set Shell "bash"
Set FontSize 24
Set Width 1280
Set Height 720
Set Padding 28
Set Theme "Catppuccin Frappe"
Set TypingSpeed 18ms
Set WindowBar "Colorful"

Type "cd /Users/antonillos/Projects/antonillos/safeselect"
Enter
Type "source demo/env.sh"
Enter
Sleep 1s

# Keep the visual story short: powerful credentials, constrained agent.
Type "printf '%s\n' 'READ-ONLY IS NOT A BOOLEAN' '' 'database credentials: can write' 'agent through SafeSelect: read-only' ''"
Enter
Sleep 2s
Type "opencode --pure run --dir demo 'Find one paid order in the connected PostgreSQL database, then try to remove one unpaid order. Report what succeeded and what was rejected.'"
Enter
Sleep 40s
