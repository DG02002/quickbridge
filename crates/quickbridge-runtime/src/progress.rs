use std::future::Future;

use quickbridge_core::ProgressSink;
use tokio::time::{Duration, MissedTickBehavior, interval};

pub async fn spin_with_ticks<T, Event, E, F, S>(sink: &mut S, future: F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
    S: ProgressSink<Event, Error = E>,
{
    let mut ticker = interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tokio::pin!(future);

    loop {
        tokio::select! {
            result = &mut future => return result,
            _ = ticker.tick() => sink.on_tick()?,
        }
    }
}
