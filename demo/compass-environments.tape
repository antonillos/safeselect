# Current source build; synthetic pre/pro connections, no network or Keychain writes.
Output "demo/recordings/compass-environments.gif"
Output "demo/recordings/compass-environments.mp4"
Output "demo/.runtime/compass-environments-screen.txt"
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
Type "cd /tmp/safeselect-compass-environments/atlas; export SAFESELECT_CONFIG_DIR=/tmp/safeselect-compass-environments/global; export PATH=/tmp/safeselect-compass-environments/bin:$PATH; unset NO_COLOR; export TERM=xterm-256color CLICOLOR=1 CLICOLOR_FORCE=1; export PS1='demo $ '; clear"
Enter
Sleep 1s
Show
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 2s
# Select both connections in a single import.
Space
Down
Space
Enter
Sleep 3s
# Update pre, retaining its existing password variable reference.
Enter
Sleep 1s
Type "y"
Enter
Sleep 1s
Enter
# Hold the boundary: pre's summary followed by pro's distinct section.
Sleep 5s
Enter
Sleep 3s
Type "y"
Enter
Sleep 1s
Enter
Sleep 3s
# Decline connectivity checks; endpoints deliberately cannot resolve.
Enter
Sleep 3s
Type "ls .safeselect/environments"
Enter
Sleep 4s
