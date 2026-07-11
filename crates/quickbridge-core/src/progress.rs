/// Receives structured progress events from domain workflows.
pub trait ProgressSink<Event> {
    type Error;

    fn on_event(&mut self, event: Event) -> Result<(), Self::Error>;

    fn on_tick(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Shared structured progress event model used across workflows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgressEvent<Step> {
    Started { step: Step, details: Vec<String> },
    Finished { step: Step },
    Warned { step: Step, details: Vec<String> },
}
