use ametis_errors::{AmetisError, ErrorCode};
use ametis_events::{CoreEvent, EventBus};
use std::{collections::HashMap, future::Future};
use tokio::{sync::oneshot, task::JoinHandle};

/// Tracks modules known to the core.
#[derive(Default)]
pub struct ModuleManager {
    modules: HashMap<String, bool>,
}

impl ModuleManager {
    /// Registers a module in the disabled state.
    pub fn register(&mut self, name: impl Into<String>) -> Result<(), AmetisError> {
        let name = name.into();
        if self.modules.contains_key(&name) {
            return Err(AmetisError::new(ErrorCode::ModuleAlreadyRegistered, format!("Module '{name}' is already registered.")));
        }
        self.modules.insert(name, false);
        Ok(())
    }

    /// Enables a registered module.
    pub fn enable(&mut self, name: &str) -> Result<(), AmetisError> {
        let state = self.modules.get_mut(name).ok_or_else(||
            AmetisError::new(ErrorCode::ModuleNotFound, format!("Module '{name}' is not registered."))
        )?;
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
        Self { tasks: HashMap::new(), events: Some(events) }
    }

    pub fn spawn<F>(&mut self, id: impl Into<String>, future: F) -> Result<(), AmetisError>
    where F: Future<Output = ()> + Send + 'static {
        let id = id.into();
        if self.tasks.contains_key(&id) {
            return Err(AmetisError::new(ErrorCode::TaskAlreadyRunning, format!("Task '{id}' is already registered.")));
        }
        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::TaskStarted { id: id.clone() });
        }
        self.tasks.insert(id, tokio::spawn(future));
        Ok(())
    }

    pub async fn wait(&mut self, id: &str) -> Result<(), AmetisError> {
        let handle = self.tasks.remove(id).ok_or_else(||
            AmetisError::new(ErrorCode::TaskNotFound, format!("Task '{id}' is not registered."))
        )?;
        handle.await.map_err(|error| AmetisError::new(
            ErrorCode::WorkerFailed,
            format!("Task '{id}' did not complete cleanly: {error}"),
        ))?;
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

/// Owns dynamically started asynchronous workers with graceful shutdown signals.
#[derive(Default)]
pub struct WorkerManager {
    workers: HashMap<String, Worker>,
    events: Option<EventBus>,
}

impl WorkerManager {
    #[must_use]
    pub fn new(events: EventBus) -> Self {
        Self { workers: HashMap::new(), events: Some(events) }
    }

    pub fn start<F, Fut>(&mut self, name: impl Into<String>, run: F) -> Result<(), AmetisError>
    where F: FnOnce(oneshot::Receiver<()>) -> Fut, Fut: Future<Output = ()> + Send + 'static {
        let name = name.into();
        if self.workers.contains_key(&name) {
            return Err(AmetisError::new(ErrorCode::WorkerAlreadyRunning, format!("Worker '{name}' is already registered.")));
        }
        let (shutdown, receiver) = oneshot::channel();
        let join = tokio::spawn(run(receiver));
        self.workers.insert(name.clone(), Worker { shutdown, join });
        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::WorkerStarted { name });
        }
        Ok(())
    }

    pub async fn stop(&mut self, name: &str) -> Result<(), AmetisError> {
        let worker = self.workers.remove(name).ok_or_else(||
            AmetisError::new(ErrorCode::WorkerNotFound, format!("Worker '{name}' is not registered."))
        )?;
        let _signal_result = worker.shutdown.send(());
        worker.join.await.map_err(|error| AmetisError::new(
            ErrorCode::WorkerFailed,
            format!("Worker '{name}' failed while stopping: {error}"),
        ))?;
        if let Some(events) = &self.events {
            let _delivery_count = events.publish(CoreEvent::WorkerStopped { name: name.to_owned() });
        }
        Ok(())
    }

    pub async fn stop_all(&mut self) -> Result<(), AmetisError> {
        let names: Vec<String> = self.workers.keys().cloned().collect();
        for name in names {
            self.stop(&name).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ModuleManager, WorkerManager};
    use ametis_events::EventBus;

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
        manager.start("idle", |shutdown| async move { let _ = shutdown.await; }).expect("start");
        manager.stop("idle").await.expect("stop");
    }
}
