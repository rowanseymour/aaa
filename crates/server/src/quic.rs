//! Links to other machines, over QUIC: each a connection with one stream
//! either way, carrying a message as its length, a little-endian `u32`,
//! and then its bytes.
//!
//! What crosses is encrypted, but nothing vouches for the host: it makes
//! itself a certificate as it starts, and a client takes whatever it's
//! shown. The connections are kept by threads of their own, so that
//! neither end of a link ever waits.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{Connection, Endpoint, RecvStream, SendStream};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::runtime::Runtime;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use crate::{Link, Lost};

/// What both ends say they speak as they connect.
const SPEAKS: &[u8] = b"match";

/// The longest message carried, in bytes. A longer one coming is taken as
/// the link being lost.
const LONGEST: usize = 1 << 20;

/// The most messages that have come over a link and not yet been taken
/// that it holds.
const HELD: usize = 256;

/// The most connections a listener keeps that have yet to say anything.
const UNHEARD: usize = 64;

/// How long an end waits to connect.
const CONNECTING: Duration = Duration::from_secs(10);

/// How often an end with nothing to say says so, and how long without a
/// word from the other before the link is taken as lost.
const KEPT_ALIVE: Duration = Duration::from_secs(2);
const SILENT: Duration = Duration::from_secs(10);

/// How long an end that's done with a link waits for what it sent to get
/// there.
const PARTING: Duration = Duration::from_secs(5);

/// An end of a link to another machine.
pub struct Remote {
    /// `None` only as it's dropped.
    to: Option<UnboundedSender<Vec<u8>>>,
    from: tokio::sync::mpsc::Receiver<Vec<u8>>,
    /// Whether nothing more will come, and so nothing more is sent.
    lost: Arc<AtomicBool>,
    /// For an end that connected: what keeps its connection, and what's
    /// sending for it.
    own: Option<(Runtime, JoinHandle<()>)>,
}

impl Link for Remote {
    fn send(&mut self, message: Vec<u8>) -> Result<(), Lost> {
        let to = self
            .to
            .as_ref()
            .filter(|_| !self.lost.load(Ordering::Relaxed));
        to.ok_or(Lost)?.send(message).map_err(|_| Lost)
    }

    fn receive(&mut self) -> Result<Option<Vec<u8>>, Lost> {
        use tokio::sync::mpsc::error::TryRecvError;
        match self.from.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(Lost),
        }
    }
}

impl Drop for Remote {
    /// Waits, though not for long, for what was sent to get there: so an
    /// end that connected mustn't be dropped by anything asynchronous.
    fn drop(&mut self) {
        self.to = None;
        if let Some((runtime, sending)) = self.own.take() {
            let _ = runtime.block_on(async { tokio::time::timeout(PARTING, sending).await });
        }
    }
}

/// Carries messages over `connection`'s streams, on the runtime this is
/// called in: the end to send and receive them by, and what's sending
/// for it, which ends once the end is dropped and what it sent has got
/// there.
fn carry(connection: Connection, send: SendStream, recv: RecvStream) -> (Remote, JoinHandle<()>) {
    let (to, outgoing) = unbounded_channel();
    let (incoming, from) = tokio::sync::mpsc::channel(HELD);
    let lost = Arc::new(AtomicBool::new(false));
    let ended = lost.clone();
    tokio::spawn(async move {
        receiving(recv, incoming).await;
        ended.store(true, Ordering::Relaxed);
    });
    let sending = tokio::spawn(sending(connection, send, outgoing));
    let remote = Remote {
        to: Some(to),
        from,
        lost,
        own: None,
    };
    (remote, sending)
}

/// Passes on each message that comes until the stream ends, the other
/// end sends what isn't one, or nobody's left to pass them to. With
/// [`HELD`] waiting to be taken it reads no more, and the other end finds
/// it can send no more.
async fn receiving(mut recv: RecvStream, incoming: tokio::sync::mpsc::Sender<Vec<u8>>) {
    loop {
        let mut length = [0; 4];
        if recv.read_exact(&mut length).await.is_err() {
            return;
        }
        let length = u32::from_le_bytes(length) as usize;
        if length > LONGEST {
            return;
        }
        let mut message = vec![0; length];
        if recv.read_exact(&mut message).await.is_err() || incoming.send(message).await.is_err() {
            return;
        }
    }
}

/// Sends each message given until there are no more to come, then closes
/// the connection once they've got there.
async fn sending(
    connection: Connection,
    mut send: SendStream,
    mut outgoing: UnboundedReceiver<Vec<u8>>,
) {
    while let Some(message) = outgoing.recv().await {
        let Ok(length) = u32::try_from(message.len()) else {
            break;
        };
        let sent = async {
            send.write_all(&length.to_le_bytes()).await?;
            send.write_all(&message).await
        };
        if sent.await.is_err() {
            break;
        }
    }
    if send.finish().is_ok() {
        let _ = tokio::time::timeout(PARTING, send.stopped()).await;
    }
    connection.close(0u32.into(), b"done");
}

/// How both ends keep a connection: saying so when there's nothing to
/// say, and giving up on an end that's gone quiet.
fn transport() -> Arc<quinn::TransportConfig> {
    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(KEPT_ALIVE));
    transport.max_idle_timeout(SILENT.try_into().ok());
    Arc::new(transport)
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn runtime() -> Result<Runtime, String> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    let runtime = builder.worker_threads(1).enable_all().build();
    runtime.map_err(|e| format!("no thread to keep connections: {e}"))
}

/// Where clients connect to a host.
pub struct Listener {
    /// What keeps its connections; `None` only as it's dropped.
    runtime: Option<Runtime>,
    endpoint: Endpoint,
    /// Behind a lock only so that it can be shared between threads.
    joined: Mutex<Receiver<Remote>>,
    /// What's sending for each end it's handed out.
    sending: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

/// Starts taking connections at `at`. The links it hands out last only as
/// long as it does.
pub fn listen(at: SocketAddr) -> Result<Listener, String> {
    let made = rcgen::generate_simple_self_signed(vec!["host".to_string()]);
    let made = made.map_err(|e| format!("no certificate: {e}"))?;
    let certificate = CertificateDer::from(made.cert);
    let key = PrivatePkcs8KeyDer::from(made.signing_key.serialize_der());
    let tls = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .and_then(|b| {
            b.with_no_client_auth()
                .with_single_cert(vec![certificate], key.into())
        });
    let mut tls = tls.map_err(|e| format!("no certificate: {e}"))?;
    tls.alpn_protocols = vec![SPEAKS.to_vec()];
    let tls = QuicServerConfig::try_from(tls).map_err(|e| format!("no certificate: {e}"))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(tls));
    config.transport_config(transport());

    let runtime = runtime()?;
    let _entered = runtime.enter();
    let endpoint = Endpoint::server(config, at).map_err(|e| format!("{at}: {e}"))?;
    let (joining, joined) = std::sync::mpsc::channel();
    let sending = Arc::new(Mutex::new(Vec::new()));
    runtime.spawn(accepting(endpoint.clone(), joining, sending.clone()));
    drop(_entered);
    Ok(Listener {
        runtime: Some(runtime),
        endpoint,
        joined: Mutex::new(joined),
        sending,
    })
}

/// Hands on a link for each connection that comes, once its client has
/// said something: only so many are waited for at once, and none for
/// long.
async fn accepting(
    endpoint: Endpoint,
    joining: Sender<Remote>,
    sending: Arc<Mutex<Vec<JoinHandle<()>>>>,
) {
    let unheard = Arc::new(Semaphore::new(UNHEARD));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(waiting) = unheard.clone().try_acquire_owned() else {
            incoming.refuse();
            continue;
        };
        let (joining, sending) = (joining.clone(), sending.clone());
        tokio::spawn(async move {
            let heard = tokio::time::timeout(CONNECTING, async {
                let connection = incoming.await.ok()?;
                let (send, recv) = connection.accept_bi().await.ok()?;
                Some((connection, send, recv))
            });
            let Ok(Some((connection, send, recv))) = heard.await else {
                return;
            };
            drop(waiting);
            let (remote, task) = carry(connection, send, recv);
            let mut sending = sending.lock().unwrap_or_else(|e| e.into_inner());
            sending.retain(|task: &JoinHandle<()>| !task.is_finished());
            sending.push(task);
            let _ = joining.send(remote);
        });
    }
}

impl Listener {
    /// Where it's taking connections, which says the port it was given
    /// if it asked for any.
    pub fn at(&self) -> Result<SocketAddr, String> {
        self.endpoint.local_addr().map_err(|e| e.to_string())
    }

    /// A link to the next client to have connected, if one has.
    pub fn accept(&mut self) -> Option<Remote> {
        let joined = self.joined.get_mut().unwrap_or_else(|e| e.into_inner());
        joined.try_recv().ok()
    }
}

impl Drop for Listener {
    /// Waits, though not for long, for what was sent over the links it
    /// handed out that have since been dropped to get there; those still
    /// held are lost.
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let sending = std::mem::take(&mut *self.sending.lock().unwrap_or_else(|e| e.into_inner()));
        runtime.block_on(async {
            let parted = async {
                for task in sending {
                    let _ = task.await;
                }
            };
            let _ = tokio::time::timeout(PARTING, parted).await;
        });
        self.endpoint.close(0u32.into(), b"done");
    }
}

/// Takes whatever certificate a host shows.
#[derive(Debug)]
struct AnyHost(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyHost {
    fn verify_server_cert(
        &self,
        _: &CertificateDer,
        _: &[CertificateDer],
        _: &ServerName,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.0.signature_verification_algorithms;
        verify_tls12_signature(message, cert, dss, algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let algorithms = &self.0.signature_verification_algorithms;
        verify_tls13_signature(message, cert, dss, algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// A link to the host taking connections at `host`, once it's connected.
/// The host only hears of it when something's sent.
pub fn connect(host: SocketAddr) -> Result<Remote, String> {
    connect_within(host, CONNECTING)
}

/// As [`connect`], waiting no longer than `wait` to be connected.
fn connect_within(host: SocketAddr, wait: Duration) -> Result<Remote, String> {
    let provider = provider();
    let tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?;
    let verifier = Arc::new(AnyHost(provider));
    let mut tls = (tls.dangerous())
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![SPEAKS.to_vec()];
    let tls = QuicClientConfig::try_from(tls).map_err(|e| e.to_string())?;
    let mut config = quinn::ClientConfig::new(Arc::new(tls));
    config.transport_config(transport());

    let from: SocketAddr = match host {
        SocketAddr::V4(_) => ([0, 0, 0, 0], 0).into(),
        SocketAddr::V6(_) => ([0u16; 8], 0).into(),
    };
    let runtime = runtime()?;
    let connected = runtime.block_on(async {
        let mut endpoint = Endpoint::client(from).map_err(|e| e.to_string())?;
        endpoint.set_default_client_config(config);
        let connecting = endpoint.connect(host, "host").map_err(|e| e.to_string())?;
        let connected = tokio::time::timeout(wait, async {
            let connection = connecting.await.map_err(|e| e.to_string())?;
            let (send, recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
            Ok::<_, String>(carry(connection, send, recv))
        });
        connected.await.map_err(|_| "no answer".to_string())?
    });
    let (mut remote, sending) = connected.map_err(|e| format!("{host}: {e}"))?;
    remote.own = Some((runtime, sending));
    Ok(remote)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// What comes next over `link`, waiting a while for it.
    fn next(link: &mut Remote) -> Result<Vec<u8>, Lost> {
        let began = Instant::now();
        loop {
            if let Some(message) = link.receive()? {
                return Ok(message);
            }
            assert!(began.elapsed() < Duration::from_secs(20), "nothing came");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn messages_cross_between_a_host_and_those_that_connect_to_it() {
        let mut listener = listen(([127, 0, 0, 1], 0).into()).unwrap();
        let at = listener.at().unwrap();
        let mut client = connect(at).unwrap();
        assert!(listener.accept().is_none());
        client.send(b"hello".to_vec()).unwrap();
        client.send(Vec::new()).unwrap();
        let big = vec![7; 300_000];
        client.send(big.clone()).unwrap();
        let began = Instant::now();
        let mut host = loop {
            if let Some(link) = listener.accept() {
                break link;
            }
            assert!(began.elapsed() < Duration::from_secs(20), "nobody came");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(next(&mut host), Ok(b"hello".to_vec()));
        assert_eq!(next(&mut host), Ok(Vec::new()));
        assert_eq!(next(&mut host), Ok(big));
        // More than is held for an end that isn't taking them: none lost.
        for i in 0..4 * HELD as u32 {
            client.send(i.to_le_bytes().to_vec()).unwrap();
        }
        for i in 0..4 * HELD as u32 {
            assert_eq!(next(&mut host), Ok(i.to_le_bytes().to_vec()));
        }
        host.send(b"welcome".to_vec()).unwrap();
        assert_eq!(next(&mut client), Ok(b"welcome".to_vec()));
        // What an end sent before it went still gets there.
        host.send(b"goodbye".to_vec()).unwrap();
        drop(host);
        assert_eq!(next(&mut client), Ok(b"goodbye".to_vec()));
        assert_eq!(next(&mut client), Err(Lost));
        assert_eq!(client.send(b"too late".to_vec()), Err(Lost));
    }

    #[test]
    fn connecting_to_nobody_fails() {
        // A port nothing's listening at, having just been let go.
        let at = listen(([127, 0, 0, 1], 0).into()).unwrap().at().unwrap();
        assert!(connect_within(at, Duration::from_millis(300)).is_err());
    }
}
