use super::super::ssh_connectivity_hint;

#[test]
fn ssh_connectivity_hint_uses_configured_endpoint_and_explains_wsl() {
    let hint = ssh_connectivity_hint("127.0.0.1", 2222);
    assert!(hint.contains("timeout 5 bash -c 'exec 3<>\"/dev/tcp/$1/$2\"' -- '127.0.0.1' 2222"));
    assert!(hint.contains("same WSL distribution as SafeSelect"));
    assert!(hint.contains("Keep the Azure CLI tunnel open"));
    assert!(hint.contains("TCP success does not validate SSH credentials or database access"));
}

#[test]
fn ssh_connectivity_hint_quotes_untrusted_host_as_one_argument() {
    let host = "host'; echo $(id); #";
    let hint = ssh_connectivity_hint(host, 2200);
    assert!(hint.contains("-- 'host'\"'\"'; echo $(id); #' 2200"));
    assert!(hint.contains("/dev/tcp/$1/$2"));
    assert!(!hint.contains(&format!("/dev/tcp/{host}")));
}
