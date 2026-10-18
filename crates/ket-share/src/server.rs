//! The share itself: one port on one local address that speaks both SOCKS5
//! and HTTP, signs devices in, and relays them through the tunnel.

use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr, SocketAddrV4},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, copy_bidirectional},
    net::{TcpListener, TcpStream},
    sync::watch,
    time::timeout,
};

use crate::{
    credentials::Credentials,
    egress::{Egress, EgressError},
    handshake::Refusal,
    http::{self, Kind, Status},
    limiter::SignInLimiter,
    policy::{self, Origin},
    socks5::{self, Reply},
    target::{Host, Target},
};

/// Time a device has to sign in and say where it is going.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Time for the name lookup and the connection through the tunnel.
const DIAL_TIMEOUT: Duration = Duration::from_secs(20);
/// A name that resolves to several addresses gets this many tries.
const MAX_ADDRESSES_TRIED: usize = 2;
pub const MAX_CONNECTIONS: usize = 256;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShareStats {
    /// Devices with at least one signed-in connection open.
    pub devices: usize,
    pub connections: usize,
    /// Wrong passwords since sharing started.
    pub refused_sign_ins: u64,
}

pub struct ShareServer {
    address: SocketAddrV4,
    stop: watch::Sender<bool>,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    /// Still accepting connections.
    listening: bool,
    /// Open signed-in connections per device address.
    devices: HashMap<IpAddr, usize>,
    /// Every open connection, signed in or not.
    connections: usize,
    refused_sign_ins: u64,
    limiter: SignInLimiter,
}

struct Shared {
    credentials: Credentials,
    egress: Arc<dyn Egress>,
    state: Arc<Mutex<State>>,
    stop: watch::Receiver<bool>,
}

impl ShareServer {
    /// Starts sharing on `address`. Port 0 picks a free one; `address` must
    /// be one of this device's own addresses on the network to share with.
    pub async fn start(
        address: SocketAddrV4,
        credentials: Credentials,
        egress: Arc<dyn Egress>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(address).await?;
        let SocketAddr::V4(address) = listener.local_addr()? else {
            unreachable!("bound to an IPv4 address");
        };
        let state = Arc::new(Mutex::new(State {
            listening: true,
            ..State::default()
        }));
        let (stop, stopped) = watch::channel(false);
        let shared = Arc::new(Shared {
            credentials,
            egress,
            state: state.clone(),
            stop: stopped,
        });
        tokio::spawn(accept_loop(listener, shared));
        Ok(Self {
            address,
            stop,
            state,
        })
    }

    pub fn address(&self) -> SocketAddrV4 {
        self.address
    }

    pub fn stats(&self) -> ShareStats {
        let state = self.state.lock().expect("share state");
        ShareStats {
            devices: state.devices.len(),
            connections: state.connections,
            refused_sign_ins: state.refused_sign_ins,
        }
    }

    /// Stops listening and closes every connection.
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }

    pub fn is_stopped(&self) -> bool {
        *self.stop.borrow()
    }

    /// Waits, for up to a second, until the port is closed and every
    /// connection has ended after [`Self::stop`].
    pub async fn closed(&self) {
        for _ in 0..50 {
            {
                let state = self.state.lock().expect("share state");
                if !state.listening && state.connections == 0 {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for ShareServer {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut stop = shared.stop.clone();
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = stop.wait_for(|stopped| *stopped) => {
                drop(listener);
                shared.state.lock().expect("share state").listening = false;
                return;
            }
        };
        let Ok((stream, peer)) = accepted else {
            // Out of descriptors or a connection reset before accept: keep
            // serving the others.
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };
        let device = peer.ip();
        {
            let mut state = shared.state.lock().expect("share state");
            let signed_in = state.devices.contains_key(&device);
            if state.connections >= MAX_CONNECTIONS
                || state.limiter.turns_away(device, signed_in, Instant::now())
            {
                continue;
            }
            state.connections += 1;
        }
        let shared = shared.clone();
        tokio::spawn(async move {
            let mut stop = shared.stop.clone();
            tokio::select! {
                () = serve(stream, device, &shared) => {}
                _ = stop.wait_for(|stopped| *stopped) => {}
            }
            shared.state.lock().expect("share state").connections -= 1;
        });
    }
}

/// Counts a signed-in device for as long as the connection lasts.
struct SignedIn<'a> {
    device: IpAddr,
    state: &'a Mutex<State>,
}

impl<'a> SignedIn<'a> {
    fn new(device: IpAddr, state: &'a Mutex<State>) -> Self {
        let mut locked = state.lock().expect("share state");
        locked.limiter.succeeded(device);
        *locked.devices.entry(device).or_default() += 1;
        Self { device, state }
    }
}

impl Drop for SignedIn<'_> {
    fn drop(&mut self) {
        let mut state = self.state.lock().expect("share state");
        if let Some(count) = state.devices.get_mut(&self.device) {
            *count -= 1;
            if *count == 0 {
                state.devices.remove(&self.device);
            }
        }
    }
}

async fn serve(mut stream: TcpStream, device: IpAddr, shared: &Shared) {
    let _ = stream.set_nodelay(true);
    let first = match timeout(HANDSHAKE_TIMEOUT, stream.read_u8()).await {
        Ok(Ok(byte)) => byte,
        _ => return,
    };
    if first == socks5::VERSION {
        serve_socks(stream, device, shared).await;
    } else if first.is_ascii_uppercase() {
        serve_http(stream, first, device, shared).await;
    }
}

async fn serve_socks(mut stream: TcpStream, device: IpAddr, shared: &Shared) {
    let target = match timeout(
        HANDSHAKE_TIMEOUT,
        socks5::negotiate(&mut stream, &shared.credentials),
    )
    .await
    {
        Ok(Ok(target)) => target,
        Ok(Err(refusal)) => return note_refusal(refusal, device, shared),
        Err(_) => return,
    };
    let _signed_in = SignedIn::new(device, &shared.state);
    match dial(&target, shared.egress.as_ref()).await {
        Ok(mut site) => {
            socks5::reply(&mut stream, Reply::Succeeded).await;
            relay(&mut stream, &mut site).await;
        }
        Err(failure) => socks5::reply(&mut stream, failure.socks_reply()).await,
    }
}

async fn serve_http(mut stream: TcpStream, first: u8, device: IpAddr, shared: &Shared) {
    let request = timeout(HANDSHAKE_TIMEOUT, async {
        let (head, rest) = http::read_head(&mut stream, Some(first)).await?;
        let request = http::parse_request(&head)?;
        Ok::<_, Refusal>((request, rest))
    })
    .await;
    let (request, rest) = match request {
        Ok(Ok(parsed)) => parsed,
        Ok(Err(refusal)) => {
            answer(&mut stream, refusal_status(refusal)).await;
            return note_refusal(refusal, device, shared);
        }
        Err(_) => return,
    };
    if let Err(refusal) = request.authorize(&shared.credentials) {
        answer(&mut stream, Status::SignInRequired).await;
        return note_refusal(refusal, device, shared);
    }
    let _signed_in = SignedIn::new(device, &shared.state);
    let mut site = match dial(&request.target, shared.egress.as_ref()).await {
        Ok(site) => site,
        Err(failure) => return answer(&mut stream, failure.http_status()).await,
    };
    match request.kind {
        Kind::Tunnel => {
            if stream
                .write_all(&http::response(Status::Established))
                .await
                .is_err()
                || site.write_all(&rest).await.is_err()
            {
                return;
            }
        }
        Kind::Forward(head) => {
            if site.write_all(&head).await.is_err() || site.write_all(&rest).await.is_err() {
                return answer(&mut stream, Status::BadGateway).await;
            }
            let Ok(Ok((response, body))) =
                timeout(DIAL_TIMEOUT, http::read_head(&mut site, None)).await
            else {
                return answer(&mut stream, Status::BadGateway).await;
            };
            let Some(response) = http::rewrite_response(&response) else {
                return answer(&mut stream, Status::BadGateway).await;
            };
            if stream.write_all(&response).await.is_err() || stream.write_all(&body).await.is_err()
            {
                return;
            }
        }
    }
    relay(&mut stream, &mut site).await;
}

fn note_refusal(refusal: Refusal, device: IpAddr, shared: &Shared) {
    if refusal == Refusal::BadCredentials {
        let mut state = shared.state.lock().expect("share state");
        state.refused_sign_ins += 1;
        state.limiter.failed(device, Instant::now());
    }
}

fn refusal_status(refusal: Refusal) -> Status {
    match refusal {
        Refusal::BadCredentials | Refusal::NoCredentials => Status::SignInRequired,
        Refusal::Target(crate::target::TargetError::LocalName) => Status::NotAllowed,
        Refusal::Unsupported => Status::NotForwarded,
        Refusal::Malformed | Refusal::Target(_) => Status::BadRequest,
    }
}

async fn answer(stream: &mut TcpStream, status: Status) {
    let _ = stream.write_all(&http::response(status)).await;
    let _ = stream.shutdown().await;
}

async fn relay<A, B>(device: &mut A, site: &mut B)
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let _ = copy_bidirectional(device, site).await;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DialFailure {
    NotAllowed,
    NoTunnel,
    Unresolved,
    TimedOut,
    Refused,
    Unreachable,
}

impl From<EgressError> for DialFailure {
    fn from(error: EgressError) -> Self {
        match error {
            EgressError::NoTunnel => Self::NoTunnel,
            EgressError::Unresolved => Self::Unresolved,
            EgressError::TimedOut => Self::TimedOut,
            EgressError::Refused => Self::Refused,
            EgressError::Unreachable => Self::Unreachable,
        }
    }
}

impl DialFailure {
    fn socks_reply(self) -> Reply {
        match self {
            Self::NotAllowed => Reply::NotAllowed,
            Self::NoTunnel => Reply::NetworkUnreachable,
            Self::Unresolved | Self::Unreachable => Reply::HostUnreachable,
            Self::TimedOut => Reply::TimedOut,
            Self::Refused => Reply::ConnectionRefused,
        }
    }

    fn http_status(self) -> Status {
        match self {
            Self::NotAllowed => Status::NotAllowed,
            Self::NoTunnel => Status::NotConnected,
            Self::TimedOut => Status::GatewayTimeout,
            Self::Unresolved | Self::Refused | Self::Unreachable => Status::BadGateway,
        }
    }
}

/// Resolves and connects through the tunnel, refusing any address outside
/// the public Internet (see [`policy`]).
async fn dial(target: &Target, egress: &dyn Egress) -> Result<TcpStream, DialFailure> {
    let attempt = async {
        let (addresses, origin) = match &target.host {
            Host::Address(address) => (vec![*address], Origin::Requested),
            Host::Name(name) => (egress.resolve(name).await?, Origin::Resolved),
        };
        let synthetic = egress.synthetic_range();
        let allowed = addresses
            .into_iter()
            .filter(|address| policy::allowed(*address, origin, synthetic))
            .take(MAX_ADDRESSES_TRIED)
            .collect::<Vec<_>>();
        if allowed.is_empty() {
            return Err(DialFailure::NotAllowed);
        }
        let mut failure = DialFailure::Unreachable;
        for address in allowed {
            match egress.connect(SocketAddr::new(address, target.port)).await {
                Ok(stream) => return Ok(stream),
                Err(EgressError::NoTunnel) => return Err(DialFailure::NoTunnel),
                Err(error) => failure = error.into(),
            }
        }
        Err(failure)
    };
    match timeout(DIAL_TIMEOUT, attempt).await {
        Ok(result) => {
            let stream = result?;
            let _ = stream.set_nodelay(true);
            Ok(stream)
        }
        Err(_) => Err(DialFailure::TimedOut),
    }
}

#[cfg(test)]
mod tests;
