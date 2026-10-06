# Requires a public release containing PR #279; do not render with v0.7.10.
Output "demo/recordings/linux-onboarding-envrefs.gif"
Output "demo/recordings/linux-onboarding-envrefs.mp4"
Output "demo/.runtime/linux-onboarding/screen.txt"
Require docker
Set Shell "bash"
Set FontSize 18
Set Width 1440
Set Height 900
Set Padding 20
Set Theme "GitHub Dark"
Set TypingSpeed 15ms
Set WindowBar "Colorful"
Set WaitTimeout 5m
Set Framerate 20
Hide
Type "docker exec -it safeselect-linux-onboarding-terminal bash --noprofile --norc"
Enter
Sleep 1s
Type "export PS1='linux $ '; clear"
Enter
Show
Type "uname -sm; java -version; asdf --version"
Enter
Sleep 3s
Type "asdf plugin add safeselect https://github.com/antonillos/asdf-safeselect.git"
Enter
Sleep 10s
Type "asdf install safeselect latest"
Enter
Wait+Screen /safeselect installed at/
Sleep 2s
Type "asdf set safeselect latest; asdf reshim safeselect; safeselect --version"
Enter
Sleep 3s
Type "safeselect import-compass --path /home/demo/compass.json"
Enter
Sleep 2s
Space
Enter
Sleep 1s
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
Type "{env:BASTION_PASSWORD}"
Enter
Sleep 3s
Type "safeselect config set-password --environment staging --password '{env:DB_PASSWORD}'"
Enter
Sleep 2s
Type "read -rsp 'Database password: ' DB_PASSWORD; echo; export DB_PASSWORD"
Enter
Sleep 1s
Type "demo-password"
Enter
Sleep 2s
Type "read -rsp 'Bastion password: ' BASTION_PASSWORD; echo; export BASTION_PASSWORD"
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
