# Current source build on macOS; not a released version. No network or Keychain writes.
Output "demo/recordings/compass-reimport.gif"
Output "demo/recordings/compass-reimport.mp4"
Output "demo/.runtime/compass-reimport-screen.txt"
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
Type "cd /tmp/safeselect-compass-reimport/atlas; export SAFESELECT_CONFIG_DIR=/tmp/safeselect-compass-reimport/global; export PATH=/tmp/safeselect-compass-reimport/bin:$PATH; unset NO_COLOR; export TERM=xterm-256color CLICOLOR=1 CLICOLOR_FORCE=1; export PS1='demo $ '; clear"
Enter
Sleep 1s
Show
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 1s
Space
Enter
Sleep 1s
Enter
Sleep 1s
Type "y"
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Sleep 2s
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 1s
Space
Enter
Sleep 1s
Down
Enter
Sleep 1s
Type "staging-copy"
Enter
Sleep 1s
Enter
Sleep 1s
Enter
Sleep 1s
Down
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
Type "synthetic-hidden-password"
Enter
Sleep 1s
Enter
Sleep 1s
Sleep 2s
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 1s
Space
Enter
Sleep 1s
Down 2
Enter
Sleep 1s
Sleep 2s
Type "ls .safeselect/environments"
Enter
Sleep 1s
Sleep 4s
