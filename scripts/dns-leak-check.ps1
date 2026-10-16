#Requires -Version 5.1
# DNS and IPv6 leak check for a Windows computer while Ket is connected.
#
#   powershell -ExecutionPolicy Bypass -File scripts\dns-leak-check.ps1
#
# Windows asks every adapter's resolvers in parallel (Smart Multi-Homed Name
# Resolution), including link-local IPv6 resolvers that no route can steer.
# While Ket is connected none of them may be reachable:
#
#   1. A unique name resolves through the tunnel (Ket's virtual DNS answers
#      from 198.18.0.0/15); an answer from anywhere else means it leaked.
#   2. Every other adapter's resolver, IPv4 and IPv6 (link-local included),
#      is unreachable outside the tunnel: a direct query fails or is answered
#      by Ket's virtual DNS inside it.
#   3. IPv6 to the internet does not leave outside the tunnel.
#
# Exit status: 0 no leak, 1 leak, 2 the check could not run.
[CmdletBinding()]
param(
    [string]$TunnelInterface = "Ket"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Ipv6Probe = "2606:4700:4700::1111"
$Leaks = [System.Collections.Generic.List[string]]::new()

function Test-VirtualAnswer {
    param([string]$Address)
    $parsed = [System.Net.IPAddress]::Parse($Address)
    $bytes = $parsed.GetAddressBytes()
    return ($bytes.Length -eq 4 -and $bytes[0] -eq 198 -and ($bytes[1] -eq 18 -or $bytes[1] -eq 19))
}

if (-not (Get-NetAdapter -Name $TunnelInterface -ErrorAction SilentlyContinue)) {
    Write-Output "dns-leak-check: the $TunnelInterface tunnel adapter is not up; connect Ket first"
    exit 2
}

$Nonce = "ket-leak-" + ([guid]::NewGuid().ToString("N").Substring(0, 12))
Write-Output "Tunnel adapter: $TunnelInterface. Probe: $Nonce"

# 1. The system resolver answers through the tunnel. Ket's virtual DNS
#    answers every name with an address in 198.18.0.0/15; any other answer,
#    including "does not exist" or a record without an address, came from a
#    resolver outside the tunnel.
Clear-DnsClientCache
$records = $null
$lookupError = $null
try {
    $records = @(Resolve-DnsName -Name "$Nonce.example.com" -Type A -DnsOnly -ErrorAction Stop)
} catch {
    $lookupError = $_.Exception.Message
}
if ($null -ne $records) {
    $addresses = @($records | Where-Object { $_.PSObject.Properties.Name -contains "IPAddress" } |
        ForEach-Object { $_.IPAddress })
    $outside = @($addresses | Where-Object { -not (Test-VirtualAnswer $_) })
    if ($addresses.Count -eq 0) {
        $Leaks.Add("a resolver outside the tunnel answered the probe without an address")
    } elseif ($outside.Count -gt 0) {
        $Leaks.Add("the system resolver answered from outside the tunnel: $($addresses -join ', ')")
    } else {
        Write-Output "  ok    the system resolver answers through the tunnel ($($addresses[0]))"
    }
} elseif ($lookupError -match "does not exist") {
    $Leaks.Add("a resolver outside the tunnel answered the probe: $lookupError")
} else {
    Write-Output "  ok    no resolver outside the tunnel answered ($lookupError)"
}

# 2. No other adapter's resolver is reachable from that adapter's own address,
#    which is how Windows' parallel lookups reach it.
$Query = [byte[]](0x4b, 0x45, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0, 7) + [System.Text.Encoding]::ASCII.GetBytes("example") +
    [byte[]](3) + [System.Text.Encoding]::ASCII.GetBytes("com") + [byte[]](0, 0, 1, 0, 1)

# Send-DirectQuery LOCAL SERVER: the A answer (or "answered" without one) when
# the server replies to a query sent from LOCAL, $null when it cannot be reached.
function Send-DirectQuery {
    param([System.Net.IPAddress]$Local, [System.Net.IPAddress]$Server)
    $udp = [System.Net.Sockets.UdpClient]::new([System.Net.IPEndPoint]::new($Local, 0))
    try {
        $udp.Client.ReceiveTimeout = 3000
        [void]$udp.Send($Query, $Query.Length, [System.Net.IPEndPoint]::new($Server, 53))
        $from = [System.Net.IPEndPoint]::new([System.Net.IPAddress]::Any, 0)
        $reply = $udp.Receive([ref]$from)
        if ($reply.Length -lt 12 -or $reply[0] -ne $Query[0] -or $reply[1] -ne $Query[1]) { return $null }
        $answers = $reply[6] * 256 + $reply[7]
        if ($answers -gt 0) { return [System.Net.IPAddress]::new($reply[($reply.Length - 4)..($reply.Length - 1)]).ToString() }
        return "answered"
    } catch {
        return $null
    } finally {
        $udp.Dispose()
    }
}

$servers = Get-DnsClientServerAddress |
    Where-Object { $_.InterfaceAlias -ne $TunnelInterface -and $_.ServerAddresses.Count -gt 0 }
foreach ($entry in $servers) {
    $locals = @(Get-NetIPAddress -InterfaceIndex $entry.InterfaceIndex -ErrorAction SilentlyContinue)
    foreach ($server in $entry.ServerAddresses) {
        $serverAddress = [System.Net.IPAddress]::Parse($server)
        $linkLocal = $serverAddress.IsIPv6LinkLocal
        $local = $locals | Where-Object {
            $address = [System.Net.IPAddress]::Parse($_.IPAddress)
            $address.AddressFamily -eq $serverAddress.AddressFamily -and $address.IsIPv6LinkLocal -eq $linkLocal
        } | Select-Object -First 1
        if (-not $local) {
            Write-Output "  ok    $($entry.InterfaceAlias) has no address to reach resolver $server from"
            continue
        }
        $localAddress = [System.Net.IPAddress]::Parse($local.IPAddress)
        if ($linkLocal) {
            $serverAddress.ScopeId = $entry.InterfaceIndex
            $localAddress.ScopeId = $entry.InterfaceIndex
        }
        $answer = Send-DirectQuery -Local $localAddress -Server $serverAddress
        if ($null -eq $answer) {
            Write-Output "  ok    $($entry.InterfaceAlias) resolver $server is unreachable"
        } elseif ($answer -ne "answered" -and (Test-VirtualAnswer $answer)) {
            Write-Output "  ok    $($entry.InterfaceAlias) resolver $server is only reached through the tunnel"
        } else {
            $Leaks.Add("$($entry.InterfaceAlias) resolver $server answers outside the tunnel ($answer)")
        }
    }
}

# 3. IPv6 stays inside the tunnel or off.
$client = [System.Net.Sockets.TcpClient]::new([System.Net.Sockets.AddressFamily]::InterNetworkV6)
try {
    $connect = $client.ConnectAsync([System.Net.IPAddress]::Parse($Ipv6Probe), 443)
    if ($connect.Wait(5000) -and $client.Connected) {
        $Leaks.Add("IPv6 reaches the internet outside the tunnel ([$Ipv6Probe]:443)")
    } else {
        Write-Output "  ok    IPv6 does not leave outside the tunnel"
    }
} catch {
    Write-Output "  ok    IPv6 does not leave outside the tunnel"
} finally {
    $client.Dispose()
}

if ($Leaks.Count -gt 0) {
    Write-Output ""
    Write-Output "LEAK:"
    $Leaks | ForEach-Object { Write-Output "  $_" }
    exit 1
}
Write-Output ""
Write-Output "No leak: lookups and IPv6 stay inside the tunnel."
exit 0
