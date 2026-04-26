use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
#[cfg(feature = "cli")]
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_variant::to_variant_name;
#[cfg(feature = "bg_pg")]
pub mod pg;

use crate::{
    app::AppContext,
    config::{self, Config, PostgresQueueConfig, QueueConfig, WorkerMode},
    Error, Result,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(ValueEnum))]
pub enum JobStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "processing")]
    Processing,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl std::str::FromStr for JobStatus {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Self::Queued),
            "processing" => Ok(Self::Processing),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("Invalid status: {s}")),
        }
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        to_variant_name(self).expect("only enum supported").fmt(f)
    }
}

pub enum Queue {
    #[cfg(feature = "bg_pg")]
    Postgres(
        pg::PgPool,
        Arc<tokio::sync::Mutex<pg::JobRegistry>>,
        pg::RunOpts,
        tokio_util::sync::CancellationToken,
    ),
    None,
}

impl Queue {
    #[allow(unused_variables)]
    pub async fn enqueue<A: Serialize + Send + Sync>(
        &self,
        class: String,
        queue: Option<String>,
        args: A,
        tags: Option<Vec<String>>,
        priority: Option<i32>,
    ) -> Result<()> {
        tracing::debug!(worker = class, queue = ?queue, tags = ?tags, "Enqueuing background job");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => {
                pg::enqueue(
                    pool,
                    &class,
                    serde_json::to_value(args)?,
                    chrono::Utc::now(),
                    None,
                    tags,
                    priority,
                )
                .await
                .map_err(Box::from)?;
            }
            Self::None => {}
        }
        Ok(())
    }

    #[allow(unused_variables)]
    pub async fn register<
        A: Serialize + Send + Sync + 'static + for<'de> serde::Deserialize<'de>,
        W: BackgroundWorker<A> + 'static,
    >(
        &self,
        worker: W,
    ) -> Result<()> {
        tracing::info!(worker = W::class_name(), "Registering background worker");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(_, registry, _, _) => {
                let mut r = registry.lock().await;
                r.register_worker(W::class_name(), worker)?;
            }
            Self::None => {}
        }
        Ok(())
    }

    #[allow(unused_variables)]
    pub async fn run(&self, tags: Vec<String>) -> Result<()> {
        tracing::info!("Starting background job processing");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, registry, run_opts, token) => {
                let handles = registry
                    .lock()
                    .await
                    .run(pool, run_opts, &token.clone(), &tags);
                Self::process_worker_handles(handles).await?;
            }
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
            }
        }
        Ok(())
    }

    async fn process_worker_handles(handles: Vec<tokio::task::JoinHandle<()>>) -> Result<()> {
        let handle_count = handles.len();
        tracing::debug!(worker_count = handle_count, "Processing worker handles");

        for (index, handle) in handles.into_iter().enumerate() {
            if let Err(e) = handle.await {
                if e.is_cancelled() {
                    tracing::debug!(
                        worker_index = index,
                        "Worker task cancelled during shutdown"
                    );
                } else if e.is_panic() {
                    tracing::error!(worker_index = index, "Worker task panicked");
                    std::panic::resume_unwind(e.into_panic());
                } else {
                    tracing::error!(worker_index = index, error = ?e, "Worker task failed to join");
                    return Err(crate::Error::Worker(format!("Worker join error: {e}")));
                }
            }
        }

        tracing::info!(
            worker_count = handle_count,
            "All worker tasks finished successfully"
        );
        Ok(())
    }

    pub async fn setup(&self) -> Result<()> {
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => {
                pg::initialize_database(pool).await.map_err(Box::from)?;
            }
            Self::None => {}
        }
        Ok(())
    }

    pub async fn clear(&self) -> Result<()> {
        tracing::info!("Clearing all jobs from queue");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => {
                pg::clear(pool).await.map_err(Box::from)?;
            }
            Self::None => {}
        }
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        tracing::trace!("Pinging job queue");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => {
                pg::ping(pool).await.map_err(Box::from)?;
            }
            Self::None => {}
        }
        Ok(())
    }

    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(_, _, _, _) => "postgres queue".to_string(),
            Self::None => "no queue".to_string(),
        }
    }

    pub fn shutdown(&self) -> Result<()> {
        tracing::info!("Shutting down background job processing");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(_, _, _, cancellation_token) => cancellation_token.cancel(),
            Self::None => {}
        }
        Ok(())
    }

    async fn get_jobs(
        &self,
        status: Option<&Vec<JobStatus>>,
        age_days: Option<i64>,
    ) -> Result<serde_json::Value> {
        tracing::info!(status = ?status, age_days = ?age_days, "Retrieving jobs");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => {
                let jobs = pg::get_jobs(pool, status, age_days)
                    .await
                    .map_err(Box::from)?;
                Ok(serde_json::to_value(jobs)?)
            }
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
                Err(Error::string("provider not configured"))
            }
        }
    }

    pub async fn cancel_jobs(&self, job_name: &str) -> Result<()> {
        tracing::info!(job_name = job_name, "Cancelling jobs by name");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => pg::cancel_jobs_by_name(pool, job_name).await,
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
                Err(Error::string("provider not configured"))
            }
        }
    }

    pub async fn clear_jobs_older_than(&self, age_days: i64, status: &Vec<JobStatus>) -> Result<()> {
        tracing::info!(age_days = age_days, status = ?status, "Clearing older jobs");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => {
                pg::clear_jobs_older_than(pool, age_days, Some(status)).await
            }
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
                Err(Error::string("provider not configured"))
            }
        }
    }

    pub async fn clear_by_status(&self, status: Vec<JobStatus>) -> Result<()> {
        tracing::info!(status = ?status, "Clearing jobs by status");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => pg::clear_by_status(pool, status).await,
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
                Err(Error::string("provider not configured"))
            }
        }
    }

    pub async fn requeue(&self, age_minutes: &i64) -> Result<()> {
        tracing::info!(age_minutes = age_minutes, "Requeuing stale jobs");
        match self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(pool, _, _, _) => pg::requeue(pool, age_minutes).await,
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
                Err(Error::string("provider not configured"))
            }
        }
    }

    pub async fn dump(
        &self,
        path: &Path,
        status: Option<&Vec<JobStatus>>,
        age_days: Option<i64>,
    ) -> Result<PathBuf> {
        tracing::info!(path = %path.display(), status = ?status, age_days = ?age_days, "Dumping jobs to file");

        if !path.exists() {
            tracing::debug!(path = %path.display(), "Directory does not exist, creating...");
            std::fs::create_dir_all(path)?;
        }

        let dump_file = path.join(format!(
            "loco-dump-jobs-{}.yaml",
            chrono::Utc::now().format("%Y-%m-%d-%H-%M-%S")
        ));

        let jobs = self.get_jobs(status, age_days).await?;

        let data = serde_yaml::to_string(&jobs)?;
        let mut file = File::create(&dump_file)?;
        file.write_all(data.as_bytes())?;

        tracing::info!(file = %dump_file.display(), "Jobs successfully dumped to file");
        Ok(dump_file)
    }

    pub async fn import(&self, path: &Path) -> Result<()> {
        tracing::info!(path = %path.display(), "Importing jobs from file");

        match &self {
            #[cfg(feature = "bg_pg")]
            Self::Postgres(_, _, _, _) => {
                let jobs: Vec<pg::Job> = serde_yaml::from_reader(File::open(path)?)?;
                for job in jobs {
                    self.enqueue(job.name.clone(), None, job.data, None, Some(job.priority))
                        .await?;
                }
                Ok(())
            }
            Self::None => {
                tracing::error!(
                    "No queue provider is configured: compile with at least one queue provider feature"
                );
                Err(Error::string("provider not configured"))
            }
        }
    }
}

#[async_trait]
pub trait BackgroundWorker<A: Send + Sync + serde::Serialize + 'static>: Send + Sync {
    #[must_use]
    fn queue() -> Option<String> {
        None
    }

    #[must_use]
    fn tags() -> Vec<String> {
        Vec::new()
    }

    fn build(ctx: &AppContext) -> Self;

    #[must_use]
    fn class_name() -> String
    where
        Self: Sized,
    {
        use heck::ToUpperCamelCase;
        let type_name = std::any::type_name::<Self>();
        let name = type_name.split("::").last().unwrap_or(type_name);
        name.to_upper_camel_case()
    }

    async fn perform_later(ctx: &AppContext, args: A) -> crate::Result<()>
    where
        Self: Sized,
    {
        Self::perform_later_with_priority(ctx, args, None).await
    }

    async fn perform_later_with_priority(
        ctx: &AppContext,
        args: A,
        priority: Option<i32>,
    ) -> crate::Result<()>
    where
        Self: Sized,
    {
        match &ctx.config.workers.mode {
            WorkerMode::BackgroundQueue => {
                if let Some(p) = &ctx.queue_provider {
                    let tags = Self::tags();
                    let tags_option = if tags.is_empty() { None } else { Some(tags) };
                    p.enqueue(
                        Self::class_name(),
                        Self::queue(),
                        args,
                        tags_option,
                        priority,
                    )
                    .await?;
                } else {
                    tracing::error!(
                        "perform_later: background queue is selected, but queue was not populated \
                         in context"
                    );
                }
            }
            WorkerMode::ForegroundBlocking => {
                Self::build(ctx).perform(args).await?;
            }
            WorkerMode::BackgroundAsync => {
                let dx = ctx.clone();
                tokio::spawn(async move {
                    if let Err(err) = Self::build(&dx).perform(args).await {
                        tracing::error!(err = err.to_string(), "worker failed to perform job");
                    }
                });
            }
        }
        Ok(())
    }

    async fn perform(&self, args: A) -> crate::Result<()>;
}

pub async fn converge(queue: &Queue, config: &QueueConfig) -> Result<()> {
    queue.setup().await?;
    let QueueConfig::Postgres(PostgresQueueConfig {
        dangerously_flush,
        uri: _,
        max_connections: _,
        enable_logging: _,
        connect_timeout: _,
        idle_timeout: _,
        poll_interval_sec: _,
        num_workers: _,
        min_connections: _,
    }) = config;

    if *dangerously_flush {
        tracing::warn!("Flush mode enabled - clearing all jobs from queue");
        queue.clear().await?;
    }

    Ok(())
}

#[allow(clippy::missing_panics_doc)]
pub async fn create_queue_provider(config: &Config) -> Result<Option<Arc<Queue>>> {
    if config.workers.mode == config::WorkerMode::BackgroundQueue {
        if let Some(queue) = &config.queue {
            match queue {
                #[cfg(feature = "bg_pg")]
                config::QueueConfig::Postgres(qcfg) => {
                    tracing::debug!("Creating Postgres queue provider");
                    Ok(Some(Arc::new(pg::create_provider(qcfg).await?)))
                }

                #[allow(unreachable_patterns)]
                _ => Err(Error::string(
                    "No queue provider feature was selected and compiled, but queue configuration \
                     is present",
                )),
            }
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}
