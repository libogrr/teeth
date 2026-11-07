//! Work that runs on its own thread while the window keeps painting.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

/// The answer of `work`, still to come.
pub struct Pending<T> {
    answer: Receiver<Result<T, String>>,
    /// The error if the thread dies without answering.
    lost: String,
}

impl<T: Send + 'static> Pending<T> {
    /// Starts `work` on a thread called `name`.
    pub fn spawn(
        name: &str,
        lost: &str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<Pending<T>, String> {
        let (tx, answer) = mpsc::channel();
        thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _ = tx.send(work());
            })
            .map_err(|_| lost.to_string())?;
        Ok(Pending { answer, lost: lost.to_string() })
    }

    /// The answer, if it has arrived.
    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.answer.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(self.lost.clone())),
        }
    }

    /// Waits up to `timeout` for the answer.
    pub fn wait(&self, timeout: Duration) -> Option<Result<T, String>> {
        match self.answer.recv_timeout(timeout) {
            Ok(result) => Some(result),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(self.lost.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOON: Duration = Duration::from_secs(5);

    #[test]
    fn there_is_no_answer_until_the_work_ends() {
        let (go, gate) = mpsc::channel::<()>();
        let work = move || {
            gate.recv().map_err(|e| e.to_string())?;
            Ok(7)
        };
        let pending = Pending::spawn("prueba", "perdido", work).unwrap();
        assert_eq!(pending.poll(), None);
        go.send(()).unwrap();
        assert_eq!(pending.wait(SOON), Some(Ok(7)));
    }

    #[test]
    fn an_error_of_the_work_comes_back_as_it_is() {
        let pending = Pending::<u8>::spawn("prueba", "perdido", || Err("sin disco".into())).unwrap();
        assert_eq!(pending.wait(SOON), Some(Err("sin disco".to_string())));
    }

    #[test]
    fn a_thread_that_dies_answers_with_the_lost_text() {
        let pending = Pending::<u8>::spawn("prueba", "perdido", || panic!("a propósito")).unwrap();
        assert_eq!(pending.wait(SOON), Some(Err("perdido".to_string())));
        assert_eq!(pending.poll(), Some(Err("perdido".to_string())));
    }

    #[test]
    fn waiting_gives_up_after_the_timeout() {
        let (_go, gate) = mpsc::channel::<()>();
        let work = move || gate.recv().map_err(|e| e.to_string()).map(|()| 7);
        let pending = Pending::spawn("prueba", "perdido", work).unwrap();
        assert_eq!(pending.wait(Duration::from_millis(5)), None);
    }
}
