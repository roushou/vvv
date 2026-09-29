//! Cooperative cancellation for one read or explicit validation call.
//! Read publication is atomic; validation retains evidence before reporting cancellation.
use crate::EngineError;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

/// A single-use cancellation handle for reads and explicit validation.
/// Clones cancel the same call.
/// Cancellation stops at the next engine checkpoint, not within a parser invocation.
#[derive(Debug, Clone, Default)]
pub struct ReadCancellation(Arc<ReadState>);
#[derive(Debug, Default)]
struct ReadState {
    state: AtomicU8,
    claimed: AtomicBool,
    publication: Mutex<()>,
}
impl ReadCancellation {
    const ACTIVE: u8 = 0;
    const CANCELLED: u8 = 1;
    const COMPLETE: u8 = 2;
    pub fn cancel(&self) {
        let _gate = self
            .0
            .publication
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = self.0.state.compare_exchange(
            Self::ACTIVE,
            Self::CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.state.load(Ordering::Acquire) == Self::CANCELLED
    }
    pub(crate) fn claim(&self) -> Result<(), EngineError> {
        if self.0.claimed.swap(true, Ordering::AcqRel) {
            return Err(EngineError::ReusedCancellation);
        }
        self.check()
    }
    pub(crate) fn check(&self) -> Result<(), EngineError> {
        if self.is_cancelled() {
            Err(EngineError::ReadCancelled)
        } else {
            Ok(())
        }
    }
    /// Publication and cancellation have one linearization point. Once publication
    /// succeeds a late cancellation cannot turn that completed result into an error.
    pub(crate) fn complete<T>(
        &self,
        publish: impl FnOnce() -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let _gate = self
            .0
            .publication
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.check()?;
        let answer = publish()?;
        self.0.state.store(Self::COMPLETE, Ordering::Release);
        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_work_cannot_publish_and_completed_work_ignores_late_cancellation() {
        let cancelled = ReadCancellation::default();
        cancelled.claim().unwrap();
        cancelled.cancel();
        assert!(matches!(
            cancelled.complete::<()>(|| panic!("must not publish")),
            Err(EngineError::ReadCancelled)
        ));
        let completed = ReadCancellation::default();
        completed.claim().unwrap();
        assert_eq!(completed.complete(|| Ok(42)).unwrap(), 42);
        completed.cancel();
        assert!(!completed.is_cancelled());
        assert!(matches!(
            completed.claim(),
            Err(EngineError::ReusedCancellation)
        ));
    }
}
