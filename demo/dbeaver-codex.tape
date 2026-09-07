# Run demo/dbeaver-codex-prepare.sh and the interactive DBeaver import first.
# This tape intentionally starts after import: it demonstrates the Codex handoff.
# Invoke VHS with SAFESELECT_DBEAVER_ROOT set to the runtime prepared for this run.
Output "docs/recordings/safeselect-dbeaver-codex.gif"

Require codex
Set Shell "bash"
Set FontSize 19
Set Width 1280
Set Height 900
Set Padding 24
Set Theme "Dracula"
Set TypingSpeed 18ms
Set WindowBar "Colorful"

Type `source "$SAFESELECT_DBEAVER_ROOT/demo.env" && cd "$SAFESELECT_DBEAVER_PROJECT" && clear`
Enter
Sleep 1s
Type "cp $SAFESELECT_DBEAVER_SOURCE/dbeaver-codex-{intro,isolate,format.py,run.sh} $SAFESELECT_DBEAVER_ROOT/ 2>/dev/null || true"
Enter
Sleep 1s
Type "../dbeaver-codex-intro.sh"
Enter
Sleep 5s
Type "../dbeaver-codex-isolate.sh"
Enter
Sleep 2s
Type "$SAFESELECT_BIN check --environment staging"
Enter
Sleep 8s
Type "$SAFESELECT_BIN agent install codex --environment staging --local"
Enter
Sleep 3s
Type "echo '=== Codex MCP allowlist (project) ==='; grep mcp_servers .codex/config.toml"
Enter
Sleep 3s
Type "SAFESELECT_CODEX_JSON=1 ../dbeaver-codex-run.sh 2>&1 | ../dbeaver-codex-format.py"
Enter
Sleep 105s
Sleep 15s
