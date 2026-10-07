# Current source build on macOS; not a released version. No network or Keychain writes.
Output "demo/recordings/compass-import-ux.gif"
Output "demo/recordings/compass-import-ux.mp4"
Output "demo/.runtime/compass-import-ux-screen.txt"
Set Shell "bash"
Set FontSize 17
Set Width 1440
Set Height 1000
Set Padding 20
Set Theme "GitHub Dark"
Set TypingSpeed 15ms
Set WindowBar "Colorful"
Set Framerate 15
Hide
Type "cd /tmp/safeselect-compass-ux/atlas; export SAFESELECT_CONFIG_DIR=/tmp/safeselect-compass-ux/global; export PATH=/tmp/safeselect-compass-ux/bin:$PATH; export PS1='demo $ '; clear"
Enter
Sleep 1s
Show
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 1s
Space
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Down
Enter
Sleep 500ms
Type "y"
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Down
Enter
Sleep 500ms
Type "y"
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Sleep 2s
Type "cat .safeselect/environments/staging.toml"
Enter
Sleep 500ms
Sleep 3s
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 500ms
Space
Enter
Sleep 500ms
Enter
Sleep 500ms
Type "y"
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Sleep 2s
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 500ms
Space
Enter
Sleep 500ms
Down
Enter
Sleep 500ms
Type "staging-copy"
Enter
Sleep 500ms
Enter
Sleep 500ms
Enter
Sleep 500ms
Down
Enter
Sleep 500ms
Down
Enter
Sleep 500ms
Type "y"
Enter
Sleep 500ms
Enter
Sleep 500ms
Type "synthetic-hidden-password"
Enter
Sleep 500ms
Enter
Sleep 500ms
Sleep 2s
Type "safeselect import-compass --path ../compass.json"
Enter
Sleep 500ms
Space
Enter
Sleep 500ms
Down 2
Enter
Sleep 500ms
Sleep 2s
Type "ls .safeselect/environments"
Enter
Sleep 500ms
Sleep 4s
