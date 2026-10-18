# Current Linux source build, not a public release or asdf installation demo.
Output "demo/recordings/linux-onboarding-envrefs.gif"
Output "demo/recordings/linux-onboarding-envrefs.mp4"
Output "demo/.runtime/linux-onboarding/screen.txt"
Require docker
Set Shell "bash"
Set FontSize 18
Set Width 1600
Set Height 1000
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 15ms
Set WindowBar "Colorful"
Set WaitTimeout 5m
Set Framerate 20
Hide
Type "docker exec -it safeselect-linux-onboarding-terminal bash --noprofile --norc"
Enter
Sleep 1s
Type "export PATH=/home/demo/.local/bin:$PATH; unset NO_COLOR; export TERM=xterm-256color CLICOLOR=1 CLICOLOR_FORCE=1; export PS1='linux $ '; clear"
Enter
Show
Type "uname -sm; java -version; safeselect --version; printf 'Current source revision: '; cat /home/demo/source-revision.txt"
Enter
Sleep 3s
Type "safeselect import-compass --path /home/demo/compass.json"
Enter
Sleep 2s
Space
Enter
# Hold the new connection heading before answering its first prompt.
Sleep 3s
Type "staging"
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
# SSH: reuse exported literal, choose process-local storage, confirm, accept variable.
Enter
Sleep 1s
Enter
Sleep 1s
Type "y"
Enter
Sleep 1s
Enter
Sleep 1s
# Database: same credential choices and readable project prefix.
Enter
Sleep 1s
Enter
Sleep 1s
Type "y"
Enter
Sleep 1s
Enter
Sleep 1s
# Skip immediate connectivity: import-process values are not parent-shell exports.
Enter
Sleep 2s
Type "read -rsp 'Database password: ' WORKSPACE_STAGING_DB_PASSWORD; echo; export WORKSPACE_STAGING_DB_PASSWORD"
Enter
Sleep 1s
Type "demo-password"
Enter
Sleep 2s
Type "read -rsp 'Bastion password: ' STAGING_SSH_PASSWORD; echo; export STAGING_SSH_PASSWORD"
Enter
Sleep 1s
Type "bastion-demo-password"
Enter
Sleep 2s
Type "cat .safeselect/environments/staging.toml"
Enter
Sleep 3s
Type "sed -n '/^\[ssh_bastions/,/^\[security/p' .safeselect/project.toml"
Enter
Sleep 3s
Type `safeselect check --environment staging; printf '\nCheck exit status: %s\n' "$?"`
Enter
Sleep 20s
