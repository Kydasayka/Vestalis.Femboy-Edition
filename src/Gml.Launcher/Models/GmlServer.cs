using System;
using System.Collections.Generic;
using System.Linq;
using Gml.Launcher.Assets;

namespace Gml.Launcher.Models;

public sealed record GmlServer(string Name, string Host);

public static class GmlServers
{
    /// <summary>
    /// Servers shown in the server selector. The server configured through
    /// <see cref="ResourceKeysDictionary.Host"/> is always placed first and is treated as the default.
    /// </summary>
    public static IReadOnlyList<GmlServer> All { get; } = BuildServers();

    public static GmlServer Default => All[0];

    public static GmlServer? FindByHost(string? host)
    {
        if (!Uri.TryCreate(host, UriKind.Absolute, out var targetUri))
            return null;

        return All.FirstOrDefault(server => SameHost(server.Host, targetUri));
    }

    private static IReadOnlyList<GmlServer> BuildServers()
    {
        var knownServers = new List<GmlServer>
        {
            new("PL1", ResourceKeysDictionary.ServerPl1Host),
            new("RU2", ResourceKeysDictionary.ServerRu2Host),
            new("RU1", ResourceKeysDictionary.ServerRu1Host),
            new("EU1", ResourceKeysDictionary.ServerEu1Host)
        };

        var configuredHost = ResourceKeysDictionary.Host;
        var defaultServer = knownServers.FirstOrDefault(server => SameHost(server.Host, configuredHost))
                            ?? new GmlServer("Custom", configuredHost);

        var servers = new List<GmlServer> { defaultServer };
        servers.AddRange(knownServers.Where(server => !SameHost(server.Host, defaultServer.Host)));

        return servers;
    }

    private static bool SameHost(string host, Uri targetUri)
    {
        return Uri.TryCreate(host, UriKind.Absolute, out var serverUri) && serverUri == targetUri;
    }

    private static bool SameHost(string firstHost, string secondHost)
    {
        return Uri.TryCreate(firstHost, UriKind.Absolute, out var firstUri)
               && Uri.TryCreate(secondHost, UriKind.Absolute, out var secondUri)
               && firstUri == secondUri;
    }
}
