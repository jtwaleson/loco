use crate::{
    app::{AppContext, SharedStore},
    environment::Environment,
    tests_cfg::config::test_config,
};

pub async fn get_app_context() -> AppContext {
    AppContext {
        environment: Environment::Test,
        #[cfg(feature = "with-db")]
        db: super::db::dummy_connection().await,
        queue_provider: None,
        config: test_config(),
        mailer: None,
        shared_store: std::sync::Arc::new(SharedStore::default()),
    }
}
