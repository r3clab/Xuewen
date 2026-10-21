use std::fmt;

use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Signal {
    Interrupt,
    Terminate,
}

impl Signal {
    /// The shell's status for a process killed by this signal.
    pub(super) fn exit_code(self) -> i32 {
        match self {
            Self::Interrupt => 130,
            Self::Terminate => 143,
        }
    }
}

/// A command ended by `Signal`: `cli::run` exits with its status, silently,
/// once the runtime is down.
#[derive(Debug)]
pub(super) struct Interrupted(pub(super) Signal);

impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "interrupted ({:?})", self.0)
    }
}

impl std::error::Error for Interrupted {}

/// Cancel `token` on SIGINT or SIGTERM. The container runs the server as
/// PID 1, which gets no default signal disposition — without an explicit
/// handler `docker stop` and Kubernetes rollouts hang the full grace period
/// and end in SIGKILL.
pub(super) async fn cancel_on_signal(token: CancellationToken) {
    let mut signals = Signals::install();
    signals.recv().await;
    token.cancel();
    std::process::exit(signals.recv().await.exit_code());
}

#[cfg(unix)]
pub(super) struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    pub(super) fn install() -> Self {
        use tokio::signal::unix::{signal, SignalKind};
        Self {
            interrupt: signal(SignalKind::interrupt())
                .expect("failed to install the Ctrl-C handler"),
            terminate: signal(SignalKind::terminate())
                .expect("failed to install the SIGTERM handler"),
        }
    }

    pub(super) async fn recv(&mut self) -> Signal {
        tokio::select! {
            _ = self.interrupt.recv() => Signal::Interrupt,
            _ = self.terminate.recv() => Signal::Terminate,
        }
    }
}

#[cfg(not(unix))]
pub(super) struct Signals;

#[cfg(not(unix))]
impl Signals {
    pub(super) fn install() -> Self {
        Self
    }

    pub(super) async fn recv(&mut self) -> Signal {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install the Ctrl-C handler");
        Signal::Interrupt
    }
}
