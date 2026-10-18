# Current source build on macOS; not a released version. No network or Keychain writes.
Output "demo/recordings/compass-import-ux.gif"
Output "demo/recordings/compass-import-ux.mp4"
Output "demo/.runtime/compass-import-ux-screen.txt"
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
Type "cd /tmp/safeselect-compass-ux/atlas; export SAFESELECT_CONFIG_DIR=/tmp/safeselect-compass-ux/global; export PATH=/tmp/safeselect-compass-ux/bin:$PATH; unset NO_COLOR; export TERM=xterm-256color CLICOLOR=1 CLICOLOR_FORCE=1; export PS1='demo $ '; clear"
Enter
Sleep 1s
Show
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 1s
Space
Enter
# Hold the new connection heading before answering its first prompt.
Sleep 3s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Down
Enter
Sleep 1s
Type "y"
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Down
Enter
Sleep 1s
Type "y"
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Sleep 2s
Type "cat .safeselect/environments/staging.toml"
Enter
Sleep 1s
Sleep 3s
Sleep 4s
