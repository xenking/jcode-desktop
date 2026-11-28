//! Shared Tokio executor for async network operations from GPUI.
//!
//! Hot reload retains old UI libraries. Each generation's executor therefore
//! retires after being idle rather than relying on static destructors at unload.
use std::{
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};

const IDLE: Duration = Duration::from_secs(60);

struct Shared {
    runtime: tokio::runtime::Runtime,
    active: usize,
    last_finished: Instant,
}

static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        let mut shared = SHARED.lock().unwrap();
        if let Some(shared) = shared.as_mut() {
            shared.active -= 1;
            shared.last_finished = Instant::now();
        }
    }
}

fn start() -> std::io::Result<Shared> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("jcode-network")
        .enable_all()
        .build()?;
    let watchdog = std::thread::Builder::new()
        .name("jcode-network-idle".into())
        .spawn(|| {
            loop {
                std::thread::sleep(IDLE);
                let retired = {
                    let mut shared = SHARED.lock().unwrap();
                    if shared
                        .as_ref()
                        .is_some_and(|s| s.active == 0 && s.last_finished.elapsed() >= IDLE)
                    {
                        shared.take()
                    } else {
                        None
                    }
                };
                if let Some(retired) = retired {
                    drop(retired);
                    break;
                }
            }
        });
    if let Err(error) = watchdog {
        runtime.shutdown_background();
        return Err(error);
    }
    Ok(Shared {
        runtime,
        active: 0,
        last_finished: Instant::now(),
    })
}

/// Run on the shared Tokio executor without occupying the caller's executor.
pub async fn run<F>(future: F) -> Result<F::Output, String>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let task = {
        let mut shared = SHARED.lock().unwrap();
        if shared.is_none() {
            *shared = Some(start().map_err(|error| error.to_string())?);
        }
        let shared = shared.as_mut().unwrap();
        shared.active += 1;
        let active = Active;
        shared.runtime.spawn(async move {
            let _active = active;
            future.await
        })
    };
    task.await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        pin::pin,
        sync::{Arc, mpsc},
        task::{Context, Poll, Wake, Waker},
    };

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn wait<F: Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = pin!(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
            std::thread::park();
        }
    }

    #[test]
    fn returns_values_and_errors_without_blocking_the_caller() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut future = Box::pin(run(async move {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            42
        }));
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(future.as_mut().poll(&mut context).is_pending());
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(future.as_mut().poll(&mut context).is_pending());
        release_tx.send(()).unwrap();
        assert_eq!(wait(future).unwrap(), 42);
        assert_eq!(
            wait(run(async { Err::<(), _>("expected") })).unwrap(),
            Err("expected")
        );
        assert!(wait(run(async { panic!("expected panic") })).is_err());
    }
}
