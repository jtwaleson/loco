//! # Database Operations
//!
//! This module defines functions and operations related to the application's
//! database interactions.

use super::Result as AppResult;
use crate::{
    app::{AppContext, Hooks},
    config,
    errors::Error,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use sea_orm::{
    ActiveModelTrait, ConnectOptions, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, DbBackend, DbConn, DbErr, EntityTrait, IntoActiveModel, Statement,
};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    fs::File,
    io::Write,
    path::Path,
    sync::OnceLock,
    time::Duration,
};
use tracing::info;

pub static EXTRACT_DB_NAME: OnceLock<Regex> = OnceLock::new();
const IGNORED_TABLES: &[&str] = &[
    "seaql_migrations",
    "pg_loco_queue",
    "sqlt_loco_queue",
    "sqlt_loco_queue_lock",
];

fn re_extract_db_name() -> &'static Regex {
    EXTRACT_DB_NAME.get_or_init(|| {
        Regex::new(r"^.+://(?:.*?/)?([^/?#]+)(?:[?#]|$)").expect("Extract db regex is correct")
    })
}

/// Verifies a user has access to data within its database
///
/// # Errors
///
/// This function will return an error if IO fails
#[allow(unreachable_patterns)]
pub async fn verify_access(db: &DatabaseConnection) -> AppResult<()> {
    match db {
        DatabaseConnection::SqlxPostgresPoolConnection(_) => {
            let res = db
                .query_all(Statement::from_string(
                    DatabaseBackend::Postgres,
                    "SELECT * FROM pg_catalog.pg_tables WHERE tableowner = current_user;",
                ))
                .await?;
            if res.is_empty() {
                return Err(Error::string(
                    "current user has no access to tables in the database",
                ));
            }
        }
        DatabaseConnection::Disconnected => {
            return Err(Error::string("connection to database has been closed"));
        }
        _ => {
            return Err(Error::string(
                "unsupported database backend; only Postgres is supported",
            ));
        }
    }
    Ok(())
}
/// converge database logic
///
/// # Errors
///
///  an `AppResult`, which is an alias for `Result<(), AppError>`. It may
/// return an `AppError` variant representing different database operation
/// failures.
pub async fn converge<H: Hooks>(
    ctx: &AppContext,
    config: &config::Database,
) -> AppResult<()> {
    if config.dangerously_recreate {
        return Err(Error::string(
            "database recreate requires an app-level migration runner; this crate no longer ships one",
        ));
    }

    if config.auto_migrate {
        return Err(Error::string(
            "database auto_migrate requires an app-level migration runner; this crate no longer ships one",
        ));
    }

    if config.dangerously_truncate {
        info!("truncating tables");
        H::truncate(ctx).await?;
    }
    Ok(())
}

/// Establish a connection to the database using the provided configuration
/// settings.
///
/// # Errors
///
/// Returns a [`sea_orm::DbErr`] if an error occurs during the database
/// connection establishment.
pub async fn connect(config: &config::Database) -> Result<DbConn, sea_orm::DbErr> {
    if config.uri.starts_with("sqlite:") {
        return Err(sea_orm::DbErr::Custom(
            "SQLite is no longer supported by loco-rs core".to_string(),
        ));
    }

    let mut opt = ConnectOptions::new(&config.uri);
    opt.max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(Duration::from_millis(config.connect_timeout))
        .idle_timeout(Duration::from_millis(config.idle_timeout))
        .sqlx_logging(config.enable_logging);

    if let Some(acquire_timeout) = config.acquire_timeout {
        opt.acquire_timeout(Duration::from_millis(acquire_timeout));
    }

    let db = Database::connect(opt).await?;

    match db.get_database_backend() {
        DatabaseBackend::Postgres | DatabaseBackend::MySql => {
            if let Some(run_on_start) = &config.run_on_start {
                db.execute(Statement::from_string(
                    db.get_database_backend(),
                    run_on_start.clone(),
                ))
                .await?;
            }
        }
        DatabaseBackend::Sqlite => {
            return Err(sea_orm::DbErr::Custom(
                "SQLite is no longer supported by loco-rs core".to_string(),
            ));
        }
    }

    Ok(db)
}

/// Extracts the database name from a given connection string.
///
/// # Errors
///
/// This function returns an error if the connection string does not match the
/// expected format.
pub fn extract_db_name(conn_str: &str) -> AppResult<&str> {
    re_extract_db_name()
        .captures(conn_str)
        .and_then(|cap| cap.get(1).map(|db| db.as_str()))
        .ok_or_else(|| Error::string("could not extract db_name"))
}
/// Apply migrations to the database using the provided migrator.
///
/// # Errors
///
/// Returns a [`sea_orm::DbErr`] if an error occurs during run migration up.
pub async fn migrate(_db: &DatabaseConnection) -> AppResult<()> {
    Err(Error::string(
        "db migrate is no longer available in loco-rs core; run migrations from your app crate",
    ))
}

/// Revert migrations to the database using the provided migrator.
///
/// # Errors
///
/// Returns a [`sea_orm::DbErr`] if an error occurs during run migration up.
pub async fn down(_db: &DatabaseConnection, _steps: u32) -> AppResult<()> {
    Err(Error::string(
        "db down is no longer available in loco-rs core; run migrations from your app crate",
    ))
}

/// Check the migration status of the database.
///
/// # Errors
///
/// Returns a [`sea_orm::DbErr`] if an error occurs during checking status
pub async fn status(_db: &DatabaseConnection) -> AppResult<()> {
    Err(Error::string(
        "db status is no longer available in loco-rs core; run migrations from your app crate",
    ))
}

/// Reset the database, dropping and recreating the schema and applying
/// migrations.
///
/// # Errors
///
/// Returns a [`sea_orm::DbErr`] if an error occurs during reset databases.
pub async fn reset(_db: &DatabaseConnection) -> AppResult<()> {
    Err(Error::string(
        "db reset is no longer available in loco-rs core; run migrations from your app crate",
    ))
}

use sea_orm::EntityName;
use serde_json::{json, Value};
/// Seed the database with data from a specified file.
/// Seeds open the file path and insert all file content into the DB.
///
/// The file content should be equal to the DB field parameters.
///
/// # Errors
///
/// Returns a [`AppResult`] if could not render the path content into
/// [`Vec<serde_json::Value>`] or could not inset the vector to DB.
#[allow(clippy::type_repetition_in_bounds)]
pub async fn seed<A>(db: &DatabaseConnection, path: &str) -> crate::Result<()>
where
    <<A as ActiveModelTrait>::Entity as EntityTrait>::Model: IntoActiveModel<A>,
    for<'de> <<A as ActiveModelTrait>::Entity as EntityTrait>::Model: serde::de::Deserialize<'de>,
    A: ActiveModelTrait + Send + Sync,
    sea_orm::Insert<A>: Send + Sync,
    <A as ActiveModelTrait>::Entity: EntityName,
{
    // Deserialize YAML file into a vector of JSON values
    let seed_data: Vec<Value> = serde_yaml::from_reader(File::open(path)?)?;

    // Insert each row
    let mut seed_models = Vec::new();
    for row in seed_data {
        let model = A::from_json(row)?;
        seed_models.push(model);
    }
    A::Entity::insert_many(seed_models).exec(db).await?;

    // Get the table name from the entity
    let table_name = A::Entity::default().table_name().to_string();

    // Get the database backend
    let db_backend = db.get_database_backend();

    // Reset auto-increment
    reset_autoincrement(db_backend, &table_name, db).await?;

    Ok(())
}

/// Checks if the specified table has an 'id' column.
///
/// This function checks if the specified table has an 'id' column, which is a
/// common primary key column. It supports `Postgres`, `SQLite`, and `MySQL`
/// database backends.
///
/// # Arguments
///
/// - `db`: A reference to the `DatabaseConnection`.
/// - `db_backend`: A reference to the `DatabaseBackend`.
/// - `table_name`: The name of the table to check.
///
/// # Returns
///
/// A `Result` containing a `bool` indicating whether the table has an 'id'
/// column.
async fn has_id_column(
    db: &DatabaseConnection,
    db_backend: &DatabaseBackend,
    table_name: &str,
) -> crate::Result<bool> {
    // First check if 'id' column exists
    let result = match db_backend {
        DatabaseBackend::Postgres => {
            let query = format!(
                "SELECT EXISTS (
              SELECT 1 
              FROM information_schema.columns 
              WHERE table_name = '{table_name}' 
              AND column_name = 'id'
          )"
            );
            let result = db
                .query_one(Statement::from_string(DatabaseBackend::Postgres, query))
                .await?;
            result.is_some_and(|row| row.try_get::<bool>("", "exists").unwrap_or(false))
        }
        DatabaseBackend::Sqlite => {
            return Err(Error::Message(
                "Unsupported database backend: SQLite".to_string(),
            ));
        }
        DatabaseBackend::MySql => {
            return Err(Error::Message(
                "Unsupported database backend: MySQL".to_string(),
            ));
        }
    };

    Ok(result)
}

/// Checks whether the specified table has an auto-increment 'id' column.
///
/// # Returns
///
/// A `Result` containing a `bool` indicating whether the table has an
/// auto-increment 'id' column.
async fn is_auto_increment(
    db: &DatabaseConnection,
    db_backend: &DatabaseBackend,
    table_name: &str,
) -> crate::Result<bool> {
    let result = match db_backend {
        DatabaseBackend::Postgres => {
            let query = format!(
                "SELECT pg_get_serial_sequence('{table_name}', 'id') IS NOT NULL as is_serial"
            );
            let result = db
                .query_one(Statement::from_string(DatabaseBackend::Postgres, query))
                .await?;
            result.is_some_and(|row| row.try_get::<bool>("", "is_serial").unwrap_or(false))
        }
        DatabaseBackend::Sqlite => {
            return Err(Error::Message(
                "Unsupported database backend: SQLite".to_string(),
            ));
        }
        DatabaseBackend::MySql => {
            return Err(Error::Message(
                "Unsupported database backend: MySQL".to_string(),
            ));
        }
    };
    Ok(result)
}

/// Function to reset auto-increment
/// # Errors
/// Returns error if it fails
pub async fn reset_autoincrement(
    db_backend: DatabaseBackend,
    table_name: &str,
    db: &DatabaseConnection,
) -> crate::Result<()> {
    // Check if 'id' column exists
    let has_id_column = has_id_column(db, &db_backend, table_name).await?;
    if !has_id_column {
        return Ok(());
    }
    // Check if 'id' column is auto-increment
    let is_auto_increment = is_auto_increment(db, &db_backend, table_name).await?;
    if !is_auto_increment {
        return Ok(());
    }

    match db_backend {
        DatabaseBackend::Postgres => {
            let query_str = format!(
                "SELECT setval(pg_get_serial_sequence('{table_name}', 'id'), COALESCE(MAX(id), 0) \
                 + 1, false) FROM {table_name}"
            );
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &query_str,
                vec![],
            ))
            .await?;
        }
        DatabaseBackend::Sqlite => {
            return Err(Error::Message(
                "Unsupported database backend: SQLite".to_string(),
            ));
        }
        DatabaseBackend::MySql => {
            return Err(Error::Message(
                "Unsupported database backend: MySQL".to_string(),
            ));
        }
    }
    Ok(())
}

/// Truncate a table in the database, effectively deleting all rows.
///
/// # Errors
///
/// Returns a [`AppResult`] if an error occurs during truncate the given table
pub async fn truncate_table<T>(db: &DatabaseConnection, _: T) -> Result<(), sea_orm::DbErr>
where
    T: EntityTrait,
{
    T::delete_many().exec(db).await?;
    Ok(())
}

/// Execute seed from the given path
///
/// # Errors
///
/// when seed process is fails
pub async fn run_app_seed<H: Hooks>(ctx: &AppContext, path: &Path) -> AppResult<()> {
    H::seed(ctx, path).await
}

/// Retrieves a list of table names from the database.
///
///
/// # Errors
///
/// Returns an error if the operation fails for any reason, such as an
/// unsupported database backend or a query execution issue.
pub async fn get_tables(db: &DatabaseConnection) -> AppResult<Vec<String>> {
    let query = match db.get_database_backend() {
        DatabaseBackend::MySql => {
            return Err(Error::Message(
                "Unsupported database backend: MySQL".to_string(),
            ));
        }
        DatabaseBackend::Postgres => {
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        }
        DatabaseBackend::Sqlite => {
            return Err(Error::Message(
                "Unsupported database backend: SQLite".to_string(),
            ));
        }
    };

    let result = db
        .query_all(Statement::from_string(
            db.get_database_backend(),
            query.to_string(),
        ))
        .await?;

    Ok(result
        .into_iter()
        .filter_map(|row| {
            let col = match db.get_database_backend() {
                sea_orm::DatabaseBackend::MySql
                | sea_orm::DatabaseBackend::Postgres
                | sea_orm::DatabaseBackend::Sqlite => "table_name",
            };

            if let Ok(table_name) = row.try_get::<String>("", col) {
                if IGNORED_TABLES.contains(&table_name.as_str()) {
                    return None;
                }
                Some(table_name)
            } else {
                None
            }
        })
        .collect())
}

/// Returns the set of column names that are of boolean type for the given table.
///
/// This uses lightweight schema metadata queries and is used by the dump logic
/// to serialize boolean columns as `true` / `false` instead of `0` / `1`.
async fn get_boolean_columns(
    db: &DatabaseConnection,
    table_name: &str,
) -> AppResult<HashSet<String>> {
    let backend = db.get_database_backend();
    let mut bool_cols = HashSet::new();

    match backend {
        DatabaseBackend::Postgres => {
            let query = "
                SELECT column_name
                FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND table_name = $1
                  AND data_type = 'boolean'
            ";

            let stmt = Statement::from_sql_and_values(backend, query, vec![table_name.into()]);
            let rows = db.query_all(stmt).await?;
            for row in rows {
                if let Ok(col) = row.try_get::<String>("", "column_name") {
                    bool_cols.insert(col);
                }
            }
        }
        DatabaseBackend::Sqlite => {
            return Err(Error::Message(
                "Unsupported database backend: SQLite".to_string(),
            ));
        }
        DatabaseBackend::MySql => {
            return Err(Error::Message(
                "Unsupported database backend: MySQL".to_string(),
            ));
        }
    }

    Ok(bool_cols)
}

/// Dumps the contents of specified database tables into YAML files.
///
/// # Errors
/// This function retrieves data from all tables in the database, filters them
/// if `only_tables` is provided, and writes each table's content to a separate
/// YAML file in the specified directory.
///
/// Returns an error if the operation fails for any reason or could not save the
/// content into a file.
#[allow(clippy::cognitive_complexity)]
pub async fn dump_tables(
    db: &DatabaseConnection,
    to: &Path,
    only_tables: Option<Vec<String>>,
) -> AppResult<()> {
    tracing::debug!("getting tables from the database");

    let tables = get_tables(db).await?;
    tracing::info!(tables = ?tables, "found tables");

    for table in tables {
        if let Some(ref only_tables) = only_tables {
            if !only_tables.contains(&table) {
                tracing::info!(table, "skipping table as it is not in the specified list");
                continue;
            }
        }

        tracing::info!(table, "get table data");

        // Discover which columns are booleans so we can dump them as true/false
        // instead of 0/1 while keeping numeric IDs and other integers intact.
        let boolean_columns = get_boolean_columns(db, &table).await?;

        let data_result = db
            .query_all(Statement::from_string(
                db.get_database_backend(),
                format!(r#"SELECT * FROM "{table}""#),
            ))
            .await?;

        tracing::info!(
            table,
            rows_fetched = data_result.len(),
            "fetched rows from table"
        );

        let mut table_data: Vec<BTreeMap<String, serde_json::Value>> = Vec::new();

        if !to.exists() {
            tracing::info!("the specified dump folder does not exist. creating the folder now");
            fs::create_dir_all(to)?;
        }

        for row in data_result {
            // Use BTreeMap to ensure deterministic key order in YAML snapshots.
            let mut row_data: BTreeMap<String, serde_json::Value> = BTreeMap::new();

            for col_name in row.column_names() {
                let value_result = row
                    // Try native JSON value first so JSON/JSONB columns and JSON-like
                    // text (e.g. '{\"k\": \"v\"}', '[1,2,3]') are captured as structured
                    // data instead of being double-quoted strings in YAML.
                    .try_get::<serde_json::Value>("", &col_name)
                    // Fallback to string and scalar types for non-JSON columns.
                    .or_else(|_| {
                        row.try_get::<String>("", &col_name)
                            .map(serde_json::Value::String)
                    })
                    .or_else(|_| {
                        row.try_get::<i8>("", &col_name)
                            .map(serde_json::Value::from)
                    })
                    .or_else(|_| {
                        row.try_get::<i16>("", &col_name)
                            .map(serde_json::Value::from)
                    })
                    .or_else(|_| {
                        row.try_get::<i32>("", &col_name)
                            .map(serde_json::Value::from)
                    })
                    .or_else(|_| {
                        row.try_get::<i64>("", &col_name)
                            .map(serde_json::Value::from)
                    })
                    .or_else(|_| {
                        row.try_get::<f32>("", &col_name)
                            .map(serde_json::Value::from)
                    })
                    .or_else(|_| {
                        row.try_get::<f64>("", &col_name)
                            .map(serde_json::Value::from)
                    })
                    .or_else(|_| {
                        row.try_get::<uuid::Uuid>("", &col_name)
                            .map(|v| serde_json::Value::String(v.to_string()))
                    })
                    .or_else(|_| {
                        row.try_get::<DateTime<Utc>>("", &col_name)
                            .map(|v| serde_json::Value::String(v.to_rfc3339()))
                    })
                    .ok();

                if let Some(mut value) = value_result {
                    if boolean_columns.contains(&col_name) {
                        if let serde_json::Value::Number(num) = &value {
                            if let Some(i) = num.as_i64() {
                                value = serde_json::Value::Bool(i != 0);
                            }
                        }
                    }

                    row_data.insert(col_name, value);
                }
            }
            table_data.push(row_data);
        }

        let data = serde_yaml::to_string(&table_data)?;

        let file_db_content_path = to.join(format!("{table}.yaml"));

        let mut file = File::create(&file_db_content_path)?;
        file.write_all(data.as_bytes())?;
        tracing::info!(table, file_db_content_path = %file_db_content_path.display(), "table data written to YAML file");
    }

    tracing::info!("dumping tables process completed successfully");

    Ok(())
}

/// dumps the db schema into file.
///
/// # Errors
/// Fails with IO / sql fails
pub async fn dump_schema(ctx: &AppContext, fname: &str) -> crate::Result<()> {
    let db = &ctx.db;

    // Match the database backend and fetch schema info
    let schema_info = match db.get_database_backend() {
        DbBackend::Postgres => {
            let query = r"
                SELECT table_name, column_name, data_type
                FROM information_schema.columns
                WHERE table_schema = 'public'
                ORDER BY table_name, ordinal_position;
            ";
            let stmt = Statement::from_string(DbBackend::Postgres, query.to_owned());
            let rows = db.query_all(stmt).await?;
            rows.into_iter()
                .map(|row| {
                    // Wrap the closure in a Result to handle errors properly
                    Ok(json!({
                        "table": row.try_get::<String>("", "table_name")?,
                        "column": row.try_get::<String>("", "column_name")?,
                        "type": row.try_get::<String>("", "data_type")?,
                    }))
                })
                .collect::<Result<Vec<serde_json::Value>, DbErr>>()? // Specify error type explicitly
        }
        DbBackend::MySql => {
            let query = r"
                SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE
                FROM INFORMATION_SCHEMA.COLUMNS
                WHERE TABLE_SCHEMA = DATABASE()
                ORDER BY TABLE_NAME, ORDINAL_POSITION;
            ";
            let stmt = Statement::from_string(DbBackend::MySql, query.to_owned());
            let rows = db.query_all(stmt).await?;
            rows.into_iter()
                .map(|row| {
                    // Wrap the closure in a Result to handle errors properly
                    Ok(json!({
                        "table": row.try_get::<String>("", "TABLE_NAME")?,
                        "column": row.try_get::<String>("", "COLUMN_NAME")?,
                        "type": row.try_get::<String>("", "COLUMN_TYPE")?,
                    }))
                })
                .collect::<Result<Vec<serde_json::Value>, DbErr>>()? // Specify error type explicitly
        }
        DbBackend::Sqlite => {
            return Err(Error::Message(
                "Unsupported database backend: SQLite".to_string(),
            ));
        }
    };
    // Serialize schema info to JSON format
    let schema_json = serde_json::to_string_pretty(&schema_info)?;

    // Save the schema to a file
    std::fs::write(fname, schema_json)?;
    Ok(())
}

