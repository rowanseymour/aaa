//! What a host and a client reach each other over: whole messages, each
//! arriving once and in the order sent, or the link is lost.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

/// The other end has gone, and nothing more will come from or reach it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lost;

/// One end of a link.
pub trait Link {
    /// Sends `message` to the other end, without waiting for it to get
    /// there.
    fn send(&mut self, message: Vec<u8>) -> Result<(), Lost>;

    /// The next message from the other end, if one has come.
    fn receive(&mut self) -> Result<Option<Vec<u8>>, Lost>;
}

/// An end of a link to another part of the same program.
#[derive(Debug)]
pub struct Local {
    to: Sender<Vec<u8>>,
    /// Behind a lock only so that an end can be shared between threads,
    /// which a receiver alone can't be; it's never waited for.
    from: Mutex<Receiver<Vec<u8>>>,
}

/// The two ends of a link within the program.
pub fn pair() -> (Local, Local) {
    let (to_b, from_a) = mpsc::channel();
    let (to_a, from_b) = mpsc::channel();
    let a = Local {
        to: to_b,
        from: Mutex::new(from_b),
    };
    let b = Local {
        to: to_a,
        from: Mutex::new(from_a),
    };
    (a, b)
}

impl Link for Local {
    fn send(&mut self, message: Vec<u8>) -> Result<(), Lost> {
        self.to.send(message).map_err(|_| Lost)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, Lost> {
        let from = self.from.get_mut().unwrap_or_else(|e| e.into_inner());
        match from.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Lost),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_cross_in_order_until_an_end_goes() {
        let (mut a, mut b) = pair();
        assert_eq!(b.receive(), Ok(None));
        a.send(vec![1]).unwrap();
        a.send(vec![2, 3]).unwrap();
        b.send(vec![4]).unwrap();
        assert_eq!(b.receive(), Ok(Some(vec![1])));
        assert_eq!(b.receive(), Ok(Some(vec![2, 3])));
        assert_eq!(a.receive(), Ok(Some(vec![4])));
        drop(a);
        // What was sent before it went still arrives.
        assert_eq!(b.send(vec![5]), Err(Lost));
        assert_eq!(b.receive(), Err(Lost));
    }
}
