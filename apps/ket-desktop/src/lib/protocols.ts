export type ProtocolId =
  | "hysteria2"
  | "stealth"
  | "vless_xtls_reality"
  | "shadowsocks2022"
  | "wire_guard"
  | "open_vpn_stunnel";

export type ProtocolPreference = "auto" | ProtocolId;

export interface ProtocolInfo {
  id: ProtocolId;
  label: string;
  shortInstruction: string;
  bestFor: string;
  operation: string;
  steps: string[];
  limitations: string[];
}

export const protocolCatalog: ProtocolInfo[] = [
  {
    id: "stealth",
    label: "HTTPS Stealth",
    shortInstruction: "Use on strict networks that still allow normal HTTPS. Ket falls back if this path is unavailable.",
    bestFor: "HTTP-only networks, filtered Wi-Fi, and Cloudflare-carried deployments.",
    operation: "Carries VLESS through XHTTP packet-up inside certificate-verified browser-shaped TLS.",
    steps: ["Select HTTPS Stealth.", "Connect normally.", "Use Auto if the network blocks the server's CDN path."],
    limitations: ["Requires the server's HTTPS/CDN route.", "Usually adds more overhead than a direct transport."],
  },
  {
    id: "wire_guard",
    label: "WireGuard TLS",
    shortInstruction: "Use for a fast tunnel that resembles WebSocket HTTPS traffic. Ket keeps automatic fallback enabled.",
    bestFor: "Networks that allow WebSockets but identify or block native VPN handshakes.",
    operation: "Carries WireGuard packets through a certificate-verified WebSocket/TLS transport.",
    steps: ["Select WireGuard TLS.", "Connect and allow the initial route test.", "Switch to HTTPS Stealth if WebSockets are blocked."],
    limitations: ["Requires WebSocket support on the network and server edge.", "Android support currently requires a compatible 64-bit build."],
  },
  {
    id: "hysteria2",
    label: "Hysteria 2",
    shortInstruction: "Use when UDP is available, especially on lossy or high-latency links.",
    bestFor: "Mobile links, long-distance servers, and unstable networks that permit UDP.",
    operation: "Uses QUIC congestion control with authenticated leases and optional Salamander or Gecko obfuscation.",
    steps: ["Select Hysteria 2.", "Connect while UDP 443 is reachable.", "Choose a TLS transport if the network blocks UDP."],
    limitations: ["Cannot cross networks that block UDP.", "Obfuscation hides QUIC shape but is not separate encryption."],
  },
  {
    id: "vless_xtls_reality",
    label: "VLESS + REALITY",
    shortInstruction: "Use for a direct, low-overhead TCP route with a TLS-like handshake.",
    bestFor: "Networks that allow direct TCP but probe or classify ordinary proxy handshakes.",
    operation: "Uses Xray VLESS with REALITY authentication and a browser TLS fingerprint over raw TCP.",
    steps: ["Select VLESS + REALITY.", "Connect through the server's direct hostname.", "Use HTTPS Stealth when direct TCP ports are filtered."],
    limitations: ["Ordinary HTTP CDNs and Cloudflare Tunnel cannot carry it.", "The configured REALITY target must remain reachable."],
  },
  {
    id: "shadowsocks2022",
    label: "Shadowsocks 2022",
    shortInstruction: "Use for a lightweight direct route when its assigned TCP/UDP port is reachable.",
    bestFor: "Lower-overhead direct connections and networks without aggressive active probing.",
    operation: "Uses SIP022 AEAD-2022 keys and a lease-specific TCP/UDP server port.",
    steps: ["Select Shadowsocks 2022.", "Connect through the assigned direct port.", "Use a TLS-shaped protocol under stronger filtering."],
    limitations: ["Requires a dedicated public port per active lease.", "Its wire shape can be easier to classify than HTTPS Stealth."],
  },
  {
    id: "open_vpn_stunnel",
    label: "OpenVPN TLS",
    shortInstruction: "Use for compatibility when modern transports fail; expect lower TCP performance.",
    bestFor: "Compatibility-focused devices and networks that permit generic direct TLS.",
    operation: "Runs authenticated OpenVPN inside a second certificate-pinned stunnel TLS layer.",
    steps: ["Select OpenVPN TLS.", "Connect and wait for both TLS handshakes.", "Prefer HTTPS Stealth on HTTP-only networks."],
    limitations: ["TCP inside TCP can reduce throughput on lossy links.", "Ordinary HTTP CDNs cannot forward this generic TLS stream."],
  },
];

export const autoProtocol = {
  label: "Automatic",
  shortInstruction: "Tests the healthiest server-offered routes and changes protocol when the current network blocks one.",
};

export function protocolInfo(id: ProtocolId): ProtocolInfo {
  const info = protocolCatalog.find((item) => item.id === id);
  if (!info) throw new Error(`Unsupported protocol: ${id}`);
  return info;
}

export function isProtocolId(value: string): value is ProtocolId {
  return protocolCatalog.some((item) => item.id === value);
}
