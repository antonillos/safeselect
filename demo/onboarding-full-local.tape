Output "/Users/antonillos/Projects/antonillos/safeselect/docs/recordings/onboarding-full-local.gif"
Require opencode
Set Shell "bash"
Set FontSize 19
Set Width 1280
Set Height 720
Set Padding 24
Set Theme "GitHub Dark"
Set TypingSpeed 9ms
Set WindowBar "Colorful"
Hide
Type "unset NO_COLOR; export TERM=xterm-256color COLORTERM=truecolor; cd /private/tmp/safeselect-onboarding-full-local && export XDG_CONFIG_HOME=/private/tmp/safeselect-onboarding-full-local/.homebrew; export PATH=/Users/antonillos/.homebrew/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin; clear"
Enter
Show
Sleep 1s
Type "brew tap antonillos/tap"
Enter
Sleep 5s
Type "brew trust --formula antonillos/tap/safeselect"
Enter
Sleep 2s
Type "brew install antonillos/tap/safeselect"
Enter
Sleep 30s
Type "safeselect --version"
Enter
Sleep 2s
Type "safeselect import-dbeaver /Users/antonillos/Projects/antonillos/safeselect-e2e/General-20260827.dbp"
Enter
Sleep 2s
Type " "
Enter
Sleep 2s
Type "staging"
Enter
Sleep 2s
Enter
Sleep 2s
Enter
Sleep 2s
Enter
Sleep 2s
Enter
Sleep 2s
Type "postgres"
Enter
Sleep 2s
Type "5432"
Enter
Sleep 2s
Enter
Sleep 2s
Type "/Users/antonillos/Projects/antonillos/safeselect/demo/.runtime/ssh/demo_ed25519"
Enter
Sleep 2s
Type "demo"
Enter
Sleep 2s
Type "demo-password"
Enter
Sleep 5s
Type "safeselect agent install opencode --environment staging --local"
Enter
Sleep 5s
Type "safeselect agent status"
Enter
Sleep 5s
Type "opencode run --thinking --model openai/gpt-5.4 --variant low 'Find one paid order in the connected PostgreSQL database, then try to remove one unpaid order. Report what succeeded and what was rejected.'"
Enter
Sleep 90s
