using System.Collections.Generic;
using System.Threading.Tasks;
using Gml.Launcher.Models;

namespace Gml.Launcher.Core.Services;

public interface ISettingsService
{
    public Task<SettingsInfo> GetSettings();
    Task UpdateSettingsAsync(SettingsInfo newSettings);
    IEnumerable<Language> GetAvailableLanguages();
    ulong GetMaxRam();
    Task UpdateInstallationDirectory(string? installationFolder);

    /// <summary>
    /// Returns the hardware ID to send with a sign-in request, or <c>null</c> when the user
    /// disabled HWID reporting in the settings.
    /// </summary>
    Task<string?> GetHwidForAuthAsync();

    /// <summary>Persists the HWID reporting preference without touching the other settings.</summary>
    Task SetHwidSendingEnabledAsync(bool enabled);
}
