use crate::{
    app::{AppContext, SharedStore},
    cache,
    environment::Environment,
    storage::{self, Storage},
    tests_cfg::config::test_config,
};

pub async fn get_app_context() -> AppContext {
    // Use null cache in tests by default.
    let cache = cache::Cache::new(cache::drivers::null::new());

    AppContext {
        environment: Environment::Test,
        #[cfg(feature = "with-db")]
        db: super::db::dummy_connection().await,
        queue_provider: None,
        config: test_config(),
        mailer: None,
        storage: Storage::single(storage::drivers::mem::new()).into(),
        cache: cache.into(),
        shared_store: std::sync::Arc::new(SharedStore::default()),
    }
}
