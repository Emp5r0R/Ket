//! Installs the leak-guard plan as Windows Filtering Platform filters in a
//! dynamic session: Windows deletes every filter when the session handle
//! closes, including when the process dies, so a crash cannot leave the
//! computer without DNS.

use std::{io, net::IpAddr, ptr};

use windows_sys::{
    Win32::{
        Foundation::{ERROR_SUCCESS, HANDLE},
        NetworkManagement::{
            IpHelper::ConvertInterfaceAliasToLuid,
            Ndis::NET_LUID_LH,
            WindowsFilteringPlatform::{
                FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_BYTE_ARRAY16, FWP_BYTE_ARRAY16_TYPE,
                FWP_CONDITION_FLAG_IS_LOOPBACK, FWP_CONDITION_VALUE0, FWP_MATCH_EQUAL,
                FWP_MATCH_FLAGS_ALL_SET, FWP_UINT8, FWP_UINT16, FWP_UINT32, FWP_UINT64,
                FWP_V6_ADDR_AND_MASK, FWP_V6_ADDR_MASK, FWP_VALUE0, FWPM_CONDITION_FLAGS,
                FWPM_CONDITION_IP_LOCAL_INTERFACE, FWPM_CONDITION_IP_REMOTE_ADDRESS,
                FWPM_CONDITION_IP_REMOTE_PORT, FWPM_FILTER_CONDITION0, FWPM_FILTER0,
                FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6,
                FWPM_SESSION_FLAG_DYNAMIC, FWPM_SESSION0, FWPM_SUBLAYER0, FwpmEngineClose0,
                FwpmEngineOpen0, FwpmFilterAdd0, FwpmSubLayerAdd0, FwpmTransactionAbort0,
                FwpmTransactionBegin0, FwpmTransactionCommit0,
            },
        },
        System::Rpc::RPC_C_AUTHN_WINNT,
    },
    core::GUID,
};

use ket_tunnel_protocol::IpPrefix;

use super::plan::{Action, Condition, Family, Rule, base_rules, tunnel_rules};

/// The sublayer holding Ket's filters; dynamic, so it leaves with the session.
const SUBLAYER_KEY: GUID = GUID::from_u128(0x6b657444_6e73_4c65_616b_477561726401);

pub(crate) struct PlatformGuard {
    engine: HANDLE,
}

// The engine handle is only used through &mut self or on drop.
unsafe impl Send for PlatformGuard {}
unsafe impl Sync for PlatformGuard {}

impl PlatformGuard {
    pub(crate) fn engage(servers: &[IpAddr], bypass: &[IpPrefix]) -> io::Result<Self> {
        let mut session = FWPM_SESSION0 {
            flags: FWPM_SESSION_FLAG_DYNAMIC,
            ..Default::default()
        };
        let mut name = wide("Ket leak guard");
        session.displayData.name = name.as_mut_ptr();
        let mut engine: HANDLE = ptr::null_mut();
        check(unsafe {
            FwpmEngineOpen0(
                ptr::null(),
                RPC_C_AUTHN_WINNT,
                ptr::null(),
                &session,
                &mut engine,
            )
        })?;
        let guard = Self { engine };
        guard.transaction(|engine| {
            let mut sublayer = FWPM_SUBLAYER0 {
                subLayerKey: SUBLAYER_KEY,
                weight: u16::MAX,
                ..Default::default()
            };
            let mut name = wide("Ket leak guard");
            sublayer.displayData.name = name.as_mut_ptr();
            check(unsafe { FwpmSubLayerAdd0(engine, &sublayer, ptr::null_mut()) })?;
            for rule in base_rules(servers, bypass) {
                add_filter(engine, &rule, None)?;
            }
            Ok(())
        })?;
        Ok(guard)
    }

    pub(crate) fn admit_tunnel(&mut self, interface: &str) -> io::Result<()> {
        let mut luid = NET_LUID_LH::default();
        let alias = wide(interface);
        let status = unsafe { ConvertInterfaceAliasToLuid(alias.as_ptr(), &mut luid) };
        if status != ERROR_SUCCESS {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the tunnel interface {interface} does not exist yet"),
            ));
        }
        let luid = unsafe { luid.Value };
        self.transaction(|engine| {
            for rule in tunnel_rules() {
                add_filter(engine, &rule, Some(luid))?;
            }
            Ok(())
        })
    }

    fn transaction(&self, body: impl FnOnce(HANDLE) -> io::Result<()>) -> io::Result<()> {
        check(unsafe { FwpmTransactionBegin0(self.engine, 0) })?;
        match body(self.engine) {
            Ok(()) => check(unsafe { FwpmTransactionCommit0(self.engine) }),
            Err(error) => {
                unsafe { FwpmTransactionAbort0(self.engine) };
                Err(error)
            }
        }
    }
}

impl Drop for PlatformGuard {
    fn drop(&mut self) {
        unsafe { FwpmEngineClose0(self.engine) };
    }
}

fn add_filter(engine: HANDLE, rule: &Rule, tunnel_luid: Option<u64>) -> io::Result<()> {
    // Values a condition points at must outlive FwpmFilterAdd0.
    let mut luid = tunnel_luid.unwrap_or_default();
    let mut address6 = FWP_BYTE_ARRAY16::default();
    let mut prefix6 = FWP_V6_ADDR_AND_MASK::default();
    let mut condition = FWPM_FILTER_CONDITION0 {
        matchType: FWP_MATCH_EQUAL,
        ..Default::default()
    };
    let mut value = FWP_CONDITION_VALUE0::default();
    let mut conditions = 1;
    match rule.condition {
        Condition::TunnelInterface => {
            if tunnel_luid.is_none() {
                return Err(io::Error::other("the tunnel interface is not known"));
            }
            condition.fieldKey = FWPM_CONDITION_IP_LOCAL_INTERFACE;
            value.r#type = FWP_UINT64;
            value.Anonymous.uint64 = &mut luid;
        }
        Condition::Loopback => {
            condition.fieldKey = FWPM_CONDITION_FLAGS;
            condition.matchType = FWP_MATCH_FLAGS_ALL_SET;
            value.r#type = FWP_UINT32;
            value.Anonymous.uint32 = FWP_CONDITION_FLAG_IS_LOOPBACK;
        }
        Condition::RemoteAddress(IpAddr::V4(address)) => {
            condition.fieldKey = FWPM_CONDITION_IP_REMOTE_ADDRESS;
            value.r#type = FWP_UINT32;
            value.Anonymous.uint32 = u32::from(address);
        }
        Condition::RemoteAddress(IpAddr::V6(address)) => {
            condition.fieldKey = FWPM_CONDITION_IP_REMOTE_ADDRESS;
            address6.byteArray16 = address.octets();
            value.r#type = FWP_BYTE_ARRAY16_TYPE;
            value.Anonymous.byteArray16 = &mut address6;
        }
        Condition::RemotePort(port) => {
            condition.fieldKey = FWPM_CONDITION_IP_REMOTE_PORT;
            value.r#type = FWP_UINT16;
            value.Anonymous.uint16 = port;
        }
        Condition::RemoteIpv6Prefix(prefix, length) => {
            condition.fieldKey = FWPM_CONDITION_IP_REMOTE_ADDRESS;
            prefix6.addr = prefix.octets();
            prefix6.prefixLength = length;
            value.r#type = FWP_V6_ADDR_MASK;
            value.Anonymous.v6AddrMask = &mut prefix6;
        }
        Condition::Everything => conditions = 0,
    }
    condition.conditionValue = value;

    let mut filter = FWPM_FILTER0 {
        layerKey: match rule.family {
            Family::V4 => FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            Family::V6 => FWPM_LAYER_ALE_AUTH_CONNECT_V6,
        },
        subLayerKey: SUBLAYER_KEY,
        numFilterConditions: conditions,
        filterCondition: if conditions == 0 {
            ptr::null_mut()
        } else {
            &mut condition
        },
        ..Default::default()
    };
    let mut name = wide("Ket leak guard");
    filter.displayData.name = name.as_mut_ptr();
    filter.weight = FWP_VALUE0 {
        r#type: FWP_UINT8,
        ..Default::default()
    };
    filter.weight.Anonymous.uint8 = rule.weight;
    filter.action.r#type = match rule.action {
        Action::Permit => FWP_ACTION_PERMIT,
        Action::Block => FWP_ACTION_BLOCK,
    };
    check(unsafe { FwpmFilterAdd0(engine, &filter, ptr::null_mut(), ptr::null_mut()) })
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn check(status: u32) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Windows Filtering Platform error 0x{status:08x}"
        )))
    }
}

#[cfg(test)]
mod tests {
    //! Real Filtering Platform behavior. Needs Administrator and the
    //! internet, so it is ignored by default; Windows CI runs it with
    //! `--include-ignored`.

    use std::{
        io::ErrorKind,
        net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
        process::Command,
        time::Duration,
    };

    use super::*;

    const PUBLIC_RESOLVER: &str = "1.1.1.1:53";
    const PUBLIC_V6: &str = "[2606:4700:4700::1111]:443";

    /// A DNS query for example.com (A) sent straight to a public resolver.
    fn direct_udp_lookup_answers() -> bool {
        let query: &[u8] = &[
            0x4b, 0x45, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 7, b'e', b'x', b'a', b'm', b'p', b'l',
            b'e', 3, b'c', b'o', b'm', 0, 0, 1, 0, 1,
        ];
        let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
            return false;
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        if socket.send_to(query, PUBLIC_RESOLVER).is_err() {
            return false;
        }
        let mut answer = [0u8; 512];
        matches!(socket.recv_from(&mut answer), Ok((length, _)) if length > 12 && answer[..2] == query[..2])
    }

    fn connect(address: &str) -> std::io::Result<TcpStream> {
        let address: SocketAddr = address.parse().unwrap();
        TcpStream::connect_timeout(&address, Duration::from_secs(5))
    }

    fn system_lookup_answers() -> bool {
        let _ = Command::new("ipconfig").arg("/flushdns").status();
        ("www.wikipedia.org", 443).to_socket_addrs().is_ok()
    }

    #[test]
    #[ignore = "needs Administrator and the internet; run by Windows CI"]
    fn the_guard_blocks_lookups_and_ipv6_outside_the_tunnel_until_released() {
        assert!(direct_udp_lookup_answers(), "precondition: DNS works");
        assert!(
            connect(PUBLIC_RESOLVER).is_ok(),
            "precondition: DNS over TCP"
        );
        assert!(system_lookup_answers(), "precondition: Windows resolves");
        let ipv6 = connect(PUBLIC_V6).is_ok();

        let mut guard = PlatformGuard::engage(&[], &[]).expect("engage the guard");
        assert!(!direct_udp_lookup_answers(), "UDP DNS left the computer");
        assert_eq!(
            connect(PUBLIC_RESOLVER).unwrap_err().kind(),
            ErrorKind::PermissionDenied,
            "TCP DNS left the computer"
        );
        assert!(
            !system_lookup_answers(),
            "Windows resolved a name through an adapter's resolver"
        );
        assert!(
            connect("1.1.1.1:443").is_ok(),
            "ordinary IPv4 must be left to the routes"
        );
        if ipv6 {
            assert_eq!(
                connect(PUBLIC_V6).unwrap_err().kind(),
                ErrorKind::PermissionDenied,
                "IPv6 left the computer"
            );
        } else {
            eprintln!("no IPv6 connectivity here; the IPv6 block was not exercised");
        }
        assert_eq!(
            guard.admit_tunnel("Ket test missing").unwrap_err().kind(),
            ErrorKind::NotFound
        );

        drop(guard);
        assert!(direct_udp_lookup_answers(), "DNS did not come back");
        assert!(
            system_lookup_answers(),
            "Windows resolution did not come back"
        );
    }

    #[test]
    #[ignore = "needs Administrator; run by Windows CI"]
    fn a_server_address_stays_reachable_under_the_guard() {
        let guard = PlatformGuard::engage(&["1.1.1.1".parse().unwrap()], &[]).expect("engage");
        assert!(connect(PUBLIC_RESOLVER).is_ok(), "the server was blocked");
        drop(guard);
    }
}
