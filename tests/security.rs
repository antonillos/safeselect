#[path = "smoke_suite/mongodb.rs"]
mod mongodb;
#[path = "smoke_suite/postgres.rs"]
mod postgres;
#[path = "security_suite/mod.rs"]
mod security_suite;

fn security_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn real_postgres_security_rejections_and_limits() {
    let _guard = security_test_lock();
    security_suite::real_postgres::run();
}

#[test]
fn real_mongodb_security_rejections_and_limits() {
    let _guard = security_test_lock();
    security_suite::real_mongodb::run();
}
