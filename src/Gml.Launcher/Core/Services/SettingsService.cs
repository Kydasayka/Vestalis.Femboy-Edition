using System.Collections.Generic;
using System.Threading.Tasks;
using Gml.Launcher.Models;

namespace Gml.Launcher.Core.Services;

public class SettingsService(ISystemService system, IStorageService storage) : ISettingsService
{

    public async Task<SettingsInfo> GetSettings()
    {
        return await storage.GetAsync<SettingsInfo>(StorageConstants.Settings) ?? SettingsInfo.Default;
    }

    public Task UpdateSettingsAsync(SettingsInfo newSettings)
    {
        return storage.SetAsync(StorageConstants.Settings, newSettings);
    }

    public IEnumerable<Language> GetAvailableLanguages()
    {
        return system.GetAvailableLanguages();
    }

    public ulong GetMaxRam()
    {
        return system.GetMaxRam();
    }

    public Task UpdateInstallationDirectory(string? installationFolder)
    {
        return storage.SetAsync(StorageConstants.InstallationDirectory, installationFolder);
    }

    public async Task<string?> GetHwidForAuthAsync()
    {
        var settings = await GetSettings();

        // Sign-in is rejected when the X-HWID header is missing, so the launcher always sends a
        // value. Turning the setting off switches from the real serials to a random identifier
        // that is generated for every request, so a ban bound to one identifier does not stick.
        return settings.SendHwid ? system.GetHwid() : system.GetRandomHwid();
    }

    public async Task SetHwidSendingEnabledAsync(bool enabled)
    {
        var settings = await GetSettings();

        if (settings.SendHwid == enabled)
            return;

        settings.SendHwid = enabled;
        await UpdateSettingsAsync(settings);
    }
}
