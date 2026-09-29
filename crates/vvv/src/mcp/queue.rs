//! One dedicated engine worker and a cancellable, bounded FIFO.
use crate::output::mcp::McpIssue;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::oneshot;
use vvv_engine::{Answer, Call, Engine, ReadCancellation, Reply};

pub(crate) struct WorkQueue {
    state: Mutex<State>,
    wake: Condvar,
    sequence: AtomicU64,
}
#[derive(Default)]
struct State {
    jobs: VecDeque<Job>,
    active: Option<(u64, ReadCancellation)>,
    closed: bool,
}
impl State {
    fn start(&mut self) -> Option<Job> {
        let job = self.jobs.pop_front()?;
        // Queued writes can be cancelled. Once dequeued, let the transaction
        // finish and preserve its receipt for retries.
        self.active = job
            .call
            .request
            .is_cancellable()
            .then(|| (job.id, job.cancellation.clone()));
        Some(job)
    }
}
struct Job {
    id: u64,
    call: Call,
    cancellation: ReadCancellation,
    reply: oneshot::Sender<Reply<Answer>>,
}
pub(crate) struct PendingCall {
    queue: Arc<WorkQueue>,
    id: u64,
}
impl PendingCall {
    pub fn cancel(&self) -> bool {
        self.queue.cancel(self.id)
    }
}
impl Drop for PendingCall {
    fn drop(&mut self) {
        self.queue.cancel(self.id);
    }
}
impl WorkQueue {
    pub const CAPACITY: usize = 8;
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            sequence: AtomicU64::new(0),
        })
    }
    pub fn submit(
        self: &Arc<Self>,
        call: Call,
    ) -> Result<(PendingCall, oneshot::Receiver<Reply<Answer>>), McpIssue> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Err(McpIssue::Closed);
        }
        if state.jobs.len() >= Self::CAPACITY {
            return Err(McpIssue::Busy);
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let (reply, receive) = oneshot::channel();
        state.jobs.push_back(Job {
            id,
            call,
            cancellation: ReadCancellation::default(),
            reply,
        });
        self.wake.notify_one();
        Ok((
            PendingCall {
                queue: self.clone(),
                id,
            },
            receive,
        ))
    }
    fn cancel(&self, id: u64) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = state.jobs.iter().position(|j| j.id == id) {
            state.jobs.remove(index);
            return true;
        }
        if let Some((active, cancellation)) = &state.active
            && *active == id
        {
            cancellation.cancel();
            return cancellation.is_cancelled();
        }
        false
    }
    pub fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        state.jobs.clear();
        if let Some((_, cancellation)) = &state.active {
            cancellation.cancel();
        }
        self.wake.notify_all();
    }
    pub fn run(&self, engine: Engine) {
        loop {
            let job = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                loop {
                    if state.closed {
                        return;
                    }
                    if let Some(job) = state.start() {
                        break job;
                    }
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
            };
            let reply = if job.call.request.is_cancellable() {
                job.call
                    .execute_with_cancellation(&engine, &job.cancellation)
            } else {
                job.call.execute(&engine)
            };
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .active = None;
            let _ = job.reply.send(reply);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_cancellation_frees_capacity_and_eof_discards_every_job() {
        let queue = WorkQueue::new();
        let call = || serde_json::from_str::<Call>(r#"{"command":"discover"}"#).unwrap();
        let mut pending = (0..WorkQueue::CAPACITY)
            .map(|_| queue.submit(call()).unwrap())
            .collect::<Vec<_>>();
        assert!(matches!(queue.submit(call()), Err(McpIssue::Busy)));
        let (guard, mut receiver) = pending.pop().unwrap();
        assert!(guard.cancel());
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        pending.push(queue.submit(call()).unwrap());
        queue.close();
        assert!(matches!(queue.submit(call()), Err(McpIssue::Closed)));
        for (_, mut receiver) in pending {
            assert!(matches!(
                receiver.try_recv(),
                Err(oneshot::error::TryRecvError::Closed)
            ));
        }
    }

    #[test]
    fn apply_cancellation_stops_queued_work_but_never_an_active_transaction() {
        let queue = WorkQueue::new();
        let call = || {
            serde_json::from_str::<Call>(
                r#"{"command":"apply_plan","plan_id":"p1.0000000000000000.0000000000000001"}"#,
            )
            .unwrap()
        };
        let (queued, mut dropped) = queue.submit(call()).unwrap();
        assert!(queued.cancel());
        assert!(matches!(
            dropped.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        let (active, _) = queue.submit(call()).unwrap();
        let job = queue.state.lock().unwrap().start().unwrap();
        assert!(!active.cancel());
        queue.close();
        assert!(!job.cancellation.is_cancelled());
        assert!(queue.state.lock().unwrap().jobs.is_empty());
    }

    #[test]
    fn active_cancellation_observes_the_engine_publication_decision() {
        let queue = WorkQueue::new();
        let cancellation = ReadCancellation::default();
        queue.state.lock().unwrap().active = Some((1, cancellation.clone()));
        assert!(queue.cancel(1));
        assert!(cancellation.is_cancelled());
        queue.state.lock().unwrap().active = None;
        assert!(!queue.cancel(1));
    }
}

#[cfg(test)]
mod sdk_tests {
    use super::*;
    use crate::mcp::{
        McpSession,
        catalog::{ToolEntry, ToolKind},
        transport::BoundedTransport,
    };
    use rmcp::{ServiceExt, model::*, service::PeerRequestOptions};
    use std::time::Duration;

    #[tokio::test]
    async fn sdk_cancellation_removes_a_queued_call_and_eof_closes_the_queue() {
        tokio::time::timeout(Duration::from_secs(10), async {
            let queue = WorkQueue::new();
            let session = McpSession {
                queue: queue.clone(),
                tools: ToolKind::ALL
                    .into_iter()
                    .map(|kind| ToolEntry::new(kind).unwrap())
                    .collect(),
            };
            let (server_io, client_io) = tokio::io::duplex(256 * 1024);
            let (read, write) = tokio::io::split(server_io);
            let wire = BoundedTransport::new(read, write, queue.clone());
            let server =
                tokio::spawn(
                    async move { session.serve(wire).await.unwrap().waiting().await.unwrap() },
                );
            let client = ClientInfo::default().serve(client_io).await.unwrap();
            let pending = client
                .send_cancellable_request(
                    ClientRequest::CallToolRequest(Request::new(CallToolRequestParams::new(
                        "vvv_discover",
                    ))),
                    PeerRequestOptions::no_options(),
                )
                .await
                .unwrap();
            while queue.state.lock().unwrap().jobs.is_empty() {
                tokio::task::yield_now().await;
            }
            client
                .notify_cancelled(CancelledNotificationParam {
                    request_id: pending.id.clone(),
                    reason: None,
                })
                .await
                .unwrap();
            assert!(matches!(
                pending.await_response().await,
                Err(rmcp::ServiceError::Cancelled { .. })
            ));
            while !queue.state.lock().unwrap().jobs.is_empty() {
                tokio::task::yield_now().await;
            }
            client.cancel().await.unwrap();
            server.await.unwrap();
            assert!(queue.state.lock().unwrap().closed);
        })
        .await
        .unwrap();
    }
}
