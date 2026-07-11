use crate::{Result, UiError};
use crossterm::event::{Event, EventStream, KeyEvent};
use futures_util::StreamExt;
use std::{future::Future, pin::Pin, time::Duration};
use tokio::signal::ctrl_c;
use tokio::time::{MissedTickBehavior, interval};

#[derive(Debug)]
pub enum AppEvent {
    Tick,
    Key(KeyEvent),
    Paste(String),
    Resize,
    CtrlC,
}

pub struct AppEventStream {
    events: EventStream,
    ticker: tokio::time::Interval,
    ctrl_c_signal: Pin<Box<dyn Future<Output = std::io::Result<()>> + Send>>,
}

impl AppEventStream {
    pub fn new(tick_rate: Duration) -> Self {
        let mut ticker = interval(tick_rate);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        Self {
            events: EventStream::new(),
            ticker,
            ctrl_c_signal: Box::pin(ctrl_c()),
        }
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        tokio::select! {
            _ = self.ticker.tick() => Ok(AppEvent::Tick),
            signal_result = &mut self.ctrl_c_signal => {
                signal_result.map_err(|source| UiError::Terminal {
                    action: "listen for Ctrl+C",
                    source,
                })?;
                Ok(AppEvent::CtrlC)
            }
            maybe_event = self.events.next() => {
                let maybe_event: Option<std::result::Result<Event, std::io::Error>> = maybe_event;
                let Some(event) = maybe_event.transpose().map_err(|source| UiError::Terminal {
                    action: "read terminal input",
                    source,
                })? else {
                    return Ok(AppEvent::CtrlC);
                };

                match event {
                    Event::Key(key) => Ok(AppEvent::Key(key)),
                    Event::Paste(text) => Ok(AppEvent::Paste(text)),
                    Event::Resize(_, _) => Ok(AppEvent::Resize),
                    _ => Ok(AppEvent::Tick),
                }
            }
        }
    }
}
