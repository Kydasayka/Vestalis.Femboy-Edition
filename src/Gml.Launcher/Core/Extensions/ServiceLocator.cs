using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Threading;
using Avalonia;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Services;
using Gml.Launcher.Models;
using Sentry;
using Splat;

namespace Gml.Launcher.Core.Extensions;

public static class ServiceLocator
{
    public static AppBuilder RegisterServices(this AppBuilder builder)
    {
        var systemService = new SystemService();

        var installationDirectory =
            Path.Combine(systemService.GetApplicationFolder(), ResourceKeysDictionary.FolderName);

        RegisterLocalizationService();
        RegisterSystemService(systemService);
        RegisterLogHelper(systemService);
        var manager = RegisterGmlManager(systemService, installationDirectory);
        var storageService = RegisterStorage();

        var settingsService = new SettingsService(systemService, storageService);
        Locator.CurrentMutable.RegisterConstant(settingsService, typeof(ISettingsService));

        Locator.CurrentMutable.RegisterConstant(
            new AutoLoginService(storageService, manager, settingsService),
            typeof(IAutoLoginService));

        if (!ApplySavedHost(storageService, manager))
            manager.ChangeHost(
                GmlClientManager.CheckApiStatus(ResourceKeysDictionary.Host, ResourceKeysDictionary.SecondaryHost));

        CheckAndChangeInstallationFolder(storageService, manager);
        CheckAndChangeLanguage(storageService, systemService);
        ApplySoundSettings(storageService);
        Locator.CurrentMutable.RegisterConstant(new VpnChecker(), typeof(IVpnChecker));

        var mirrorControl = new MirrorControlService(storageService, manager);
        Locator.CurrentMutable.RegisterConstant(mirrorControl, typeof(IMirrorControlService));

        // Restore the optional service key in the background so the badge knows whether
        // mode switching is available without opening the settings first.
        _ = mirrorControl.LoadAsync();

        Locator.CurrentMutable.RegisterConstant(new BackendChecker(), typeof(IBackendChecker));

        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            SentrySdk.CaptureException((Exception)args.ExceptionObject);
        };

        Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Configuring ended");

        return builder;
    }

    private static void RegisterLogHelper(SystemService systemService)
    {
        Locator.CurrentMutable.RegisterConstant(new LogHandler());
    }

    private static void CheckAndChangeLanguage(LocalStorageService storageService, SystemService systemService)
    {
        var data = storageService.GetAsync<SettingsInfo>(StorageConstants.Settings).Result;

        if (data != null && !string.IsNullOrEmpty(data.LanguageCode))
            Assets.Resources.Resources.Culture = systemService
                .GetAvailableLanguages()
                .FirstOrDefault(c => c.Culture.Name == data.LanguageCode)?
                .Culture ?? new CultureInfo("ru-RU");
    }

    private static LocalStorageService RegisterStorage()
    {
        var storageService = new LocalStorageService();
        Locator.CurrentMutable.RegisterConstant(storageService, typeof(IStorageService));

        return storageService;
    }

    /// <summary>
    /// Restores the interface-sound preference before the first page is shown, so the very first
    /// click already follows the user's choice.
    /// </summary>
    private static void ApplySoundSettings(LocalStorageService storageService)
    {
        try
        {
            var settings = storageService.GetAsync<SettingsInfo>(StorageConstants.Settings).Result;
            SoundService.IsEnabled = settings?.InterfaceSounds ?? true;
        }
        catch (Exception exception)
        {
            Debug.WriteLine(
                $"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Failed to read sound settings: {exception.Message}");

            SoundService.IsEnabled = true;
        }
    }

    private static void CheckAndChangeInstallationFolder(LocalStorageService storageService, GmlClientManager manager)
    {
        var installationDirectory = storageService.GetAsync<string>(StorageConstants.InstallationDirectory).Result;

        if (!string.IsNullOrEmpty(installationDirectory)) manager.ChangeInstallationFolder(installationDirectory);
    }

    private static bool ApplySavedHost(LocalStorageService storageService, GmlClientManager manager)
    {
        try
        {
            var savedHost = storageService.GetAsync<string>(StorageConstants.ServerHost).Result;

            if (string.IsNullOrWhiteSpace(savedHost))
                return false;

            // The saved address may no longer be served: PL1 used to be addressed by its bare
            // IP, which its certificate does not cover. Fall back to the configured default
            // instead of trying to reach a host the backend cannot authenticate.
            if (GmlServers.FindByHost(savedHost) is not { } server)
            {
                Debug.WriteLine(
                    $"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Ignoring outdated saved server '{savedHost}'");

                storageService.SetAsync<string?>(StorageConstants.ServerHost, null).Wait();
                return false;
            }

            manager.ChangeHost(server.Host);
            return true;
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Failed to apply saved server: {exception.Message}");
            return false;
        }
    }

    private static GmlClientManager RegisterGmlManager(SystemService systemService, string installationDirectory)
    {
        var gateWay = ResourceKeysDictionary.Host;

        var manager = new GmlClientManager(
            installationDirectory,
            gateWay,
            new GameLoader(),
            ResourceKeysDictionary.FolderName,
            systemService.GetOsType());

        Locator.CurrentMutable.RegisterConstant(manager, typeof(IGmlClientManager));

        return manager;
    }

    private static void RegisterSystemService(SystemService systemService)
    {
        Locator.CurrentMutable.RegisterConstant(systemService, typeof(ISystemService));

    }

    private static void RegisterLocalizationService()
    {
        var service = new ResourceLocalizationService();
        Locator.CurrentMutable.RegisterConstant(service, typeof(ILocalizationService));
    }
}
