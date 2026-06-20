#[path = "security_suite/mod.rs"]
mod security_suite;

#[test]
fn real_postgres_security_rejections_and_limits() {
    security_suite::real_postgres::run();
}

#[test]
fn real_postgres_reconnect_after_docker_restart() {
    security_suite::reconnect::run();
}
