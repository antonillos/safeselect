#[path = "smoke_suite/postgres.rs"]
mod postgres;
#[path = "security_suite/mod.rs"]
mod security_suite;

#[test]
fn real_postgres_security_rejections_and_limits() {
    security_suite::real_postgres::run();
}
