use amethys_errors::{AmethysError, ErrorCode};
use tokio::sync::broadcast;

/// Events emitted by the core and its domain services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreEvent {
    RuntimeStarted,
    RuntimeStopping,
    ModuleRegistered { name: String },
    TaskStarted { id: String },
    TaskFinished { id: String },
    WorkerStarted { name: String },
    WorkerStopped { name: String },
    ErrorReported { code: String },
}

/// Cloneable bounded publisher/subscriber for core events.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<CoreEvent>,
}

impl EventBus {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity.max(1));
        Self { sender }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.sender.subscribe()
    }

    /// Publishes an event and returns the number of active receivers.
    pub fn publish(&self, event: CoreEvent) -> Result<usize, AmethysError> {
        self.sender.send(event).map_err(|_| {
            AmethysError::new(
                ErrorCode::EventNoSubscribers,
                "The event had no active subscribers.",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreEvent, EventBus};

    #[tokio::test]
    async fn subscribers_receive_events() {
        let bus = EventBus::new(8);
        let mut receiver = bus.subscribe();
        bus.publish(CoreEvent::RuntimeStarted)
            .expect("subscriber exists");
        assert_eq!(
            receiver.recv().await.expect("event arrives"),
            CoreEvent::RuntimeStarted
        );
    }
}
