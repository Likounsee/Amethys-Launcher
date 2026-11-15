use amethys_errors::{AmethysError, ErrorCode};
use amethys_events::{CoreEvent, EventBus};
use std::{
    collections::HashMap,
    future::Future,
    time::Duration,
};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

const DEFAULT_WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Tracks modules known to the core.
#[derive(Default)]
pub struct ModuleManager {
    modules: HashMap<String, bool>,
}

impl ModuleManager {
    /// Registers a module in the disabled state.
    pub fn register(&mut self, name: impl Into<String>) -> Result<(), AmethysError> {
        let name = name.into();
        if let std::collections::hash_map::Entry::Vacant(entry) = self.modules.entry(name) {
            entry.insert(false);
            Ok(())
        } else {
            let name = self.modules.keys().last().map_or("unknown", String::as_str);
            Err(AmethysError::new(
                ErrorCode::ModuleAlreadyRegistered,
                format!("Module '{name}' is already registered."),
            ))
        }
    }

    /// Enables a registered module.
    pub fn enable(&mut self, name: &str) -> Result<(), AmethysError> {
        let state = self.modules.get_mut(name).ok_or_else(|| {
            AmethysError::new(
                ErrorCode::ModuleNotFound,
                format!("Module '{name}' is not registered."),
            )
        })?;
        *state = true;
        Ok(())
    }

    #[must_use]
    pub fn is_enabled(&self, name: &str) -> bool {
        self.modules.get(name).copied().unwrap_or(false)
    }
}

/// Tracks asynchronous core tasks.
#[derive(Default)]
pub struct TaskManager {
    tasks: HashMap<String, JoinHandle<()>>,
    events: Option<EventBus>,
}

impl TaskManager {
    #[must_use]
    pub fn new(events: EventBus) -> Self {
        Self {
            tasks: HashMap::new(),
            events: Some(events),
        }
    }

    pub fn spawn<F>(&mut self, id: impl Into<String>, future: F) -> Result<(), AmethysError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = id.into();
        if self.tasks.contains_key(&id) {
            return Err(AmethysError::new(
                ErrorCode::TaskAlreadyRunning,
                format!("Task '{id}' is already registered."),
            ));
        }
        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::TaskStarted { id: id.clone() });
        }
        self.tasks.insert(id, tokio::spawn(future));
        Ok(())
    }

    pub async fn wait(&mut self, id: &str) -> Result<(), AmethysError> {
        let handle = self.tasks.remove(id).ok_or_else(|| {
            AmethysError::new(
                ErrorCode::TaskNotFound,
                format!("Task '{id}' is not registered."),
            )
        })?;
        handle.await.map_err(|error| {
            AmethysError::new(
                ErrorCode::WorkerFailed,
                format!("Task '{id}' did not complete cleanly: {error}"),
            )
        })?;
        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::TaskFinished { id: id.to_owned() });
        }
        Ok(())
    }
}

struct Worker {
    shutdown: oneshot::Sender<()>,
    join: JoinHandle<()>,
}

/// Owns asynchronous workers and bounds how long graceful shutdown may take.
pub struct WorkerManager {
    workers: HashMap<String, Worker>,
    events: Option<EventBus>,
    shutdown_timeout: Duration,
}

impl Default for WorkerManager {
    fn default() -> Self {
        Self {
            workers: HashMap::new(),
            events: None,
            shutdown_timeout: DEFAULT_WORKER_SHUTDOWN_TIMEOUT,
        }
    }
}

impl WorkerManager {
    #[must_use]
    pub fn new(events: EventBus) -> Self {
        Self {
            workers: HashMap::new(),
            events: Some(events),
            shutdown_timeout: DEFAULT_WORKER_SHUTDOWN_TIMEOUT,
        }
    }

    /// Creates a manager with an explicit graceful-shutdown timeout.
    #[must_use]
    pub fn with_shutdown_timeout(events: EventBus, shutdown_timeout: Duration) -> Self {
        Self {
            workers: HashMap::new(),
            events: Some(events),
            shutdown_timeout,
        }
    }

    pub fn start<F, Fut>(&mut self, name: impl Into<String>, run: F) -> Result<(), AmethysError>
    where
        F: FnOnce(oneshot::Receiver<()>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let name = name.into();
        if self.workers.contains_key(&name) {
            return Err(AmethysError::new(
                ErrorCode::WorkerAlreadyRunning,
                format!("Worker '{name}' is already registered."),
            ));
        }
        let (shutdown, receiver) = oneshot::channel();
        let join = tokio::spawn(run(receiver));
        self.workers
            .insert(name.clone(), Worker { shutdown, join });
        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::WorkerStarted { name });
        }
        Ok(())
    }

    pub async fn stop(&mut self, name: &str) -> Result<(), AmethysError> {
        let worker = self.workers.remove(name).ok_or_else(|| {
            AmethysError::new(
                ErrorCode::WorkerNotFound,
                format!("Worker '{name}' is not registered."),
            )
        })?;
        let _signal_result = worker.shutdown.send(());
        let mut join = worker.join;

        match timeout(self.shutdown_timeout, &mut join).await {
            Ok(result) => {
                result.map_err(|error| {
                    AmethysError::new(
                        ErrorCode::WorkerFailed,
                        format!("Worker '{name}' failed while stopping: {error}"),
                    )
                })?;
            }
            Err(_) => {
                join.abort();
                let _abort_result = join.await;
                return Err(AmethysError::new(
                    ErrorCode::WorkerShutdownTimeout,
                    format!(
                        "Worker '{name}' did not stop within {:?} and was aborted.",
                        self.shutdown_timeout
                    ),
                ));
            }
        }

        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::WorkerStopped {
                name: name.to_owned(),
            });
        }
        Ok(())
    }

    pub async fn stop_all(&mut self) -> Result<(), AmethysError> {
        let names: Vec<String> = self.workers.keys().cloned().collect();
        let mut first_error = None;
        for name in names {
            if let Err(error) = self.stop(&name).await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ModuleManager, WorkerManager};
    use amethys_errors::ErrorCode;
    use amethys_events::EventBus;
    use std::time::Duration;

    #[test]
    fn module_registration_and_activation_work() {
        let mut manager = ModuleManager::default();
        manager.register("steam").expect("register");
        manager.enable("steam").expect("enable");
        assert!(manager.is_enabled("steam"));
        assert!(manager.register("steam").is_err());
    }

    #[tokio::test]
    async fn worker_can_stop_gracefully() {
        let mut manager = WorkerManager::new(EventBus::new(8));
        manager
            .start("idle", |shutdown| async move {
                let _ = shutdown.await;
            })
            .expect("start");
        manager.stop("idle").await.expect("stop");
    }

    #[tokio::test]
    async fn unresponsive_worker_is_aborted_after_timeout() {
        let mut manager = WorkerManager::with_shutdown_timeout(
            EventBus::new(8),
            Duration::from_millis(10),
        );
        manager
            .start("stuck", |shutdown| async move {
                let _ = shutdown.await;
                std::future::pending::<()>().await;
            })
            .expect("start");

        let error = manager.stop("stuck").await.expect_err("worker must time out");
        assert_eq!(error.code(), ErrorCode::WorkerShutdownTimeout);
    }

    #[tokio::test]
    async fn stop_all_attempts_remaining_workers_after_an_error() {
        let mut manager = WorkerManager::with_shutdown_timeout(
            EventBus::new(8),
            Duration::from_millis(10),
        );
        manager
            .start("stuck", |shutdown| async move {
                let _ = shutdown.await;
                std::future::pending::<()>().await;
            })
            .expect("start");
        manager
            .start("idle", |shutdown| async move {
                let _ = shutdown.await;
            })
            .expect("start");

        let error = manager.stop_all().await.expect_err("stuck worker must time out");
        assert_eq!(error.code(), ErrorCode::WorkerShutdownTimeout);
        assert!(manager.workers.is_empty());
    }
}
