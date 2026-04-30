use crate::{
    config::{self, Config},
    controller::middleware,
    logger,
};

#[must_use]
pub fn test_config() -> Config {
    Config {
        logger: config::Logger {
            enable: false,
            pretty_backtrace: true,
            level: logger::LogLevel::Off,
            format: logger::Format::Json,
            override_filter: None,
            file_appender: None,
        },
        server: config::Server {
            binding: "localhost".to_string(),
            port: 5555,
            host: "localhost".to_string(),
            middlewares: middleware::Config::default(),
        },
        #[cfg(feature = "with-db")]
        database: get_database_config(),
        queue: None,
        auth: None,
        workers: config::Workers {
            mode: config::WorkerMode::ForegroundBlocking,
        },
        mailer: None,
        initializers: None,
        settings: None,
    }
}

#[must_use]
pub fn get_database_config() -> config::Database {
    config::Database {
        uri: "postgres://postgres:postgres@localhost:5432/postgres".to_string(),
        enable_logging: false,
        min_connections: 1,
        max_connections: 1,
        connect_timeout: 500,
        idle_timeout: 500,
        acquire_timeout: None,
        auto_migrate: false,
        dangerously_truncate: false,
        dangerously_recreate: false,
        run_on_start: None,
    }
}
