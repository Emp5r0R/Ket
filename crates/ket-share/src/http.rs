//! The HTTP proxy front door: `CONNECT` for HTTPS and everything else that
//! tunnels, and plain `http://` requests forwarded one per connection.
//! Sign-in is HTTP Basic (`Proxy-Authorization`), which browsers ask for on
//! their own when the share answers 407.

use base64::{Engine, engine::general_purpose::STANDARD};
use tokio::io::{AsyncRead, AsyncReadExt};
use zeroize::Zeroizing;

use crate::{
    credentials::Credentials,
    handshake::Refusal,
    target::{Host, Target, TargetError},
};

/// Request and response heads larger than this are refused; real ones,
/// cookies included, are far smaller.
pub(crate) const MAX_HEAD: usize = 16 * 1024;
const END_OF_HEAD: &[u8] = b"\r\n\r\n";
/// Headers that belong to the hop between the device and the share, never
/// passed to the site. `Proxy-Authorization` carries the share's password.
const HOP_HEADERS: [&str; 4] = [
    "proxy-authorization",
    "proxy-connection",
    "connection",
    "keep-alive",
];

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    /// `CONNECT`: once connected, bytes pass through untouched.
    Tunnel,
    /// A plain request, rewritten for the site: origin-form target, no
    /// hop-by-hop headers, and `Connection: close`.
    Forward(Vec<u8>),
}

pub(crate) struct Request {
    pub target: Target,
    pub kind: Kind,
    authorization: Option<(Vec<u8>, Zeroizing<Vec<u8>>)>,
}

impl Request {
    pub(crate) fn authorize(&self, credentials: &Credentials) -> Result<(), Refusal> {
        match &self.authorization {
            None => Err(Refusal::NoCredentials),
            Some((username, password)) if credentials.matches(username, password) => Ok(()),
            Some(_) => Err(Refusal::BadCredentials),
        }
    }
}

/// Reads up to the blank line that ends a head, starting with `first`, which
/// the caller has already read. Returns the head and whatever followed it.
pub(crate) async fn read_head<S>(
    stream: &mut S,
    first: Option<u8>,
) -> Result<(Vec<u8>, Vec<u8>), Refusal>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = Vec::with_capacity(1024);
    buffer.extend(first);
    let mut chunk = [0; 2048];
    loop {
        if let Some(end) = find(&buffer, END_OF_HEAD) {
            let rest = buffer.split_off(end + END_OF_HEAD.len());
            return Ok((buffer, rest));
        }
        if buffer.len() > MAX_HEAD {
            return Err(Refusal::Malformed);
        }
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| Refusal::Malformed)?;
        if read == 0 {
            return Err(Refusal::Malformed);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

pub(crate) fn parse_request(head: &[u8]) -> Result<Request, Refusal> {
    let text = std::str::from_utf8(head).map_err(|_| Refusal::Malformed)?;
    let mut lines = text
        .strip_suffix("\r\n\r\n")
        .ok_or(Refusal::Malformed)?
        .split("\r\n");
    let request_line = lines.next().ok_or(Refusal::Malformed)?;
    let mut parts = request_line.split(' ');
    let (Some(method), Some(uri), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal::Malformed);
    };
    if method.is_empty() || !method.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(Refusal::Malformed);
    }
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(Refusal::Malformed);
    }
    let headers = lines.map(header).collect::<Result<Vec<_>, _>>()?;
    let authorization = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("proxy-authorization"))
        .map(|(_, value)| basic(value))
        .transpose()?;

    if method == "CONNECT" {
        let target = authority(uri, None)?;
        return Ok(Request {
            target,
            kind: Kind::Tunnel,
            authorization,
        });
    }
    let rest = strip_prefix_ignore_case(uri, "http://").ok_or(Refusal::Unsupported)?;
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (host_port, path) = rest.split_at(split);
    let target = authority(host_port, Some(80))?;
    let path = match path {
        "" => "/".to_owned(),
        query if query.starts_with('?') => format!("/{query}"),
        path => path.to_owned(),
    };
    let mut forwarded = format!("{method} {path} {version}\r\n");
    let mut has_host = false;
    for (name, value) in &headers {
        if HOP_HEADERS.iter().any(|hop| name.eq_ignore_ascii_case(hop)) {
            continue;
        }
        has_host |= name.eq_ignore_ascii_case("host");
        forwarded.push_str(&format!("{name}: {value}\r\n"));
    }
    if !has_host {
        forwarded.push_str(&format!("Host: {host_port}\r\n"));
    }
    forwarded.push_str("Connection: close\r\n\r\n");
    Ok(Request {
        target,
        kind: Kind::Forward(forwarded.into_bytes()),
        authorization,
    })
}

/// The site's response head with `Connection: close`, so the device opens a
/// new connection (and so a new check of where it goes) for its next request.
pub(crate) fn rewrite_response(head: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(head).ok()?.strip_suffix("\r\n\r\n")?;
    let mut lines = text.split("\r\n");
    let status = lines.next().filter(|line| line.starts_with("HTTP/1."))?;
    let mut rewritten = format!("{status}\r\n");
    for line in lines {
        let (name, _) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("connection") || name.eq_ignore_ascii_case("keep-alive") {
            continue;
        }
        rewritten.push_str(line);
        rewritten.push_str("\r\n");
    }
    rewritten.push_str("Connection: close\r\n\r\n");
    Some(rewritten.into_bytes())
}

/// A short response that ends the conversation.
pub(crate) fn response(status: Status) -> Vec<u8> {
    let (code, reason, text) = match status {
        Status::Established => return b"HTTP/1.1 200 Connection established\r\n\r\n".to_vec(),
        Status::SignInRequired => (
            407,
            "Proxy Authentication Required",
            "Sign in with the user name and password Ket shows.\n",
        ),
        Status::BadRequest => (400, "Bad Request", "Ket could not read this request.\n"),
        Status::NotForwarded => (
            400,
            "Bad Request",
            "This is a Ket share. Set it as the proxy in your device's Wi-Fi or proxy settings instead of opening it here.\n",
        ),
        Status::NotAllowed => (
            403,
            "Forbidden",
            "Ket shares the Internet, not this device or its network.\n",
        ),
        Status::NotConnected => (
            503,
            "Service Unavailable",
            "Ket is not connected right now. Try again in a moment.\n",
        ),
        Status::BadGateway => (
            502,
            "Bad Gateway",
            "The site could not be reached through Ket.\n",
        ),
        Status::GatewayTimeout => (504, "Gateway Timeout", "The site did not answer in time.\n"),
    };
    let challenge = if status == Status::SignInRequired {
        "Proxy-Authenticate: Basic realm=\"Ket\", charset=\"UTF-8\"\r\n"
    } else {
        ""
    };
    format!(
        "HTTP/1.1 {code} {reason}\r\n{challenge}Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    )
    .into_bytes()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Status {
    Established,
    SignInRequired,
    BadRequest,
    /// A request for the share itself (a browser opening its address), or
    /// `https://` without `CONNECT`.
    NotForwarded,
    NotAllowed,
    NotConnected,
    BadGateway,
    GatewayTimeout,
}

fn header(line: &str) -> Result<(&str, &str), Refusal> {
    let (name, value) = line.split_once(':').ok_or(Refusal::Malformed)?;
    // A leading space would be an obsolete folded line.
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    {
        return Err(Refusal::Malformed);
    }
    Ok((name, value.trim_matches([' ', '\t'])))
}

fn basic(value: &str) -> Result<(Vec<u8>, Zeroizing<Vec<u8>>), Refusal> {
    let (scheme, encoded) = value.split_once(' ').ok_or(Refusal::Malformed)?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return Err(Refusal::Malformed);
    }
    let decoded = Zeroizing::new(
        STANDARD
            .decode(encoded.trim())
            .map_err(|_| Refusal::Malformed)?,
    );
    let colon = decoded
        .iter()
        .position(|&byte| byte == b':')
        .ok_or(Refusal::Malformed)?;
    Ok((
        decoded[..colon].to_vec(),
        Zeroizing::new(decoded[colon + 1..].to_vec()),
    ))
}

/// `host:port` (with IPv6 in brackets), or `host` alone when there is a
/// default port.
fn authority(text: &str, default_port: Option<u16>) -> Result<Target, Refusal> {
    if text.contains('@') {
        return Err(Refusal::Malformed);
    }
    let (host, port) = match text.rfind(':') {
        Some(colon) if !text[colon..].contains(']') => {
            let digits = &text[colon + 1..];
            if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(Refusal::Malformed);
            }
            (
                &text[..colon],
                digits.parse::<u16>().map_err(|_| Refusal::Malformed)?,
            )
        }
        _ => (text, default_port.ok_or(Refusal::Malformed)?),
    };
    if port == 0 {
        return Err(Refusal::Target(TargetError::Invalid));
    }
    // An IPv6 address must be in brackets here, or its colons would be read
    // as a port.
    if host.contains(':') && !host.starts_with('[') {
        return Err(Refusal::Malformed);
    }
    let host = Host::parse(host).map_err(Refusal::Target)?;
    Ok(Target { host, port })
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncWriteExt, duplex};

    use super::*;

    const SIGN_IN: &str = "Proxy-Authorization: Basic a2V0OmFiY2RlZmdoamttbg==\r\n";

    fn credentials() -> Credentials {
        Credentials::new("ket", "abcdefghjkmn")
    }

    fn parse(head: &str) -> Result<Request, Refusal> {
        parse_request(head.as_bytes())
    }

    fn name(host: &str, port: u16) -> Target {
        Target {
            host: Host::Name(host.to_owned()),
            port,
        }
    }

    #[test]
    fn connect_names_the_target_and_checks_the_password() {
        let request = parse(&format!(
            "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n{SIGN_IN}\r\n"
        ))
        .unwrap();
        assert_eq!(request.target, name("example.com", 443));
        assert_eq!(request.kind, Kind::Tunnel);
        assert_eq!(request.authorize(&credentials()), Ok(()));

        let request = parse("CONNECT [2606:4700::1]:443 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(
            request.target.host,
            Host::Address("2606:4700::1".parse().unwrap())
        );
        assert_eq!(
            request.authorize(&credentials()),
            Err(Refusal::NoCredentials)
        );

        let wrong = format!(
            "CONNECT example.com:443 HTTP/1.1\r\nProxy-Authorization: Basic {}\r\n\r\n",
            STANDARD.encode("ket:abcdefghjkmp")
        );
        assert_eq!(
            parse(&wrong).unwrap().authorize(&credentials()),
            Err(Refusal::BadCredentials)
        );
    }

    #[test]
    fn plain_requests_are_rewritten_for_the_site() {
        let request = parse(&format!(
            "GET http://Example.com/a/b?c=d HTTP/1.1\r\nHost: example.com\r\n{SIGN_IN}Proxy-Connection: keep-alive\r\nConnection: keep-alive\r\nKeep-Alive: 5\r\nAccept: */*\r\n\r\n"
        ))
        .unwrap();
        assert_eq!(request.target, name("example.com", 80));
        assert_eq!(
            request.kind,
            Kind::Forward(b"GET /a/b?c=d HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\nConnection: close\r\n\r\n".to_vec())
        );

        let request =
            parse("POST http://example.com:8080?x HTTP/1.0\r\nContent-Length: 0\r\n\r\n").unwrap();
        assert_eq!(request.target, name("example.com", 8080));
        assert_eq!(
            request.kind,
            Kind::Forward(b"POST /?x HTTP/1.0\r\nContent-Length: 0\r\nHost: example.com:8080\r\nConnection: close\r\n\r\n".to_vec())
        );
    }

    #[test]
    fn requests_the_share_does_not_forward() {
        assert_eq!(
            parse("GET https://example.com/ HTTP/1.1\r\n\r\n").err(),
            Some(Refusal::Unsupported)
        );
        assert_eq!(
            parse("GET /index.html HTTP/1.1\r\n\r\n").err(),
            Some(Refusal::Unsupported)
        );
        assert_eq!(
            parse("CONNECT localhost:22 HTTP/1.1\r\n\r\n").err(),
            Some(Refusal::Target(TargetError::LocalName))
        );
        assert_eq!(
            parse("CONNECT example.com:0 HTTP/1.1\r\n\r\n").err(),
            Some(Refusal::Target(TargetError::Invalid))
        );
        for head in [
            "CONNECT example.com HTTP/1.1\r\n\r\n",
            "CONNECT example.com:443x HTTP/1.1\r\n\r\n",
            "CONNECT example.com:70000 HTTP/1.1\r\n\r\n",
            "CONNECT 2606:4700::1:443 HTTP/1.1\r\n\r\n",
            "CONNECT user@example.com:443 HTTP/1.1\r\n\r\n",
            "GET http://user:pass@example.com/ HTTP/1.1\r\n\r\n",
            "get http://example.com/ HTTP/1.1\r\n\r\n",
            "GET http://example.com/ HTTP/2\r\n\r\n",
            "GET  http://example.com/ HTTP/1.1\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\n folded\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nNo colon\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nProxy-Authorization: Bearer abc\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nProxy-Authorization: Basic !!!\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nProxy-Authorization: Basic a2V0\r\n\r\n",
        ] {
            assert_eq!(parse(head).err(), Some(Refusal::Malformed), "{head:?}");
        }
    }

    #[test]
    fn site_responses_close_after_one_request() {
        assert_eq!(
            rewrite_response(b"HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nKeep-Alive: timeout=5\r\nContent-Length: 2\r\n\r\n").unwrap(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n".to_vec()
        );
        assert_eq!(rewrite_response(b"SSH-2.0-OpenSSH\r\n\r\n"), None);
    }

    #[test]
    fn sign_in_challenges_name_basic() {
        let text = String::from_utf8(response(Status::SignInRequired)).unwrap();
        assert!(text.starts_with("HTTP/1.1 407 "));
        assert!(text.contains("Proxy-Authenticate: Basic realm=\"Ket\""));
        assert!(text.contains("Connection: close"));
        assert!(
            !String::from_utf8(response(Status::NotConnected))
                .unwrap()
                .contains("Proxy-Authenticate")
        );
    }

    #[tokio::test]
    async fn heads_are_read_to_the_blank_line_and_no_further() {
        let (mut near, mut share) = duplex(MAX_HEAD * 2);
        near.write_all(b"ONNECT a.com:443 HTTP/1.1\r\n\r\nhello")
            .await
            .unwrap();
        let (head, rest) = read_head(&mut share, Some(b'C')).await.unwrap();
        assert_eq!(head, b"CONNECT a.com:443 HTTP/1.1\r\n\r\n");
        assert_eq!(rest, b"hello");

        near.write_all(&vec![b'a'; MAX_HEAD + 10]).await.unwrap();
        assert_eq!(
            read_head(&mut share, None).await.err(),
            Some(Refusal::Malformed)
        );

        near.write_all(b"GET http://a.com/ HTTP/1.1\r\n")
            .await
            .unwrap();
        drop(near);
        assert_eq!(
            read_head(&mut share, None).await.err(),
            Some(Refusal::Malformed)
        );
    }
}
