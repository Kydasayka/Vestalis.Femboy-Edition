using System;
using System.Linq;
using System.Net.NetworkInformation;

namespace Gml.Launcher.Core.Services;

public class VpnChecker : IVpnChecker
{
    private readonly string[] VpnKeywords =
    [
        "WireGuard",
        "OpenVPN",
        "VLESS",
        "VPN",
        "PPTP",
        "L2TP",
        "IPSec",
        "IKEv2",
        "SoftEther",
        "SSTP",
        "VPNClient",
        "ExpressVPN",
        "NordVPN",
        "CyberGhost",
        "Surfshark",
        "PrivateInternetAccess",
        "ProtonVPN",
        "HideMyAss",
        "VyprVPN",
        "Tunnelbear",
        "Hotspot Shield",
        "StrongVPN",
        "SaferVPN",
        "PureVPN",
        "IVPN",
        "TorGuard"
    ];

    public bool IsUseVpnTunnel()
    {
        return false;
    }
}
