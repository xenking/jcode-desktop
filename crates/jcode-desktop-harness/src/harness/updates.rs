//! Ordered update delivery from harness threads to the UI: lossless FIFO, with
//! replaceable runtime snapshots.

use std::collections::HashMap;

use jcode_sdk::ApiEvent;

use crate::harness::protocol::Update;

/// The panel session whose identity snapshot this update replaces wholesale.
pub(super) fn runtime_info_session(update: &Update) -> Option<&str> {
    match update {
        Update::Event {
            session_id,
            event: ApiEvent::RuntimeInfo { .. },
        } => Some(session_id),
        _ => None,
    }
}

/// Ordinary updates are lossless FIFO. Identity snapshots are replaceable, so
/// retain only the newest pending catalog per session, before the UI drains.
#[derive(Default)]
pub(super) struct UpdateQueue {
    pending: std::collections::BTreeMap<u64, Update>,
    runtime: HashMap<String, u64>,
    sequence: u64,
    receivers: usize,
}

#[derive(Clone)]
pub(super) struct UpdateSender {
    queue: std::sync::Arc<std::sync::Mutex<UpdateQueue>>,
    wake: async_channel::Sender<()>,
}

pub(super) struct UpdateReceiver {
    queue: std::sync::Arc<std::sync::Mutex<UpdateQueue>>,
    wake: async_channel::Receiver<()>,
    notify: async_channel::WeakSender<()>,
}

pub(super) fn update_channel() -> (UpdateSender, UpdateReceiver) {
    let queue = std::sync::Arc::new(std::sync::Mutex::new(UpdateQueue {
        receivers: 1,
        ..UpdateQueue::default()
    }));
    // Only the wakeup is bounded. Blocking a worker on UI consumption would
    // strand Cancel/Stop and move the backlog into the SDK event subscriber.
    let (wake_tx, wake_rx) = async_channel::bounded(1);
    let notify = wake_tx.downgrade();
    (
        UpdateSender {
            queue: queue.clone(),
            wake: wake_tx,
        },
        UpdateReceiver {
            queue,
            wake: wake_rx,
            notify,
        },
    )
}

impl UpdateSender {
    #[allow(clippy::result_large_err)]
    pub(super) fn send(&self, update: Update) -> Result<(), async_channel::SendError<Update>> {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        if self.wake.is_closed() {
            return Err(async_channel::SendError(update));
        }
        let sequence = queue.sequence;
        queue.sequence = sequence.checked_add(1).expect("update sequence exhausted");
        if let Some(session) = runtime_info_session(&update)
            && let Some(previous) = queue.runtime.insert(session.to_owned(), sequence)
        {
            queue.pending.remove(&previous);
        }
        // Replacements take their new chronological position, not the old
        // snapshot's position ahead of intervening deltas and lifecycle events.
        queue.pending.insert(sequence, update);
        drop(queue);
        let _ = self.wake.try_send(());
        Ok(())
    }

    #[cfg(test)]
    #[allow(clippy::result_large_err)]
    pub(super) fn try_send(&self, update: Update) -> Result<(), async_channel::SendError<Update>> {
        self.send(update)
    }
}

impl UpdateReceiver {
    pub(super) fn try_recv(&self) -> Result<Update, async_channel::TryRecvError> {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((sequence, update)) = queue.pending.pop_first() {
            if let Some(session) = runtime_info_session(&update)
                && queue.runtime.get(session) == Some(&sequence)
            {
                queue.runtime.remove(session);
            }
            let more = !queue.pending.is_empty();
            drop(queue);
            if more && let Some(notify) = self.notify.upgrade() {
                // A capacity-one signal may wake only one of several waiting
                // receivers. Hand the wakeup on while updates remain queued.
                let _ = notify.try_send(());
            }
            return Ok(update);
        }
        if self.wake.is_closed() {
            Err(async_channel::TryRecvError::Closed)
        } else {
            Err(async_channel::TryRecvError::Empty)
        }
    }

    pub(super) async fn recv(&self) -> Result<Update, async_channel::RecvError> {
        loop {
            match self.try_recv() {
                Ok(update) => return Ok(update),
                Err(async_channel::TryRecvError::Closed) => return Err(async_channel::RecvError),
                Err(async_channel::TryRecvError::Empty) => {}
            }
            // Recheck the queue after closure too: the final sender can enqueue
            // between the empty check and this wait, then drop its wake handle.
            let _ = self.wake.recv().await;
        }
    }

    #[cfg(test)]
    pub(super) fn recv_blocking(&self) -> Result<Update, async_channel::RecvError> {
        loop {
            match self.try_recv() {
                Ok(update) => return Ok(update),
                Err(async_channel::TryRecvError::Closed) => return Err(async_channel::RecvError),
                Err(async_channel::TryRecvError::Empty) => {}
            }
            let _ = self.wake.recv_blocking();
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.queue.lock().unwrap().pending.len()
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Clone for UpdateReceiver {
    fn clone(&self) -> Self {
        self.queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .receivers += 1;
        Self {
            queue: self.queue.clone(),
            wake: self.wake.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl Drop for UpdateReceiver {
    fn drop(&mut self) {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        queue.receivers -= 1;
        if queue.receivers == 0 {
            // Release histories/catalogs even if a worker keeps its sender
            // during a slow network request after the last UI receiver drops.
            self.wake.close();
            queue.pending.clear();
            queue.runtime.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::protocol::Update;
    use jcode_sdk::ApiEvent;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    fn runtime_update(session: &str, model: String) -> Update {
        Update::Event {
            session_id: session.into(),
            event: ApiEvent::RuntimeInfo {
                speed_mode: Default::default(),
                speed_options: Vec::new(),
                session_id: session.into(),
                provider: None,
                model: Some(model),
                reasoning_effort: None,
                auth_method: None,
                account_label: None,
                routes: Vec::new(),
            },
        }
    }

    #[test]
    fn runtime_mailbox_bounds_payloads_before_any_consumer_runs() {
        let (sender, receiver) = update_channel();
        let (finished, done) = channel();
        let worker = std::thread::spawn(move || {
            for index in 0..2000 {
                for session in ["a", "b"] {
                    sender
                        .send(runtime_update(
                            session,
                            format!("{index}:{}", "x".repeat(16_384)),
                        ))
                        .unwrap();
                }
            }
            finished.send(sender).unwrap();
        });
        // No receiver is polled until the producer has finished: bounded
        // send_blocking would deadlock here, consumer-only coalescing retains
        // about 64 MiB of model strings instead of two final snapshots.
        let sender = done
            .recv_timeout(Duration::from_secs(5))
            .expect("producer must not wait for UI");
        worker.join().unwrap();
        assert_eq!(receiver.len(), 2);
        assert_eq!(sender.queue.lock().unwrap().runtime.len(), 2);
        let retained_model_bytes: usize = sender
            .queue
            .lock()
            .unwrap()
            .pending
            .values()
            .map(|update| match update {
                Update::Event {
                    event:
                        ApiEvent::RuntimeInfo {
                            model: Some(model), ..
                        },
                    ..
                } => model.len(),
                _ => 0,
            })
            .sum();
        // The old unbounded channel retained all 65,553,780 model bytes from
        // this workload. This is payload accounting, not a process-RSS claim.
        assert_eq!(retained_model_bytes, 32_778);
        for session in ["a", "b"] {
            let update = receiver.try_recv().unwrap();
            assert_eq!(runtime_info_session(&update), Some(session));
            assert!(
                matches!(update, Update::Event { event: ApiEvent::RuntimeInfo { model: Some(model), .. }, .. } if model.starts_with("1999:"))
            );
        }
        assert!(sender.queue.lock().unwrap().runtime.is_empty());
        sender.send(runtime_update("a", "new".into())).unwrap();
        assert_eq!(receiver.len(), 1, "draining clears the replacement index");
    }

    #[test]
    fn runtime_mailbox_preserves_delta_and_lifecycle_order() {
        let (sender, receiver) = update_channel();
        sender.send(runtime_update("a", "old-a".into())).unwrap();
        sender.send(Update::Status("delta-1".into())).unwrap();
        sender.send(runtime_update("b", "old-b".into())).unwrap();
        sender.send(Update::Status("delta-2".into())).unwrap();
        sender.send(runtime_update("a", "new-a".into())).unwrap();
        sender
            .send(Update::SessionLost {
                session_id: "a".into(),
                reason: "closed".into(),
            })
            .unwrap();
        sender.send(runtime_update("b", "new-b".into())).unwrap();
        let mut seen = Vec::new();
        while let Ok(update) = receiver.try_recv() {
            seen.push(match update {
                Update::Status(value) => value,
                Update::SessionLost { .. } => "lost-a".into(),
                Update::Event {
                    event:
                        ApiEvent::RuntimeInfo {
                            model: Some(model), ..
                        },
                    ..
                } => model,
                _ => panic!("unexpected update"),
            });
        }
        assert_eq!(seen, ["delta-1", "delta-2", "new-a", "lost-a", "new-b"]);
    }

    #[test]
    fn update_mailbox_wakes_async_receiver_and_drains_after_sender_close() {
        use std::future::Future;
        use std::task::{Context, Poll, Wake, Waker};
        struct WakeCount(std::sync::atomic::AtomicUsize);
        impl Wake for WakeCount {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let (sender, receiver) = update_channel();
        let count = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut context = Context::from_waker(&waker);
        let mut receive = std::pin::pin!(receiver.recv());
        assert!(receive.as_mut().poll(&mut context).is_pending());
        sender.send(Update::Status("first".into())).unwrap();
        assert!(count.0.load(std::sync::atomic::Ordering::Relaxed) > 0);
        assert!(
            matches!(receive.as_mut().poll(&mut context), Poll::Ready(Ok(Update::Status(value))) if value == "first")
        );
        sender.send(Update::Status("last".into())).unwrap();
        drop(sender);
        assert!(matches!(receiver.recv_blocking(), Ok(Update::Status(value)) if value == "last"));
        assert!(receiver.recv_blocking().is_err());
    }

    #[test]
    fn update_mailbox_hands_wakeup_to_other_waiting_receiver() {
        use std::future::Future;
        use std::task::{Context, Wake, Waker};
        struct WakeCount(std::sync::atomic::AtomicUsize);
        impl Wake for WakeCount {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let (sender, receiver) = update_channel();
        let other = receiver.clone();
        let a = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        let b = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        let wa = Waker::from(a.clone());
        let wb = Waker::from(b.clone());
        let mut ca = Context::from_waker(&wa);
        let mut cb = Context::from_waker(&wb);
        let mut fa = std::pin::pin!(receiver.recv());
        let mut fb = std::pin::pin!(other.recv());
        assert!(fa.as_mut().poll(&mut ca).is_pending());
        assert!(fb.as_mut().poll(&mut cb).is_pending());
        sender.send(Update::Status("one".into())).unwrap();
        sender.send(Update::Status("two".into())).unwrap();
        if a.0.load(std::sync::atomic::Ordering::Relaxed) > 0 {
            assert!(fa.as_mut().poll(&mut ca).is_ready());
            assert!(b.0.load(std::sync::atomic::Ordering::Relaxed) > 0);
            assert!(fb.as_mut().poll(&mut cb).is_ready());
        } else {
            assert!(b.0.load(std::sync::atomic::Ordering::Relaxed) > 0);
            assert!(fb.as_mut().poll(&mut cb).is_ready());
            assert!(a.0.load(std::sync::atomic::Ordering::Relaxed) > 0);
            assert!(fa.as_mut().poll(&mut ca).is_ready());
        }
    }

    #[test]
    fn update_mailbox_last_receiver_releases_payloads_with_live_sender() {
        let (sender, receiver) = update_channel();
        let second = receiver.clone();
        sender
            .send(Update::Status("retained".repeat(4096)))
            .unwrap();
        drop(receiver);
        assert_eq!(second.len(), 1);
        drop(second);
        let queue = sender.queue.lock().unwrap();
        assert!(queue.pending.is_empty());
        assert!(queue.runtime.is_empty());
        drop(queue);
        assert!(sender.send(Update::Status("after close".into())).is_err());
    }

    #[test]
    fn update_mailbox_concurrent_receiver_drop_clears_queue() {
        let (sender, receiver) = update_channel();
        let second = receiver.clone();
        sender
            .send(runtime_update("a", "retained".repeat(4096)))
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let other_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            other_barrier.wait();
            drop(second);
        });
        barrier.wait();
        drop(receiver);
        worker.join().unwrap();
        assert!(sender.queue.lock().unwrap().pending.is_empty());
        assert!(sender.send(Update::Status("after close".into())).is_err());
    }
}
