# Preflight: ./demo/onboarding-prepare.sh; released Homebrew binary.
# The local tap uses this checkout's shortened caveats pending publication.
Output "demo/recordings/onboarding-full-local.gif"
Output "demo/recordings/onboarding-full-local.mp4"
Output "demo/.runtime/onboarding-full-local-screen.txt"
Require brew
Require opencode
Set Shell "bash"
Set FontSize 19
Set Width 1600
Set Height 1000
Set Padding 24
Set Theme "Dracula"
Set TypingSpeed 15ms
Set WindowBar "Colorful"
Set Framerate 15
Hide
Type "cd /private/tmp/safeselect-onboarding-full-local && source recording.env; clear"
Enter
Sleep 1s
Show
Type "brew tap antonillos/tap"
Enter
Sleep 3s
Type "brew trust --formula antonillos/tap/safeselect"
Enter
Sleep 2s
Type "brew install antonillos/tap/safeselect"
Enter
Wait@60s /demo \$\s*$/
Sleep 3s
Screenshot "demo/.runtime/onboarding-install.png"
Type "safeselect --version"
Enter
Sleep 2s
Type "safeselect import-dbeaver dbeaver-demo.dbp"
Enter
Sleep 2s
Space
Enter
Sleep 1s
Type "staging"
Enter
Sleep 1s
# Configure the fixture's SSH bastion.
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Type "postgres"
Enter
Sleep 1s
Type "5432"
Enter
Sleep 1s
# Key-file authentication.
Enter
Sleep 1s
Type "/private/tmp/safeselect-onboarding-full-local/ssh/demo_ed25519"
Enter
Sleep 1s
# Enter a hidden database password and store it in macOS Keychain.
Enter
Sleep 1s
Enter
Sleep 1s
Type "demo-password"
Enter
Sleep 1s
# Decline the immediate check so preparation can pin the fixture host key.
Enter
Sleep 1s
Hide
Type "python3 pin-bastion.py; clear"
Enter
Sleep 1s
Show
Type "safeselect driver download --vendor postgresql"
Enter
Wait@90s /demo \$\s*$/
Sleep 2s
Type "safeselect check --environment staging"
Enter
Wait@60s /demo \$\s*$/
Sleep 4s
Screenshot "demo/.runtime/onboarding-check.png"
Type "safeselect agent install opencode --environment staging --local"
Enter
Sleep 3s
Type "opencode reload"
Enter
Wait@30s /demo \$\s*$/
Sleep 3s
Type "opencode mcp list"
Enter
Wait@30s /demo \$\s*$/
Sleep 4s
Type "opencode run --thinking --model openai/gpt-6-luna#medium 'In the onboarding database, show me one paid order. Then try deleting one unpaid order and tell me what happened.'"
Enter
Wait@180s /demo \$\s*$/
Sleep 8s
Screenshot "demo/.runtime/onboarding-agent.png"
