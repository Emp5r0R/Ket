use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::Mutex as StdMutex,
};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::{limiter, policy::Ipv4Range};

const PASSWORD: &str = "abcdefghjkmn";
const PUBLIC: &str = "93.184.216.34";

/// Stands in for the tunnel: every name resolves to the address in
/// `answers`, and every connection, wherever it is meant to go, reaches the
/// test's own site on loopback. It records what the share asked for.
struct FakeTunnel {
    site: SocketAddr,
    up: bool,
    answers: Vec<IpAddr>,
    dialled: StdMutex<Vec<SocketAddr>>,
}

#[async_trait]
impl Egress for FakeTunnel {
    async fn resolve(&self, _name: &str) -> Result<Vec<IpAddr>, EgressError> {
        if !self.up {
            return Err(EgressError::NoTunnel);
        }
        Ok(self.answers.clone())
    }

    async fn connect(&self, address: SocketAddr) -> Result<TcpStream, EgressError> {
        if !self.up {
            return Err(EgressError::NoTunnel);
        }
        self.dialled.lock().unwrap().push(address);
        TcpStream::connect(self.site)
            .await
            .map_err(|error| EgressError::from_connect(&error))
    }

    fn synthetic_range(&self) -> Option<Ipv4Range> {
        Some(Ipv4Range::new(Ipv4Addr::new(198, 18, 0, 0), 15))
    }
}

struct Lab {
    share: ShareServer,
    tunnel: Arc<FakeTunnel>,
    site: TcpListener,
}

async fn lab(up: bool, answers: &[&str]) -> Lab {
    let site = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tunnel = Arc::new(FakeTunnel {
        site: site.local_addr().unwrap(),
        up,
        answers: answers
            .iter()
            .map(|answer| answer.parse().unwrap())
            .collect(),
        dialled: StdMutex::default(),
    });
    let share = ShareServer::start(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        Credentials::new("ket", PASSWORD),
        tunnel.clone(),
    )
    .await
    .unwrap();
    Lab {
        share,
        tunnel,
        site,
    }
}

impl Lab {
    async fn device(&self) -> TcpStream {
        TcpStream::connect(self.share.address()).await.unwrap()
    }

    fn dialled(&self) -> Vec<SocketAddr> {
        self.tunnel.dialled.lock().unwrap().clone()
    }
}

async fn socks_sign_in(stream: &mut TcpStream, password: &str) -> [u8; 2] {
    stream.write_all(&[5, 1, 2]).await.unwrap();
    let mut method = [0; 2];
    stream.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 2]);
    let mut request = vec![1, 3];
    request.extend(b"ket");
    request.push(password.len() as u8);
    request.extend(password.as_bytes());
    stream.write_all(&request).await.unwrap();
    let mut status = [0; 2];
    stream.read_exact(&mut status).await.unwrap();
    status
}

async fn socks_connect(stream: &mut TcpStream, name: &str, port: u16) -> u8 {
    let mut request = vec![5, 1, 0, 3, name.len() as u8];
    request.extend(name.as_bytes());
    request.extend(port.to_be_bytes());
    stream.write_all(&request).await.unwrap();
    let mut reply = [0; 10];
    stream.read_exact(&mut reply).await.unwrap();
    reply[1]
}

fn authorization(password: &str) -> String {
    format!(
        "Proxy-Authorization: Basic {}\r\n",
        STANDARD.encode(format!("ket:{password}"))
    )
}

async fn read_all(stream: &mut TcpStream) -> String {
    let mut text = Vec::new();
    let _ = stream.read_to_end(&mut text).await;
    String::from_utf8_lossy(&text).into_owned()
}

async fn echo_once(site: &TcpListener) {
    let (mut stream, _) = site.accept().await.unwrap();
    let mut buffer = [0; 5];
    stream.read_exact(&mut buffer).await.unwrap();
    stream.write_all(&buffer).await.unwrap();
}

#[tokio::test]
async fn socks_devices_reach_a_named_site_through_the_tunnel() {
    let lab = lab(true, &[PUBLIC]).await;
    let mut device = lab.device().await;
    assert_eq!(socks_sign_in(&mut device, PASSWORD).await, [1, 0]);
    assert_eq!(socks_connect(&mut device, "example.com", 443).await, 0);
    assert_eq!(lab.share.stats().devices, 1);
    let site = echo_once(&lab.site);
    device.write_all(b"hello").await.unwrap();
    site.await;
    let mut echoed = [0; 5];
    device.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"hello");
    assert_eq!(lab.dialled(), ["93.184.216.34:443".parse().unwrap()]);
    drop(device);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        lab.share.stats(),
        ShareStats {
            devices: 0,
            connections: 0,
            refused_sign_ins: 0
        }
    );
}

#[tokio::test]
async fn http_connect_signs_in_and_tunnels() {
    let lab = lab(true, &[PUBLIC]).await;
    let mut device = lab.device().await;
    device
        .write_all(
            format!(
                "CONNECT example.com:443 HTTP/1.1\r\n{}\r\nhel",
                authorization(PASSWORD)
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut established = [0; 39];
    device.read_exact(&mut established).await.unwrap();
    assert_eq!(&established, b"HTTP/1.1 200 Connection established\r\n\r\n");
    let site = echo_once(&lab.site);
    device.write_all(b"lo").await.unwrap();
    site.await;
    let mut echoed = [0; 5];
    device.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"hello");
}

#[tokio::test]
async fn plain_http_is_forwarded_without_the_password() {
    let lab = lab(true, &[PUBLIC]).await;
    let mut device = lab.device().await;
    device
        .write_all(format!("GET http://example.com/page HTTP/1.1\r\nHost: example.com\r\n{}Connection: keep-alive\r\n\r\n", authorization(PASSWORD)).as_bytes())
        .await
        .unwrap();
    let (mut site, _) = lab.site.accept().await.unwrap();
    let (head, _) = http::read_head(&mut site, None).await.unwrap();
    assert_eq!(
        head,
        b"GET /page HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"
    );
    site.write_all(b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: 2\r\n\r\nok")
        .await
        .unwrap();
    drop(site);
    assert_eq!(
        read_all(&mut device).await,
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
    );
}

#[tokio::test]
async fn devices_without_the_password_are_asked_for_it_and_guessing_is_stopped() {
    let lab = lab(true, &[PUBLIC]).await;
    let mut device = lab.device().await;
    device
        .write_all(b"CONNECT example.com:443 HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert!(read_all(&mut device).await.starts_with("HTTP/1.1 407 "));
    assert_eq!(
        lab.share.stats().refused_sign_ins,
        0,
        "asking is not guessing"
    );

    for _ in 0..limiter::MAX_FAILURES {
        let mut device = lab.device().await;
        assert_eq!(socks_sign_in(&mut device, "wrongwrongwr").await, [1, 1]);
    }
    // The share counts a wrong try just after answering it.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        lab.share.stats().refused_sign_ins,
        u64::from(limiter::MAX_FAILURES)
    );
    // Turned away before it can try again, even with the right password.
    let mut device = lab.device().await;
    let _ = device.write_all(&[5, 1, 2]).await;
    let mut reply = Vec::new();
    let _ = device.read_to_end(&mut reply).await;
    assert!(reply.is_empty());
    assert!(lab.dialled().is_empty());
}

#[tokio::test]
async fn this_device_and_its_network_are_never_reached() {
    // A name the tunnel resolves to a private address, and private
    // addresses written directly.
    let lab = lab(true, &["192.168.1.1", "127.0.0.1"]).await;
    let mut device = lab.device().await;
    socks_sign_in(&mut device, PASSWORD).await;
    assert_eq!(
        socks_connect(&mut device, "rebind.example", 80).await,
        Reply::NotAllowed as u8
    );

    let mut device = lab.device().await;
    device
        .write_all(
            format!(
                "CONNECT 127.0.0.1:39731 HTTP/1.1\r\n{}\r\n",
                authorization(PASSWORD)
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    assert!(read_all(&mut device).await.starts_with("HTTP/1.1 403 "));

    let mut device = lab.device().await;
    device
        .write_all(
            format!(
                "CONNECT localhost:22 HTTP/1.1\r\n{}\r\n",
                authorization(PASSWORD)
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    assert!(read_all(&mut device).await.starts_with("HTTP/1.1 403 "));
    assert!(lab.dialled().is_empty());
}

#[tokio::test]
async fn the_tunnel_s_stand_in_addresses_are_dialled() {
    let lab = lab(true, &["198.18.0.9"]).await;
    let mut device = lab.device().await;
    socks_sign_in(&mut device, PASSWORD).await;
    assert_eq!(socks_connect(&mut device, "example.com", 443).await, 0);
    assert_eq!(lab.dialled(), ["198.18.0.9:443".parse().unwrap()]);
}

#[tokio::test]
async fn without_a_tunnel_nothing_is_dialled() {
    let lab = lab(false, &[PUBLIC]).await;
    let mut device = lab.device().await;
    socks_sign_in(&mut device, PASSWORD).await;
    assert_eq!(
        socks_connect(&mut device, "example.com", 443).await,
        Reply::NetworkUnreachable as u8
    );

    let mut device = lab.device().await;
    device
        .write_all(
            format!(
                "CONNECT 93.184.216.34:443 HTTP/1.1\r\n{}\r\n",
                authorization(PASSWORD)
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    assert!(read_all(&mut device).await.starts_with("HTTP/1.1 503 "));
}

#[tokio::test]
async fn stopping_closes_open_connections() {
    let lab = lab(true, &[PUBLIC]).await;
    let mut device = lab.device().await;
    socks_sign_in(&mut device, PASSWORD).await;
    assert_eq!(socks_connect(&mut device, "example.com", 443).await, 0);
    let address = lab.share.address();
    lab.share.stop();
    lab.share.closed().await;
    assert!(read_all(&mut device).await.is_empty());
    assert!(TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn other_protocols_get_no_answer() {
    let lab = lab(true, &[PUBLIC]).await;
    let mut device = lab.device().await;
    device
        .write_all(&[4, 1, 0, 80, 1, 1, 1, 1, 0])
        .await
        .unwrap();
    assert!(read_all(&mut device).await.is_empty());
}
