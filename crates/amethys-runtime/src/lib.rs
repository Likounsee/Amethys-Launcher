use amethys_errors::{AmethysError, ErrorCode};
use amethys_events::{CoreEvent, EventBus};

/// Lifecycle state of the core runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    Created,
    Running,
    Stopping,
    Stopped,
}

/// Owns the core lifecycle and its internal event bus.
pub struct CoreRuntime {
    state: RuntimeState,
    events: EventBus,
}

impl CoreRuntime {
    #[must_use]
    pub fn new(events: EventBus) -> Self {
        Self {
            state: RuntimeState::Created,
            events,
        }
    }

    #[must_use]
    pub const fn state(&self) -> RuntimeState {
        self.state
    }

    #[must_use]
    pub const fn events(&self) -> &EventBus {
        &self.events
    }

    pub fn start(&mut self) -> Result<(), AmethysError> {
        if self.state != RuntimeState::Created {
            return Err(AmethysError::new(
                ErrorCode::RuntimeInvalidTransition,
                format!("Cannot start runtime from {:?}.", self.state),
            ));
        }
        self.state = RuntimeState::Running;
        // No listeners during early startup is a normal condition.
        let _delivery_count = self.events.publish(CoreEvent::RuntimeStarted);
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), AmethysError> {
        if self.state != RuntimeState::Running {
            return Err(AmethysError::new(
                ErrorCode::RuntimeInvalidTransition,
                format!("Cannot stop runtime from {:?}.", self.state),
            ));
        }
        self.state = RuntimeState::Stopping;
        let _delivery_count = self.events.publish(CoreEvent::RuntimeStopping);
        self.state = RuntimeState::Stopped;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreRuntime, RuntimeState};
    use amethys_events::EventBus;

    #[test]
    fn runtime_lifecycle_is_enforced() {
        let mut runtime = CoreRuntime::new(EventBus::new(4));
        assert_eq!(runtime.state(), RuntimeState::Created);
        runtime.start().expect("start succeeds");
        assert_eq!(runtime.state(), RuntimeState::Running);
        runtime.shutdown().expect("shutdown succeeds");
        assert_eq!(runtime.state(), RuntimeState::Stopped);
        assert!(runtime.start().is_err());
    }
}
