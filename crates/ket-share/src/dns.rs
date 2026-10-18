//! Just enough DNS to look up a name's IPv4 addresses through the tunnel's
//! resolver. The desktop share sends its lookups on a socket tied to the
//! tunnel instead of using the system resolver, so a lookup for a nearby
//! device can never leave through the local network, even in the moment the
//! tunnel goes down.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use rand::{Rng, rngs::OsRng};
use tokio::{net::UdpSocket, time::timeout};

use crate::egress::EgressError;

const TRIES: usize = 2;
const TRY_TIMEOUT: Duration = Duration::from_secs(2);
const HEADER: usize = 12;
const TYPE_A: u16 = 1;
const CLASS_IN: u16 = 1;

/// A recursive query for the A records of `name`, which must already be a
/// valid host name ([`crate::Host::parse`]).
pub fn query(id: u16, name: &str) -> Vec<u8> {
    let mut packet = Vec::with_capacity(HEADER + name.len() + 6);
    packet.extend(id.to_be_bytes());
    // Recursion desired; one question.
    packet.extend([0x01, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        packet.push(label.len() as u8);
        packet.extend(label.as_bytes());
    }
    packet.push(0);
    packet.extend(TYPE_A.to_be_bytes());
    packet.extend(CLASS_IN.to_be_bytes());
    packet
}

#[derive(Debug, Eq, PartialEq)]
pub enum Answer {
    Addresses(Vec<Ipv4Addr>),
    /// The name does not exist or has no IPv4 address.
    NotFound,
    /// Not the answer to this query (another ID or name), or unreadable.
    Ignored,
}

/// The A records in `response` if it answers the query `id` for `name`.
pub fn answer(id: u16, name: &str, response: &[u8]) -> Answer {
    read_answer(id, name, response).unwrap_or(Answer::Ignored)
}

fn read_answer(id: u16, name: &str, response: &[u8]) -> Option<Answer> {
    let header = response.get(..HEADER)?;
    if u16::from_be_bytes([header[0], header[1]]) != id || header[2] & 0x80 == 0 {
        return None;
    }
    let questions = u16::from_be_bytes([header[4], header[5]]);
    let answers = u16::from_be_bytes([header[6], header[7]]);
    if questions != 1 {
        return None;
    }
    let question = query(id, name);
    let asked = &question[HEADER..];
    if !response
        .get(HEADER..HEADER + asked.len())?
        .eq_ignore_ascii_case(asked)
    {
        return None;
    }
    match header[3] & 0x0f {
        0 => {}
        3 => return Some(Answer::NotFound),
        _ => return None,
    }
    let mut offset = HEADER + asked.len();
    let mut addresses = Vec::new();
    for _ in 0..answers {
        offset = skip_name(response, offset)?;
        let fixed = response.get(offset..offset + 10)?;
        let kind = u16::from_be_bytes([fixed[0], fixed[1]]);
        let class = u16::from_be_bytes([fixed[2], fixed[3]]);
        let length = usize::from(u16::from_be_bytes([fixed[8], fixed[9]]));
        offset += 10;
        let data = response.get(offset..offset + length)?;
        if kind == TYPE_A && class == CLASS_IN && length == 4 {
            addresses.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        offset += length;
    }
    Some(if addresses.is_empty() {
        Answer::NotFound
    } else {
        Answer::Addresses(addresses)
    })
}

/// Skips a name in an answer: labels to the root, or a compression pointer.
fn skip_name(packet: &[u8], mut offset: usize) -> Option<usize> {
    loop {
        let length = *packet.get(offset)?;
        match length {
            0 => return Some(offset + 1),
            pointer if pointer & 0xc0 == 0xc0 => {
                packet.get(offset + 1)?;
                return Some(offset + 2);
            }
            label if label & 0xc0 == 0 => offset += 1 + usize::from(label),
            _ => return None,
        }
    }
}

/// Looks `name` up on `socket`, which is connected to the tunnel's resolver
/// and tied to the tunnel. A reply from anywhere else is never read: a
/// connected UDP socket only receives from the address it is connected to.
pub async fn lookup(socket: &UdpSocket, name: &str) -> Result<Vec<IpAddr>, EgressError> {
    let id = OsRng.r#gen::<u16>();
    let packet = query(id, name);
    let mut buffer = [0; 1500];
    for _ in 0..TRIES {
        if socket.send(&packet).await.is_err() {
            return Err(EgressError::NoTunnel);
        }
        let attempt = timeout(TRY_TIMEOUT, async {
            loop {
                let read = socket
                    .recv(&mut buffer)
                    .await
                    .map_err(|_| EgressError::NoTunnel)?;
                match answer(id, name, &buffer[..read]) {
                    Answer::Addresses(addresses) => {
                        return Ok(addresses.into_iter().map(IpAddr::V4).collect());
                    }
                    Answer::NotFound => return Err(EgressError::Unresolved),
                    Answer::Ignored => {}
                }
            }
        })
        .await;
        if let Ok(result) = attempt {
            return result;
        }
    }
    Err(EgressError::TimedOut)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A response to `query(id, "example.com")` with a CNAME and two A
    /// records, written with compression pointers as resolvers do.
    fn response(id: u16, rcode: u8, records: &[(&[u8], u16, &[u8])]) -> Vec<u8> {
        let mut packet = query(id, "example.com");
        packet[2] = 0x81;
        packet[3] = 0x80 | rcode;
        packet[7] = records.len() as u8;
        for (name, kind, data) in records {
            packet.extend(*name);
            packet.extend(kind.to_be_bytes());
            packet.extend(CLASS_IN.to_be_bytes());
            packet.extend([0, 0, 1, 0]);
            packet.extend((data.len() as u16).to_be_bytes());
            packet.extend(*data);
        }
        packet
    }

    const POINTER: &[u8] = &[0xc0, 0x0c];

    #[test]
    fn queries_ask_for_ipv4_addresses_recursively() {
        assert_eq!(
            query(0x1234, "example.com"),
            [
                0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 7, b'e', b'x', b'a', b'm', b'p', b'l',
                b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1
            ]
        );
    }

    #[test]
    fn a_records_are_read_and_others_skipped() {
        let cname: &[u8] = &[3, b'w', b'w', b'w', 0xc0, 0x0c];
        let packet = response(
            7,
            0,
            &[
                (POINTER, 5, cname),
                (POINTER, TYPE_A, &[93, 184, 216, 34]),
                (
                    &[3, b'w', b'w', b'w', 0xc0, 0x0c],
                    TYPE_A,
                    &[93, 184, 216, 35],
                ),
                (POINTER, 28, &[0; 16]),
            ],
        );
        assert_eq!(
            answer(7, "example.com", &packet),
            Answer::Addresses(vec![
                Ipv4Addr::new(93, 184, 216, 34),
                Ipv4Addr::new(93, 184, 216, 35)
            ])
        );
        // Question names are compared without case.
        assert!(matches!(
            answer(7, "EXAMPLE.com", &packet),
            Answer::Addresses(_)
        ));
    }

    #[test]
    fn missing_names_are_reported() {
        assert_eq!(
            answer(7, "example.com", &response(7, 3, &[])),
            Answer::NotFound
        );
        assert_eq!(
            answer(
                7,
                "example.com",
                &response(7, 0, &[(POINTER, 28, &[0; 16])])
            ),
            Answer::NotFound
        );
    }

    #[test]
    fn other_answers_are_ignored() {
        let packet = response(7, 0, &[(POINTER, TYPE_A, &[93, 184, 216, 34])]);
        assert_eq!(answer(8, "example.com", &packet), Answer::Ignored);
        assert_eq!(answer(7, "example.org", &packet), Answer::Ignored);
        assert_eq!(
            answer(7, "example.com", &query(7, "example.com")),
            Answer::Ignored,
            "a query is not an answer"
        );
        assert_eq!(
            answer(7, "example.com", &response(7, 2, &[])),
            Answer::Ignored
        );
        for end in 0..packet.len() {
            assert_ne!(
                answer(7, "example.com", &packet[..end]),
                Answer::Addresses(vec![Ipv4Addr::new(93, 184, 216, 34)]),
                "{end}"
            );
        }
        let mut looping = response(7, 0, &[(&[0x80], TYPE_A, &[1, 1, 1, 1])]);
        assert_eq!(answer(7, "example.com", &looping), Answer::Ignored);
        looping.truncate(HEADER);
        assert_eq!(answer(7, "example.com", &looping), Answer::Ignored);
    }

    #[tokio::test]
    async fn lookups_wait_for_the_matching_answer() {
        let resolver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket
            .connect(resolver.local_addr().unwrap())
            .await
            .unwrap();
        let server = tokio::spawn(async move {
            let mut buffer = [0; 512];
            let (read, from) = resolver.recv_from(&mut buffer).await.unwrap();
            let id = u16::from_be_bytes([buffer[0], buffer[1]]);
            assert_eq!(&buffer[2..read], &query(id, "example.com")[2..]);
            // A stray reply first, then the real one.
            resolver
                .send_to(&response(id.wrapping_add(1), 0, &[]), from)
                .await
                .unwrap();
            resolver
                .send_to(
                    &response(id, 0, &[(POINTER, TYPE_A, &[93, 184, 216, 34])]),
                    from,
                )
                .await
                .unwrap();
        });
        assert_eq!(
            lookup(&socket, "example.com").await.unwrap(),
            vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]
        );
        server.await.unwrap();
    }
}
