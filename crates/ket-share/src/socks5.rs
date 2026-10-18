//! SOCKS5 (RFC 1928) with user name and password sign-in (RFC 1929),
//! `CONNECT` only. A device that offers no password is turned away: the share
//! never runs open.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    credentials::Credentials,
    handshake::Refusal,
    target::{Host, Target, TargetError},
};

pub(crate) const VERSION: u8 = 0x05;
const PASSWORD_METHOD: u8 = 0x02;
const NO_ACCEPTABLE_METHOD: u8 = 0xff;
const PASSWORD_VERSION: u8 = 0x01;
const CONNECT: u8 = 0x01;
const IPV4: u8 = 0x01;
const NAME: u8 = 0x03;
const IPV6: u8 = 0x04;

/// RFC 1928 reply codes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Reply {
    Succeeded = 0x00,
    GeneralFailure = 0x01,
    NotAllowed = 0x02,
    NetworkUnreachable = 0x03,
    HostUnreachable = 0x04,
    ConnectionRefused = 0x05,
    TimedOut = 0x06,
    CommandNotSupported = 0x07,
    AddressNotSupported = 0x08,
}

/// Runs the handshake after the version byte, which the caller has read to
/// tell SOCKS from HTTP, and returns where the device wants to go. On a
/// refusal the device has already been told why.
pub(crate) async fn negotiate<S>(
    stream: &mut S,
    credentials: &Credentials,
) -> Result<Target, Refusal>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let count = read_u8(stream).await?;
    let mut methods = vec![0; usize::from(count)];
    stream
        .read_exact(&mut methods)
        .await
        .map_err(|_| Refusal::Malformed)?;
    if !methods.contains(&PASSWORD_METHOD) {
        let _ = stream.write_all(&[VERSION, NO_ACCEPTABLE_METHOD]).await;
        return Err(Refusal::NoCredentials);
    }
    write(stream, &[VERSION, PASSWORD_METHOD]).await?;

    if read_u8(stream).await? != PASSWORD_VERSION {
        return Err(Refusal::Malformed);
    }
    let username = read_field(stream).await?;
    let password = zeroize::Zeroizing::new(read_field(stream).await?);
    if !credentials.matches(&username, &password) {
        let _ = stream.write_all(&[PASSWORD_VERSION, 0x01]).await;
        return Err(Refusal::BadCredentials);
    }
    write(stream, &[PASSWORD_VERSION, 0x00]).await?;

    let mut header = [0; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| Refusal::Malformed)?;
    if header[0] != VERSION || header[2] != 0 {
        return Err(Refusal::Malformed);
    }
    if header[1] != CONNECT {
        reply(stream, Reply::CommandNotSupported).await;
        return Err(Refusal::Unsupported);
    }
    let host = match header[3] {
        IPV4 => {
            let mut octets = [0; 4];
            stream
                .read_exact(&mut octets)
                .await
                .map_err(|_| Refusal::Malformed)?;
            Host::Address(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        IPV6 => {
            let mut octets = [0; 16];
            stream
                .read_exact(&mut octets)
                .await
                .map_err(|_| Refusal::Malformed)?;
            Host::Address(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        NAME => {
            let name = read_field(stream).await?;
            let parsed = std::str::from_utf8(&name)
                .map_err(|_| TargetError::Invalid)
                .and_then(Host::parse);
            match parsed {
                Ok(host) => host,
                Err(error) => {
                    // The port still follows; the device gets its answer either way.
                    reply(stream, refusal_reply(error)).await;
                    return Err(Refusal::Target(error));
                }
            }
        }
        _ => {
            reply(stream, Reply::AddressNotSupported).await;
            return Err(Refusal::Unsupported);
        }
    };
    let mut port = [0; 2];
    stream
        .read_exact(&mut port)
        .await
        .map_err(|_| Refusal::Malformed)?;
    let port = u16::from_be_bytes(port);
    if port == 0 {
        reply(stream, Reply::GeneralFailure).await;
        return Err(Refusal::Target(TargetError::Invalid));
    }
    Ok(Target { host, port })
}

/// Answers the `CONNECT`. The bound address is left as 0.0.0.0:0: the device
/// has no use for the address the share dialled from.
pub(crate) async fn reply<S>(stream: &mut S, reply: Reply)
where
    S: AsyncWrite + Unpin,
{
    let _ = stream
        .write_all(&[VERSION, reply as u8, 0, IPV4, 0, 0, 0, 0, 0, 0])
        .await;
}

fn refusal_reply(error: TargetError) -> Reply {
    match error {
        TargetError::LocalName => Reply::NotAllowed,
        TargetError::Invalid => Reply::GeneralFailure,
    }
}

async fn read_u8<S: AsyncRead + Unpin>(stream: &mut S) -> Result<u8, Refusal> {
    stream.read_u8().await.map_err(|_| Refusal::Malformed)
}

/// A length-prefixed field (1 to 255 bytes).
async fn read_field<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Vec<u8>, Refusal> {
    let length = read_u8(stream).await?;
    if length == 0 {
        return Err(Refusal::Malformed);
    }
    let mut field = vec![0; usize::from(length)];
    stream
        .read_exact(&mut field)
        .await
        .map_err(|_| Refusal::Malformed)?;
    Ok(field)
}

async fn write<S: AsyncWrite + Unpin>(stream: &mut S, bytes: &[u8]) -> Result<(), Refusal> {
    stream
        .write_all(bytes)
        .await
        .map_err(|_| Refusal::Malformed)
}

#[cfg(test)]
mod tests {
    use tokio::io::duplex;

    use super::*;

    fn credentials() -> Credentials {
        Credentials::new("ket", "abcdefghjkmn")
    }

    /// Plays `client` into the handshake (after the version byte) and
    /// returns the result with everything the share wrote back.
    async fn run(client: &[u8]) -> (Result<Target, Refusal>, Vec<u8>) {
        let (mut near, mut share) = duplex(4096);
        near.write_all(client).await.unwrap();
        near.shutdown().await.unwrap();
        let result = negotiate(&mut share, &credentials()).await;
        drop(share);
        let mut written = Vec::new();
        near.read_to_end(&mut written).await.unwrap();
        (result, written)
    }

    fn sign_in() -> Vec<u8> {
        let mut bytes = vec![2, 0x00, 0x02, 0x01, 3];
        bytes.extend(b"ket");
        bytes.push(12);
        bytes.extend(b"abcdefghjkmn");
        bytes
    }

    #[tokio::test]
    async fn a_signed_in_connect_to_a_name_returns_the_target() {
        let mut client = sign_in();
        client.extend([VERSION, CONNECT, 0, NAME, 11]);
        client.extend(b"example.com");
        client.extend(443u16.to_be_bytes());
        let (result, written) = run(&client).await;
        assert_eq!(
            result,
            Ok(Target {
                host: Host::Name("example.com".to_owned()),
                port: 443
            })
        );
        assert_eq!(written, [VERSION, PASSWORD_METHOD, PASSWORD_VERSION, 0x00]);
    }

    #[tokio::test]
    async fn addresses_of_both_families_are_read() {
        let mut client = sign_in();
        client.extend([VERSION, CONNECT, 0, IPV4, 93, 184, 216, 34, 0, 80]);
        assert_eq!(
            run(&client).await.0,
            Ok(Target {
                host: Host::Address("93.184.216.34".parse().unwrap()),
                port: 80
            })
        );
        let mut client = sign_in();
        client.extend([VERSION, CONNECT, 0, IPV6]);
        client.extend("2606:4700::1".parse::<Ipv6Addr>().unwrap().octets());
        client.extend(443u16.to_be_bytes());
        assert_eq!(
            run(&client).await.0,
            Ok(Target {
                host: Host::Address("2606:4700::1".parse().unwrap()),
                port: 443
            })
        );
    }

    #[tokio::test]
    async fn a_device_without_a_password_is_turned_away() {
        let (result, written) = run(&[1, 0x00]).await;
        assert_eq!(result, Err(Refusal::NoCredentials));
        assert_eq!(written, [VERSION, NO_ACCEPTABLE_METHOD]);
    }

    #[tokio::test]
    async fn a_wrong_password_is_refused_and_counted() {
        let mut client = vec![1, 0x02, 0x01, 3];
        client.extend(b"ket");
        client.push(12);
        client.extend(b"abcdefghjkmp");
        let (result, written) = run(&client).await;
        assert_eq!(result, Err(Refusal::BadCredentials));
        assert_eq!(written, [VERSION, PASSWORD_METHOD, PASSWORD_VERSION, 0x01]);
    }

    #[tokio::test]
    async fn only_connect_is_offered() {
        let mut client = sign_in();
        client.extend([VERSION, 0x03, 0, IPV4, 0, 0, 0, 0, 0, 0]);
        let (result, written) = run(&client).await;
        assert_eq!(result, Err(Refusal::Unsupported));
        assert_eq!(written[4..6], [VERSION, Reply::CommandNotSupported as u8]);
    }

    #[tokio::test]
    async fn local_names_and_bad_input_are_refused() {
        let mut client = sign_in();
        client.extend([VERSION, CONNECT, 0, NAME, 9]);
        client.extend(b"localhost");
        client.extend(80u16.to_be_bytes());
        let (result, written) = run(&client).await;
        assert_eq!(result, Err(Refusal::Target(TargetError::LocalName)));
        assert_eq!(written[4..6], [VERSION, Reply::NotAllowed as u8]);

        let mut client = sign_in();
        client.extend([VERSION, CONNECT, 0, 0x09]);
        assert_eq!(run(&client).await.0, Err(Refusal::Unsupported));

        let mut client = sign_in();
        client.extend([VERSION, CONNECT, 0, IPV4, 1, 1, 1, 1, 0, 0]);
        assert_eq!(
            run(&client).await.0,
            Err(Refusal::Target(TargetError::Invalid))
        );

        // Cut short anywhere.
        let full = sign_in();
        for end in 0..full.len() {
            assert_eq!(run(&full[..end]).await.0, Err(Refusal::Malformed), "{end}");
        }
    }
}
