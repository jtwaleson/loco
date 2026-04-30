/// Postgres based background job queue provider
use std::{
    collections::HashMap, future::Future, panic::AssertUnwindSafe, pin::Pin, sync::Arc,
    time::Duration,
};

use super::{BackgroundWorker, JobStatus, Queue};
use crate::{config::PostgresQueueConfig, Error, Result};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
pub use sqlx::PgPool;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions, PgRow},
    ConnectOptions, Row,
};
use std::fmt::Write;
use tokio::{task::JoinHandle, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, trace};
use ulid::Ulid;
type JobId = String;
type JobData = JsonValue;

type JobHandler = Box<
    dyn Fn(
            JobId,
            JobData,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<(), crate::Error>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Job {
    pub id: JobId,
    pub name: String,
    #[serde(rename = "task_data")]
    pub data: JobData,
    pub status: JobStatus,
    pub run_at: DateTime<Utc>,
    pub interval: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub priority: i32,
}

pub struct JobRegistry {
    handlers: Arc<HashMap<String, JobHandler>>,
}

impl JobRegistry {
    /// Creates a new `JobRegistry`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: Arc::new(HashMap::new()),
        }
    }

    /// Registers a job handler with the provided name.
    /// # Errors
    /// Fails if cannot register worker
    pub fn register_worker<Args, W>(&mut self, name: String, worker: W) -> Result<()>
    where
        Args: Send + Serialize + Sync + 'static,
        W: BackgroundWorker<Args> + 'static,
        for<'de> Args: Deserialize<'de>,
    {
        let worker = Arc::new(worker);
        let wrapped_handler = move |_job_id: String, job_data: JobData| {
            let w = worker.clone();

            Box::pin(async move {
                let args = serde_json::from_value::<Args>(job_data);
                match args {
                    Ok(args) => {
                        // Wrap the perform call in catch_unwind to handle panics
                        match AssertUnwindSafe(w.perform(args)).catch_unwind().await {
                            Ok(result) => result,
                            Err(panic) => {
                                let panic_msg = panic
                                    .downcast_ref::<String>()
                                    .map(String::as_str)
                                    .or_else(|| panic.downcast_ref::<&str>().copied())
                                    .unwrap_or("Unknown panic occurred");
                                error!(err = panic_msg, "worker panicked");
                                Err(Error::string(panic_msg))
                            }
                        }
                    }
                    Err(err) => Err(err.into()),
                }
            }) as Pin<Box<dyn Future<Output = Result<(), crate::Error>> + Send>>
        };

        Arc::get_mut(&mut self.handlers)
            .ok_or_else(|| Error::string("cannot register worker"))?
            .insert(name, Box::new(wrapped_handler));
        Ok(())
    }

    /// Returns a reference to the job handlers.
    #[must_use]
    pub fn handlers(&self) -> &Arc<HashMap<String, JobHandler>> {
        &self.handlers
    }

    /// Runs the job handlers with the provided number of workers.
    #[must_use]
    pub fn run(
        &self,
        pool: &PgPool,
        opts: &RunOpts,
        token: &CancellationToken,
        tags: &[String],
    ) -> Vec<JoinHandle<()>> {
        let mut jobs = Vec::new();

        let interval = opts.poll_interval_sec;
        for idx in 0..opts.num_workers {
            let handlers = self.handlers.clone();
            let worker_token = token.clone(); // Clone token for this worker
            let worker_tags = tags.to_vec();

            let pool = pool.clone();
            let job = tokio::spawn(async move {
                loop {
                    // Check for cancellation before potentially blocking on dequeue
                    if worker_token.is_cancelled() {
                        trace!(worker_id = idx, "Cancellation received, stopping worker");
                        break;
                    }
                    trace!(
                        pool_size = pool.num_idle(),
                        worker_id = idx,
                        "Connection pool stats"
                    );
                    let job_opt = match dequeue(&pool, &worker_tags).await {
                        Ok(t) => t,
                        Err(err) => {
                            error!(error = %err, "Failed to fetch job from queue");
                            None
                        }
                    };

                    if let Some(job) = job_opt {
                        debug!(job_id = %job.id, job_name = %job.name, "Processing job");
                        if let Some(handler) = handlers.get(&job.name) {
                            match handler(job.id.clone(), job.data.clone()).await {
                                Ok(()) => {
                                    if let Err(err) =
                                        complete_job(&pool, &job.id, job.interval).await
                                    {
                                        error!(
                                            error = %err,
                                            job_id = %job.id,
                                            job_name = %job.name,
                                            "Failed to mark job as completed"
                                        );
                                    } else {
                                        debug!(job_id = %job.id, "Job completed successfully");
                                    }
                                }
                                Err(err) => {
                                    if let Err(fail_err) = fail_job(&pool, &job.id, &err).await {
                                        error!(
                                            error = %fail_err,
                                            job_id = %job.id,
                                            job_name = %job.name,
                                            "Failed to mark job as failed"
                                        );
                                    } else {
                                        debug!(job_id = %job.id, error = %err, "Job execution failed");
                                    }
                                }
                            }
                        } else {
                            error!(job_name = %job.name, "No handler registered for job");
                        }
                    } else {
                        // Use tokio::select! to wait for interval or cancellation
                        tokio::select! {
                            biased;
                            () = worker_token.cancelled() => {
                                trace!(worker_id = idx, "Cancellation received during sleep, stopping worker");
                                break;
                            }
                            () = sleep(Duration::from_secs(interval.into())) => {
                                // Interval elapsed, continue loop
                            }
                        }
                    }
                }
            });

            jobs.push(job);
        }

        jobs
    }
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

async fn connect(cfg: &PostgresQueueConfig) -> Result<PgPool> {
    let mut conn_opts: PgConnectOptions = cfg.uri.parse()?;
    if !cfg.enable_logging {
        conn_opts = conn_opts.disable_statement_logging();
    }
    let pool = PgPoolOptions::new()
        .min_connections(cfg.min_connections)
        .max_connections(cfg.max_connections)
        .idle_timeout(Duration::from_millis(cfg.idle_timeout))
        .acquire_timeout(Duration::from_millis(cfg.connect_timeout))
        .connect_with(conn_opts)
        .await?;
    Ok(pool)
}

/// Initialize job tables
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn initialize_database(pool: &PgPool) -> Result<()> {
    debug!("Initializing job database tables");

    // First, check if the table exists
    let table_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT FROM information_schema.tables 
            WHERE table_name = 'pg_loco_queue'
        )",
    )
    .fetch_one(pool)
    .await?;

    if !table_exists {
        // Create the table with all columns including priority
        sqlx::raw_sql(&format!(
            r"
                CREATE TABLE pg_loco_queue (
                    id VARCHAR NOT NULL,
                    name VARCHAR NOT NULL,
                    task_data JSONB NOT NULL,
                    status VARCHAR NOT NULL DEFAULT '{}',
                    attempts_left INT NOT NULL DEFAULT 3,
                    run_at TIMESTAMPTZ NOT NULL,
                    interval BIGINT,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                    tags JSONB,
                    priority INT NOT NULL DEFAULT 0
                );
                ",
            JobStatus::Queued
        ))
        .execute(pool)
        .await?;
    } else {
        // Check if priority column exists
        let priority_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT FROM information_schema.columns 
                WHERE table_name = 'pg_loco_queue' 
                AND column_name = 'priority'
            )",
        )
        .fetch_one(pool)
        .await?;

        if !priority_exists {
            debug!("Adding priority column to existing pg_loco_queue table");
            sqlx::query("ALTER TABLE pg_loco_queue ADD COLUMN priority INT NOT NULL DEFAULT 0")
                .execute(pool)
                .await?;
        }

        // Check if attempts_left column exists
        let attempts_left_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT FROM information_schema.columns
                WHERE table_name = 'pg_loco_queue'
                AND column_name = 'attempts_left'
            )",
        )
        .fetch_one(pool)
        .await?;

        if !attempts_left_exists {
            debug!("Adding attempts_left column to existing pg_loco_queue table");
            sqlx::query(
                "ALTER TABLE pg_loco_queue ADD COLUMN attempts_left INT NOT NULL DEFAULT 3",
            )
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}

/// Add a job
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn enqueue(
    pool: &PgPool,
    name: &str,
    data: JobData,
    run_at: DateTime<Utc>,
    interval: Option<Duration>,
    tags: Option<Vec<String>>,
    priority: Option<i32>,
) -> Result<JobId> {
    let data_json = serde_json::to_value(data)?;
    let tags_json = tags
        .as_ref()
        .map(|t| serde_json::to_value(t).unwrap_or(serde_json::Value::Null));

    #[allow(clippy::cast_possible_truncation)]
    let interval_ms: Option<i64> = interval.map(|i| i.as_millis() as i64);

    let id = Ulid::new().to_string();
    debug!(job_id = %id, job_name = %name, run_at = %run_at, tags = ?tags, "Enqueueing job");
    sqlx::query(
        "INSERT INTO pg_loco_queue (id, task_data, name, run_at, interval, tags, priority) VALUES ($1, $2, $3, \
         $4, $5, $6, $7)",
    )
    .bind(id.clone())
    .bind(data_json)
    .bind(name)
    .bind(run_at)
    .bind(interval_ms)
    .bind(tags_json)
    .bind(priority.unwrap_or(0))
    .execute(pool)
    .await?;
    Ok(id)
}

async fn dequeue(client: &PgPool, worker_tags: &[String]) -> Result<Option<Job>> {
    let mut tx = client.begin().await?;

    // Base query
    let mut query = String::from(
        "SELECT id, name, task_data, status, run_at, interval, tags, priority FROM pg_loco_queue WHERE status = $1 AND run_at <= NOW() "
    );

    // Apply tag filtering logic
    if worker_tags.is_empty() {
        // If worker has no tags, only process jobs with no tags
        query.push_str("AND (tags IS NULL) ");
    } else {
        // If worker has tags, we need a more complex condition
        query.push_str("AND (tags IS NOT NULL) ");

        // In PostgreSQL, we need to build a condition for each tag individually
        let mut conditions = Vec::new();

        for (i, _) in worker_tags.iter().enumerate() {
            // Check if the tag exists as a JSON string in the tags array
            // Using ? operator checks if string exists as array element
            conditions.push(format!("(tags)::jsonb ? ${}", i + 2));
        }

        if !conditions.is_empty() {
            query.push_str(" AND (");
            query.push_str(&conditions.join(" OR "));
            query.push(')');
        }
    }

    query.push_str(" ORDER BY priority DESC, run_at LIMIT 1 FOR UPDATE SKIP LOCKED");

    // Create the query
    let mut db_query = sqlx::query(&query).bind(JobStatus::Queued.to_string());

    // Bind tag parameters
    for tag in worker_tags {
        db_query = db_query.bind(tag);
    }

    let row = db_query
        .map(|row: PgRow| to_job(&row).ok())
        .fetch_optional(&mut *tx)
        .await?
        .flatten();

    if let Some(job) = row {
        debug!(job_id = %job.id, job_name = %job.name, job_tags = ?job.tags, job_priority = %job.priority, "Dequeueing job for processing");
        let status_after_start: String = sqlx::query(
            "UPDATE pg_loco_queue
             SET
                 status = CASE WHEN attempts_left <= 0 THEN $1 ELSE $2 END,
                 attempts_left = CASE WHEN attempts_left <= 0 THEN 0 ELSE attempts_left - 1 END,
                 updated_at = NOW()
             WHERE id = $3
             RETURNING status",
        )
            .bind(JobStatus::Failed.to_string())
            .bind(JobStatus::Processing.to_string())
            .bind(&job.id)
            .fetch_one(&mut *tx)
            .await?
            .get("status");

        if status_after_start == JobStatus::Failed.to_string() {
            debug!(
                job_id = %job.id,
                job_name = %job.name,
                "Job marked as failed because attempts_left is exhausted"
            );

            tx.commit().await?;
            return Ok(None);
        }

        tx.commit().await?;
        Ok(Some(job))
    } else {
        Ok(None)
    }
}

async fn complete_job(pool: &PgPool, id: &JobId, interval_ms: Option<i64>) -> Result<()> {
    let (status, run_at) = interval_ms.map_or_else(
        || (JobStatus::Completed.to_string(), Utc::now()),
        |interval_ms| {
            (
                JobStatus::Queued.to_string(),
                Utc::now() + chrono::Duration::milliseconds(interval_ms),
            )
        },
    );

    trace!(
        job_id = %id,
        status = %status,
        run_at = %run_at,
        "Marking job as completed"
    );

    sqlx::query(
        "UPDATE pg_loco_queue SET status = $1, updated_at = NOW(), run_at = $2 WHERE id = $3",
    )
    .bind(status)
    .bind(run_at)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

async fn fail_job(pool: &PgPool, id: &JobId, error: &crate::Error) -> Result<()> {
    let msg = error.to_string();
    debug!(job_id = %id, error = %msg, "Marking job as failed");
    let error_json = serde_json::json!({ "error": msg });
    sqlx::query(
        "UPDATE pg_loco_queue SET status = $1, updated_at = NOW(), task_data = task_data || \
         $2::jsonb WHERE id = $3",
    )
    .bind(JobStatus::Failed.to_string())
    .bind(error_json)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Cancels jobs in the `pg_loco_queue` table by their name.
///
/// This function updates the status of all jobs with the given `name` and a status of
/// [`JobStatus::Queued`] to [`JobStatus::Cancelled`]. The update also sets the `updated_at` timestamp to the
/// current time.
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn cancel_jobs_by_name(pool: &PgPool, name: &str) -> Result<()> {
    debug!(job_name = %name, "Cancelling queued jobs by name");
    sqlx::query(
        "UPDATE pg_loco_queue SET status = $1, updated_at = NOW() WHERE name = $2 AND status = $3",
    )
    .bind(JobStatus::Cancelled.to_string())
    .bind(name)
    .bind(JobStatus::Queued.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Clear all jobs
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn clear(pool: &PgPool) -> Result<()> {
    sqlx::query("DELETE FROM pg_loco_queue")
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes jobs from the `pg_loco_queue` table based on their status.
///
/// This function removes all jobs with a status that matches any of the statuses provided
/// in the `status` argument. The statuses are checked against the `status` column in the
/// database, and any matching rows are deleted.
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn clear_by_status(pool: &PgPool, status: Vec<JobStatus>) -> Result<()> {
    let status_in = status
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<String>>();

    debug!(status = ?status, "Clearing jobs by status");
    sqlx::query("DELETE FROM pg_loco_queue WHERE status = ANY($1)")
        .bind(status_in)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes jobs from the `pg_loco_queue` table that are older than a specified number of days.
///
/// This function removes jobs that have a `created_at` timestamp older than the provided
/// number of days. Additionally, if a `status` is provided, only jobs with a status matching
/// one of the provided values will be deleted.
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn clear_jobs_older_than(
    pool: &PgPool,
    age_days: i64,
    status: Option<&Vec<JobStatus>>,
) -> Result<()> {
    let mut query_builder = sqlx::query_builder::QueryBuilder::<sqlx::Postgres>::new(
        "DELETE FROM pg_loco_queue WHERE created_at < NOW() - INTERVAL '1 day' * ",
    );

    query_builder.push_bind(age_days);

    if let Some(status_list) = status {
        if !status_list.is_empty() {
            let status_in = status_list
                .iter()
                .map(|s| format!("'{s}'"))
                .collect::<Vec<String>>()
                .join(",");

            query_builder.push(format!(" AND status IN ({status_in})"));
        }
    }

    debug!(age_days = age_days, status = ?status, "Clearing older jobs");
    query_builder.build().execute(pool).await?;

    Ok(())
}

/// Requeues jobs from [`JobStatus::Processing`] to [`JobStatus::Queued`].
///
/// This function updates the status of all jobs that are currently in the [`JobStatus::Processing`] state
/// to the [`JobStatus::Queued`] state, provided they have been updated more than the specified age (`age_minutes`).
/// The jobs that meet the criteria will have their `updated_at` timestamp set to the current time.
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn requeue(pool: &PgPool, age_minutes: &i64) -> Result<()> {
    let interval = format!("{age_minutes} MINUTE");

    let query = format!(
        "UPDATE pg_loco_queue SET status = $1, updated_at = NOW() WHERE status = $2 AND updated_at <= NOW() - INTERVAL '{interval}'"
    );

    debug!(age_minutes = age_minutes, "Requeueing stalled jobs");
    sqlx::query(&query)
        .bind(JobStatus::Queued.to_string())
        .bind(JobStatus::Processing.to_string())
        .execute(pool)
        .await?;

    Ok(())
}

/// Ping system
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn ping(pool: &PgPool) -> Result<()> {
    trace!("Pinging job queue database");
    sqlx::query("SELECT id from pg_loco_queue LIMIT 1")
        .execute(pool)
        .await?;
    Ok(())
}

/// Retrieves a list of jobs from the `pg_loco_queue` table in the database.
///
/// This function queries the database for jobs, optionally filtering by their
/// `status`. If a status is provided, only jobs with statuses included in the
/// provided list will be fetched. If no status is provided, all jobs will be
/// returned.
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn get_jobs(
    pool: &PgPool,
    status: Option<&Vec<JobStatus>>,
    age_days: Option<i64>,
) -> Result<Vec<Job>, sqlx::Error> {
    let mut query = String::from("SELECT * FROM pg_loco_queue where true");

    if let Some(status) = status {
        let status_in = status
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<String>>()
            .join(",");
        let _ = write!(query, " AND status in ({status_in})");
    }

    if let Some(age_days) = age_days {
        let _ = write!(
            query,
            " AND created_at <= NOW() - INTERVAL '1 day' * {age_days}"
        );
    }

    debug!(status = ?status, age_days = ?age_days, "Retrieving jobs");
    let rows = sqlx::query(&query).fetch_all(pool).await?;
    let jobs = rows.iter().filter_map(|row| to_job(row).ok()).collect();
    debug!(job_count = rows.len(), "Retrieved jobs from database");
    Ok(jobs)
}

/// Converts a row from the database into a [`Job`] object.
///
/// This function takes a row from the `Postgres` database and manually extracts the necessary
/// fields to populate a [`Job`] object.
///
/// **Note:** This function manually extracts values from the database row instead of using
/// the `FromRow` trait, which would require enabling the 'macros' feature in the dependencies.
/// The decision to avoid `FromRow` is made to keep the build smaller and faster, as the 'macros'
/// feature is unnecessary in the current dependency tree.
fn to_job(row: &PgRow) -> Result<Job> {
    let tags_json: Option<serde_json::Value> = row.try_get("tags").unwrap_or_default();
    let tags = tags_json.and_then(|json_val| {
        if json_val.is_array() {
            let tags_vec: Vec<String> =
                serde_json::from_value(json_val).unwrap_or_else(|_| Vec::new());
            if tags_vec.is_empty() {
                None
            } else {
                Some(tags_vec)
            }
        } else {
            None
        }
    });

    Ok(Job {
        id: row.get("id"),
        name: row.get("name"),
        data: row.get("task_data"),
        status: row.get::<String, _>("status").parse().map_err(|err| {
            let status: String = row.get("status");
            tracing::error!(status, err = %err, "Unsupported job status in database");
            Error::string("invalid job status")
        })?,
        run_at: row.get("run_at"),
        interval: row.get("interval"),
        created_at: row.try_get("created_at").unwrap_or_default(),
        updated_at: row.try_get("updated_at").unwrap_or_default(),
        tags,
        priority: row.get("priority"),
    })
}

#[derive(Debug)]
pub struct RunOpts {
    pub num_workers: u32,
    pub poll_interval_sec: u32,
}

/// Create this provider
///
/// # Errors
///
/// This function will return an error if it fails
pub async fn create_provider(qcfg: &PostgresQueueConfig) -> Result<Queue> {
    debug!(
        num_workers = qcfg.num_workers,
        poll_interval = qcfg.poll_interval_sec,
        "Creating job queue provider"
    );
    let pool = connect(qcfg).await.map_err(Box::from)?;
    let registry = JobRegistry::new();
    let token = CancellationToken::new(); // Create the token
    Ok(Queue::Postgres(
        pool,
        Arc::new(tokio::sync::Mutex::new(registry)),
        RunOpts {
            num_workers: qcfg.num_workers,
            poll_interval_sec: qcfg.poll_interval_sec,
        },
        token, // Pass the token
    ))
}
