pub mod process;
pub mod server;
pub mod validation;
pub mod logging;

#[cfg(test)]
pub mod test_utils {
    use std::sync::{Mutex, MutexGuard, OnceLock};
    /// A global env lock shared across ALL test modules to prevent races on env vars.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|e| e.into_inner())
    }
}
