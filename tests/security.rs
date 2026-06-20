#[path = "security_suite/mod.rs"]
mod security_suite;
#[path = "smoke_suite/postgres.rs"]
mod postgres;

#[test]
fn real_postgres_security_rejections_and_limits() {
    security_suite::real_postgres::run();
}
