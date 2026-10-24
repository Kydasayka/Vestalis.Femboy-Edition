using Avalonia;
using Avalonia.Media;
using GamerVII.Notification.Avalonia;
using Gml.Client.Interfaces;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Exceptions;
using Gml.Launcher.Core.Services;
using Gml.Launcher.Models;
using Gml.Launcher.ViewModels.Base;
using Gml.Launcher.ViewModels.Pages;
using ReactiveUI;
using ReactiveUI.Fody.Helpers;
using Sentry;
using Splat;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Reactive.Concurrency;
using System.Reactive.Subjects;
using System.Threading.Tasks;

namespace Gml.Launcher.ViewModels;

public class MainWindowViewModel : WindowViewModelBase, IScreen
{
    private readonly IVpnChecker _vpnChecker;
    private readonly ILocalizationService _localizationService;
    private readonly IBackendChecker _backendChecker;
    private readonly IGmlClientManager _gmlClientManager;
    private readonly IStorageService _storageService;
    private readonly IAutoLoginService _autoLoginService;
    private bool _isServerSwitchRunning;
    private GmlServer _selectedServer = GmlServers.Default;
    protected internal readonly Subject<bool> _gameLaunched = new();

    public MainWindowViewModel(IVpnChecker? vpnChecker = null, ILocalizationService? localizationService = null,
        IBackendChecker? backendChecker = null, IGmlClientManager? gmlClientManager = null,
        IStorageService? storageService = null, IAutoLoginService? autoLoginService = null)
    {
        _vpnChecker = vpnChecker ?? Locator.Current.GetService<IVpnChecker>()
            ?? throw new ServiceNotFoundException(typeof(IVpnChecker));

        _localizationService = localizationService ?? Locator.Current.GetService<ILocalizationService>()
            ?? throw new ServiceNotFoundException(typeof(ILocalizationService));

        _backendChecker = backendChecker ?? Locator.Current.GetService<IBackendChecker>()
            ?? throw new ServiceNotFoundException(typeof(IBackendChecker));

        _gmlClientManager = gmlClientManager ?? Locator.Current.GetService<IGmlClientManager>()
            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));

        _storageService = storageService ?? Locator.Current.GetService<IStorageService>()
            ?? throw new ServiceNotFoundException(typeof(IStorageService));

        _autoLoginService = autoLoginService ?? Locator.Current.GetService<IAutoLoginService>()
            ?? throw new ServiceNotFoundException(typeof(IAutoLoginService));

        Servers = GmlServers.All;
        _selectedServer = GmlServers.FindByHost(_gmlClientManager.HostUri.ToString()) ?? GmlServers.Default;

        Router.Navigate.Execute(new LoginPageViewModel(this, OnClosed));

        RxApp.MainThreadScheduler.Schedule(TimeSpan.FromSeconds(2), CheckBackend);

        RxApp.MainThreadScheduler.Schedule(TimeSpan.FromSeconds(4), CheckSystem);
    }

    public INotificationMessageManager Manager { get; } = new NotificationMessageManager();
    protected internal IObservable<bool> GameLaunched => _gameLaunched;
    public RoutingState Router { get; } = new();

    public IReadOnlyList<GmlServer> Servers { get; }

    [Reactive] public bool IsServerSwitchInProgress { get; private set; }

    public GmlServer SelectedServer
    {
        get => _selectedServer;
        set
        {
            if (value is null || SameHost(value, _selectedServer))
                return;

            if (_isServerSwitchRunning)
            {
                this.RaisePropertyChanged(nameof(SelectedServer));
                return;
            }

            var previousServer = _selectedServer;
            this.RaiseAndSetIfChanged(ref _selectedServer, value);
            _ = SwitchServerAsync(value, previousServer);
        }
    }

    private static bool SameHost(GmlServer left, GmlServer right)
    {
        return Uri.TryCreate(left.Host, UriKind.Absolute, out var leftUri)
               && Uri.TryCreate(right.Host, UriKind.Absolute, out var rightUri)
               && leftUri == rightUri;
    }

    private async Task SwitchServerAsync(GmlServer server, GmlServer previousServer)
    {
        _isServerSwitchRunning = true;
        IsServerSwitchInProgress = true;

        try
        {
            await _storageService.SetAsync(StorageConstants.ServerHost, server.Host);
            _gmlClientManager.ChangeHost(server.Host);

            // Authentication is bound to a concrete backend, so switching the server
            // requires a fresh sign-in. The launcher itself keeps running.
            await _storageService.SetAsync<ILauncherUser?>(StorageConstants.User, null);
            await _autoLoginService.ClearCredentialsAsync();
            await _backendChecker.UpdateBackendStatus();

            // Error reporting is configured from the backend, so it has to follow the switch as
            // well instead of keeping the DSN of the server the launcher started with. The refresh
            // runs in the background so a slow telemetry endpoint cannot delay the login page.
            if (_backendChecker.IsOffline)
                SentryInitializer.Disable();
            else
                _ = SentryInitializer.RefreshAsync(server.Host);

            Router.Navigate.Execute(new LoginPageViewModel(this, OnClosed));

            if (_backendChecker.IsOffline)
            {
                Manager
                    .CreateMessage(true, "#993030",
                        _localizationService.GetString(ResourceKeysDictionary.Warning),
                        $"{server.Name}: {_localizationService.GetString(ResourceKeysDictionary.BackendOffline)}")
                    .Dismiss()
                    .WithDelay(TimeSpan.FromSeconds(5))
                    .Queue();
            }
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);

            _selectedServer = previousServer;
            this.RaisePropertyChanged(nameof(SelectedServer));

            Manager
                .CreateMessage(true, "#D03E3E",
                    _localizationService.GetString(ResourceKeysDictionary.Error),
                    exception.Message)
                .Dismiss()
                .WithDelay(TimeSpan.FromSeconds(5))
                .Queue();
        }
        finally
        {
            _isServerSwitchRunning = false;
            IsServerSwitchInProgress = false;
        }
    }

    public static async void RestartApp()
    {
        Process.Start(new ProcessStartInfo
        {
            FileName = $"{Environment.ProcessPath}",
            Arguments = string.Empty,
            UseShellExecute = true
        });
        Environment.Exit(0);
    }

    public async void CheckBackendStatus(INotificationMessageButton button)
    {
        button.IsEnabled = false;
        button.Content = _localizationService.GetString(ResourceKeysDictionary.CheckBackendTitle);

        var CheckMessage = Manager
            .CreateMessage(false, "#222",
                _localizationService.GetString(ResourceKeysDictionary.CheckBackendTitle),
                _localizationService.GetString(ResourceKeysDictionary.CheckBackendMessage))
            .WithAdditionalContent(ContentLocation.Bottom, new Avalonia.Controls.ProgressBar
            {
                Foreground = new SolidColorBrush(Color.FromArgb(128, 200, 200, 200)),
                Background = Brushes.Black,
                Margin = new Thickness(5, 5, 5, 5),
                IsIndeterminate = true,
                IsHitTestVisible = false
            })
            .WithCloseButtonVisible(false)
            .Queue();

        bool backendInactive = await _backendChecker.BackendIsActive();

        Manager.Dismiss(CheckMessage);
        button.IsEnabled = true;
        button.Content = _localizationService.GetString(ResourceKeysDictionary.CheckBackendButton);

        if (!backendInactive)
        {
            Manager
                .CreateMessage(false, "#086",
                    _localizationService.GetString(ResourceKeysDictionary.CheckBackendTitle),
                    _localizationService.GetString(ResourceKeysDictionary.CheckBackendResultSuccessful)).Dismiss().WithDelay(TimeSpan.FromSeconds(5))
                .Queue();
        }
        else
        {
            Manager
               .CreateMessage(false, "#933",
                   _localizationService.GetString(ResourceKeysDictionary.CheckBackendTitle),
                   _localizationService.GetString(ResourceKeysDictionary.CheckBackendResultFailed)).Dismiss().WithDelay(TimeSpan.FromSeconds(5))
               .Queue();
        }
    }

    private async void CheckBackend()
    {
        if (_backendChecker.IsOffline)
        {
            Manager
                .CreateMessage(true, "#993030",
                    _localizationService.GetString(ResourceKeysDictionary.OfflineMode),
                    _localizationService.GetString(ResourceKeysDictionary.OfflineDescription))
                .HasBadge("WARNING")
                .Accent("#A54848")
                .WithButtonsVisibility(true)
                .WithCloseButtonVisible(true)
                .WithButton(_localizationService.GetString(ResourceKeysDictionary.RestartButton), button => RestartApp())
                .WithButton(_localizationService.GetString(ResourceKeysDictionary.CheckBackendButton), button => CheckBackendStatus(button))
                .Queue();
        }
    }
    private async void CheckSystem()
    {
        if (_vpnChecker.IsUseVpnTunnel())
        {
            Manager
                .CreateMessage(true, "#3684EA",
                    _localizationService.GetString(ResourceKeysDictionary.Information),
                    _localizationService.GetString(ResourceKeysDictionary.VpnUse))
                .Dismiss()
                .WithDelay(TimeSpan.FromSeconds(10))
                .Queue();
        }
    }
}
