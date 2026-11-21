//! Console lines from a socket, for driving a running game from scripts. Each
//! line is answered once it has run, with what the console says to it and
//! then an empty line, so a client reading answers in turn knows each
//! command finished before sending the next.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};

use bevy::prelude::*;

/// A line from a client and where its answer goes.
pub type Asked = (String, SyncSender<String>);

/// The lines clients have sent, waiting for the console.
#[derive(Resource)]
pub struct Remote(Mutex<Receiver<Asked>>);

impl Remote {
    /// Listens on `port` of this machine only.
    pub fn listen(port: u16) -> Result<Remote, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|e| format!("console port {port}: {e}"))?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &tx);
                });
            }
        });
        Ok(Remote(Mutex::new(rx)))
    }

    /// The lines sent since last asked.
    pub fn take(&self) -> Vec<Asked> {
        let rx = self.0.lock().expect("not poisoned");
        rx.try_iter().collect()
    }
}

fn serve(stream: TcpStream, lines: &Sender<Asked>) -> std::io::Result<()> {
    let mut out = stream.try_clone()?;
    for line in BufReader::new(stream).lines() {
        let line = line?;
        let (tx, rx) = sync_channel(1);
        if lines.send((line, tx)).is_err() {
            break;
        }
        // The game dropped the line only by ending.
        let Ok(answer) = rx.recv() else { break };
        out.write_all(answer.as_bytes())?;
        out.write_all(b"\n")?;
        out.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_each_line_in_turn() {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let remote = Remote::listen(port).unwrap();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            stream.write_all(b"echo a\nzoom\n").unwrap();
            stream.shutdown(std::net::Shutdown::Write).unwrap();
            let mut answers = String::new();
            std::io::Read::read_to_string(&mut stream, &mut answers).unwrap();
            answers
        });
        let mut answered = 0;
        while answered < 2 {
            for (line, tx) in remote.take() {
                let answer = match line.as_str() {
                    "echo a" => "a\n".to_string(),
                    _ => String::new(),
                };
                tx.send(answer).unwrap();
                answered += 1;
            }
            std::thread::yield_now();
        }
        assert_eq!(client.join().unwrap(), "a\n\n\n");
    }
}
