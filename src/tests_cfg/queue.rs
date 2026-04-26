#[cfg(feature = "bg_pg")]
use crate::bgworker;
use std::path::PathBuf;

#[cfg(feature = "bg_pg")]
/// # Panics
///
/// This function will panic if it fails to prepare or insert the seed data, causing the tests to fail quickly
/// and preventing further test execution with incomplete setup.
pub async fn postgres_seed_data(pool: &sqlx::PgPool) {
    let yaml_tasks = std::fs::read_to_string(
        PathBuf::from("tests")
            .join("fixtures")
            .join("queue")
            .join("jobs.yaml"),
    )
    .expect("Failed to read YAML file");

    let tasks: Vec<bgworker::pg::Job> =
        serde_yaml::from_str(&yaml_tasks).expect("Failed to parse YAML");
    for task in tasks {
        sqlx::query(
            r"
            INSERT INTO pg_loco_queue (id, name, task_data, status, run_at, interval, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)
            ",
        )
        .bind(task.id)
        .bind(task.name)
        .bind(task.data)
        .bind(task.status.to_string())
        .bind(task.run_at)
        .bind(task.created_at)
        .bind(task.updated_at)
        .execute(pool)
        .await.expect("execute insert query");
    }
}

