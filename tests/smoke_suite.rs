#[path = "smoke_suite/mod.rs"]
mod smoke_suite;

#[test]
fn postgres_smoke_errors_and_timeouts() {
    smoke_suite::smoke::run();
}

#[test]
fn postgres_reconnect_after_docker_restart() {
    smoke_suite::reconnect::run();
}
