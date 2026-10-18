# Current macOS source build; synthetic offline fixture, no Keychain writes.
Output "demo/recordings/dbeaver-import-ux.gif"
Output "demo/recordings/dbeaver-import-ux.mp4"
Output "demo/.runtime/dbeaver-import-ux-screen.txt"
Set Shell "bash"
Set FontSize 19
Set Width 1600
Set Height 1100
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 15ms
Set WindowBar "Colorful"
Set Framerate 15
Hide
Type "cd /tmp/safeselect-dbeaver-ux/atlas; export SAFESELECT_CONFIG_DIR=/tmp/safeselect-dbeaver-ux/global; export PATH=/tmp/safeselect-dbeaver-ux/bin:$PATH; unset NO_COLOR; export TERM=xterm-256color CLICOLOR=1 CLICOLOR_FORCE=1; export PS1='demo $ '; clear"
Enter
Sleep 1s
Show
Type "safeselect import-dbeaver ../dbeaver.zip"
Enter
Sleep 1s
# Select synthetic connection
Space
Enter
Sleep 1s
# Environment name
Enter
Sleep 1s
# Configure SSH
Enter
Sleep 1s
# Bastion host
Enter
Sleep 1s
# Bastion port
Enter
Sleep 1s
# Bastion username
Enter
Sleep 1s
# Forward host
Enter
Sleep 1s
# Forward port
Enter
Sleep 1s
# Password authentication
Enter
Sleep 1s
# Enter hidden SSH password
Enter
Sleep 1s
# Session-only SSH storage, not Keychain
Down
Enter
Sleep 1s
# Confirm session-only storage
Type "y"
Enter
Sleep 1s
# Friendly SSH variable
Enter
Sleep 1s
# Hidden synthetic SSH password
Type "synthetic-ssh-password"
Enter
Sleep 1s
# Use exported database password
Enter
Sleep 1s
# Session-only DB storage, not Keychain
Down
Enter
Sleep 1s
# Confirm session-only storage
Type "y"
Enter
Sleep 1s
# Friendly DB variable
Enter
Sleep 1s
# Decline connectivity check
Enter
Sleep 1s
Sleep 2s
# Give the final password-free configuration its own clean screen.
Type "clear"
Enter
Sleep 500ms
Type "cat .safeselect/environments/staging.toml"
Enter
Sleep 4s
