//! Bridge behaviour: the coordinator, session workers, remote recovery and the
//! SSH and cloud transport pools, driven against in-process daemons.

#[cfg(test)]
mod general;
#[cfg(all(test, target_os = "macos"))]
mod paired_gateway;
#[cfg(all(test, unix))]
mod remote_hosts;
#[cfg(test)]
mod side_panel_routing;
#[cfg(test)]
mod stop_reason_routing;
#[cfg(test)]
mod submission;
#[cfg(all(test, unix))]
mod transport_integration;

use super::*;
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use jcode_sdk::{ApiEvent, ConnectOptions, JcodeClient, SessionInfo};

use super::remote;
use super::remote::transport;
use crate::harness::catalog::persisted_todo_title;
use crate::harness::hub::run_with_transports;
use crate::harness::local_runtime::{
    RUNTIME_REVIVE_INTERVAL, revive_due, should_detach_on_drop, start_local_runtime,
};
use crate::harness::remote::sessions::{create_remote_session, list_remote_sessions};
use crate::harness::session::events::{
    ReconnectWait, TEST_RETRY_POLICY, collect_disconnected_commands, event_session_id,
    is_already_processing_error, is_daemon_connection_closed, namespace_event,
    queue_wrong_session_retry, recover_async_attachment_mismatch, recover_async_busy,
    retry_countdown_phase, update_turn_activity, wait_for_reconnect,
};
use crate::harness::session::worker::{
    MAX_RECONNECT_DELAY, SIDE_QUESTION_REMINDER, next_reconnect_delay, saved_session_identity,
    session_worker, session_worker_with_connector, should_log_reconnect,
};
use crate::harness::session::{
    SessionCommand, WorkerCommand, next_worker_command, recovery, send_to_session_worker,
};
use crate::harness::updates::UpdateSender;
