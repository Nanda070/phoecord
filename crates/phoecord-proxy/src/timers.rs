//! The proxy's timer driver: a task that sleeps until the earliest plugin timer
//! (`Host::schedule`) is due and fires it through [`Proxy::fire_timer`]. The store of pending
//! timers is [`phoecord_transport::timers::Timers`].

use crate::Proxy;
use std::sync::{Arc, Weak};
use std::time::Instant;

pub use phoecord_transport::timers::*;

/// The driver: sleeps until the earliest timer is due (or until the store changes), then fires
/// what is due. Ends when the proxy is gone.
pub(crate) async fn drive(proxy: Weak<Proxy>, timers: Arc<Timers>) {
    loop {
        let changed = timers.changed();
        tokio::pin!(changed);
        // Take the permit of a change made before this point, so it is not missed.
        changed.as_mut().enable();
        match timers.next_due() {
            None => changed.await,
            Some(due) => {
                tokio::select! {
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(due)) => {}
                    _ = &mut changed => {}
                }
            }
        }
        let Some(proxy) = proxy.upgrade() else { return };
        for (plugin, tag) in timers.take_due(Instant::now()) {
            proxy.fire_timer(&plugin, &tag);
        }
    }
}
