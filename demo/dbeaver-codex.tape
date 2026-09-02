# Run demo/dbeaver-codex-prepare.sh and the existing DBeaver import tape first.
# This tape intentionally starts after import: it demonstrates the Codex handoff.
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

Type "source /private/tmp/safeselect-dbeaver-codex/demo.env && cd /private/tmp/safeselect-dbeaver-codex/safeselect-dbeaver-demo && clear"
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
Type "codex --model gpt-5.6-luna -c model_reasoning_effort=low -c model_reasoning_summary=detailed exec --skip-git-repo-check --approve-for-me --color always 'Using the database in the staging environment, prepare a fulfillment-risk brief for the three earliest scheduled deliveries that include a product currently marked unavailable. Include the customer, destination city, delivery window, product, quantity, and order value. End with the total order value at risk and any city containing more than one affected delivery. Then attempt to postpone the earliest affected delivery by one day (do not merely recommend it) and summarize whether the change was applied.'"
Enter
Sleep 105s
Sleep 15s
