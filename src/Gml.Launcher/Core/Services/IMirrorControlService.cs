using System.Threading.Tasks;

namespace Gml.Launcher.Core.Services;

/// <summary>State reported by <c>GET /api/v1/mirror/mode</c>.</summary>
public sealed record MirrorModeState(string Mode, string? Upstream, bool SyncEnabled, bool CanControl);

/// <summary>
/// Optional service access to the mirror backend. The control key is entered by hand in the
/// settings and stored only in the local launcher database, never in the repository, so other
/// users of the build do not get the ability to switch the backend mode.
/// </summary>
public interface IMirrorControlService
{
    /// <summary>True when a stored key was accepted by the backend.</summary>
    bool IsUnlocked { get; }

    /// <summary>True when a key is stored locally, even if the backend no longer accepts it.</summary>
    bool HasKey { get; }

    /// <summary>Loads the saved key and validates it against the backend. Safe to call on startup.</summary>
    Task LoadAsync();

    /// <summary>Validates the key against the backend and persists it only when it is accepted.</summary>
    Task<bool> SaveKeyAsync(string? key);

    /// <summary>Forgets the stored key and locks the controls.</summary>
    Task ClearKeyAsync();

    /// <summary>Reads the current mode. Returns null when the server has no mode endpoint or is unreachable.</summary>
    Task<MirrorModeState?> GetStateAsync();

    /// <summary>Switches the backend to <paramref name="mode"/> ("local" or "proxy").</summary>
    Task<(bool Ok, string Message)> SetModeAsync(string mode);
}
