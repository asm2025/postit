//! What a running publish reports back, and what tells it to stop: the container reference it
//! saves under the delivery's fence, and the lease renewer that notices a lost lease or the
//! owner's cancel request.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::TimeDelta;
use postit_core::{AgentError, Clock, PublishProgress};
use postit_data::publishing::{DeliveryRepo, Fence, LeaseRenewal};
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::outcome::CancelCause;

/// The flags a running publish and its lease renewer share.
#[derive(Debug, Default)]
struct Flags {
    /// The lease is gone: the sweep or another worker owns the delivery now.
    lost: AtomicBool,
    /// The owner asked to stop the delivery.
    cancel: AtomicBool,
    /// A container reference was saved during this run.
    saved: AtomicBool,
}

/// The engine's [`PublishProgress`] for one `in_flight` delivery.
pub struct DeliveryProgress {
    pool: PgPool,
    fence: Fence,
    flags: Arc<Flags>,
    shutdown: CancellationToken,
}

impl DeliveryProgress {
    /// The progress of the delivery `fence` names (expected `in_flight`). `cancel_requested`
    /// is what the claim saw; `shutdown` is the server's.
    #[must_use]
    pub fn new(
        pool: PgPool,
        fence: Fence,
        cancel_requested: bool,
        shutdown: CancellationToken,
    ) -> Self {
        let flags = Flags::default();
        flags.cancel.store(cancel_requested, Ordering::SeqCst);
        Self {
            pool,
            fence,
            flags: Arc::new(flags),
            shutdown,
        }
    }

    /// Why the publish was asked to stop, if it was: a lost lease first (nothing may be written
    /// then), then the owner's request, then the server's shutdown.
    #[must_use]
    pub fn cause(&self) -> CancelCause {
        if self.flags.lost.load(Ordering::SeqCst) {
            CancelCause::LeaseLost
        } else if self.flags.cancel.load(Ordering::SeqCst) {
            CancelCause::User
        } else if self.shutdown.is_cancelled() {
            CancelCause::Shutdown
        } else {
            CancelCause::None
        }
    }

    /// Whether a container reference was saved during this run, meaning the platform already
    /// holds part of the upload.
    #[must_use]
    pub fn saved_container_ref(&self) -> bool {
        self.flags.saved.load(Ordering::SeqCst)
    }

    /// Starts renewing the lease every `lease / 3` until the returned guard is dropped. A
    /// renewal that finds the lease gone sets the lost flag; one that sees the owner's cancel
    /// request sets the cancel flag.
    #[must_use]
    pub fn renew_every_third(&self, lease: Duration, clock: Arc<dyn Clock>) -> Renewer {
        let pool = self.pool.clone();
        let fence = self.fence;
        let flags = Arc::clone(&self.flags);
        // `interval` refuses a zero period; settings never configure a lease that short.
        let period = (lease / 3).max(Duration::from_millis(10));
        let lease = TimeDelta::from_std(lease).unwrap_or(TimeDelta::MAX);
        let handle = tokio::spawn(async move {
            let mut ticks = tokio::time::interval(period);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // The first tick completes at once; the claim has just set the lease.
            ticks.tick().await;
            loop {
                ticks.tick().await;
                let renewal = match pool.acquire().await {
                    Ok(mut conn) => {
                        DeliveryRepo::renew_lease(
                            &mut conn,
                            fence.id,
                            fence.dispatch_gen,
                            clock.now(),
                            lease,
                        )
                        .await
                    }
                    Err(error) => Err(error.into()),
                };
                match renewal {
                    Ok(LeaseRenewal::Lost) => {
                        flags.lost.store(true, Ordering::SeqCst);
                        return;
                    }
                    Ok(LeaseRenewal::Held { cancel_requested }) => {
                        if cancel_requested {
                            flags.cancel.store(true, Ordering::SeqCst);
                        }
                    }
                    // The next tick tries again; the lease outlasts two missed renewals.
                    Err(_) => tracing::warn!(
                        delivery = %fence.id,
                        "could not renew a publishing lease"
                    ),
                }
            }
        });
        Renewer(handle)
    }
}

#[async_trait]
impl PublishProgress for DeliveryProgress {
    async fn save_container_ref(&self, container_ref: &str) -> Result<(), AgentError> {
        if self.flags.lost.load(Ordering::SeqCst) {
            return Err(AgentError::Cancelled);
        }
        let written = match self.pool.acquire().await {
            Ok(mut conn) => DeliveryRepo::set_container_ref(&mut conn, self.fence, container_ref)
                .await
                .map_err(|_| ()),
            Err(_) => Err(()),
        };
        match written {
            Ok(true) => {
                self.flags.saved.store(true, Ordering::SeqCst);
                Ok(())
            }
            // The fence no longer matches: someone else owns the delivery now.
            Ok(false) => {
                self.flags.lost.store(true, Ordering::SeqCst);
                Err(AgentError::Cancelled)
            }
            Err(()) => Err(AgentError::Transport {
                message: "the upload's progress could not be saved".to_owned(),
            }),
        }
    }

    fn cancel_requested(&self) -> bool {
        self.cause() != CancelCause::None
    }
}

/// Stops the lease renewer when dropped.
pub struct Renewer(JoinHandle<()>);

impl Drop for Renewer {
    fn drop(&mut self) {
        self.0.abort();
    }
}
