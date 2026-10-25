using Avalonia.Threading;
using Gml.Client;
using Gml.Client.Models;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Exceptions;
using Gml.Launcher.Core.Services;
using Gml.Launcher.ViewModels.Base;
using GmlCore.Interfaces.User;
using ReactiveUI.Fody.Helpers;
using Sentry;
using Splat;
using System;
using System.Diagnostics;
using System.IdentityModel.Tokens.Jwt;
using System.Linq;
using System.Security.Claims;
using System.Threading.Tasks;
using Gml.Client.Interfaces;
using IStorageService = Gml.Launcher.Core.Services.IStorageService;

namespace Gml.Launcher.ViewModels;

public class SplashScreenViewModel : WindowViewModelBase
{
    private readonly IStorageService _storageService;
    private readonly ILocalizationService _localizationService;
    private readonly IGmlClientManager _manager;
    private readonly ISystemService _systemService;
    private readonly IBackendChecker _backendChecker;
    private readonly DispatcherTimer _timer;
    private bool _isBackendChecked;
    public IGmlClientManager Manager => _manager;

    public SplashScreenViewModel(
        ISystemService? systemService = null,
        IGmlClientManager? manager = null,
        IStorageService? storage = null,
        ILocalizationService? localizationService = null,
        IBackendChecker? backendChecker = null)
    {
        _storageService = storage ?? Locator.Current.GetService<IStorageService>()
            ?? throw new ServiceNotFoundException(typeof(IStorageService));

        _systemService = systemService ?? Locator.Current.GetService<ISystemService>()
            ?? throw new ServiceNotFoundException(typeof(ISystemService));

        _manager = manager ?? Locator.Current.GetService<IGmlClientManager>()
            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));

        _localizationService = localizationService ?? Locator.Current.GetService<ILocalizationService>()
            ?? throw new ServiceNotFoundException(typeof(ILocalizationService));

        _backendChecker = backendChecker ?? Locator.Current.GetService<IBackendChecker>()
            ?? throw new ServiceNotFoundException(typeof(IBackendChecker));

        _timer = new DispatcherTimer();
        _timer.Interval = TimeSpan.FromSeconds(15);
        _timer.Tick += OnTimerTick;

        StatusText = _localizationService.GetString(ResourceKeysDictionary.PreparingLaunch);
    }

    [Reactive] public string StatusText { get; set; }
    [Reactive] public bool InfinityLoading { get; set; } = true;
    [Reactive] public short Progress { get; set; }
    [Reactive] public bool IsAuth { get; private set; }
    public ILocalizationService LocalizationService => _localizationService;

    private void OnTimerTick(object? sender, EventArgs e)
    {
        if (sender != null && !_isBackendChecked)
        {
            StatusText = _localizationService.GetString(ResourceKeysDictionary.BackendCheckingLonger);
        }
    }

    public async Task InitializeAsync()
    {
        try
        {
            await _systemService.LoadSystemData();

            ChangeState(_localizationService.GetString(ResourceKeysDictionary.BackendChecking), true);
            _isBackendChecked = false;
            _timer.Start();
            await _backendChecker.UpdateBackendStatus();
            _isBackendChecked = true;
            if (_backendChecker.IsOffline)
            {
                ChangeState(_localizationService.GetString(ResourceKeysDictionary.BackendOffline), true);
                await Task.Delay(1000);
            }

            if (!_backendChecker.IsOffline)
            {
                ChangeState(_localizationService.GetString(ResourceKeysDictionary.SentrySDKInit), true);
                await InitializeSentryAsync();
            }

            if (!_backendChecker.IsOffline)
            {
                var authUser = await _storageService.GetAsync<AuthLauncherUser>(StorageConstants.User);

                IsAuth = authUser != null
                         && authUser.ExpiredDate > DateTime.Now
                         && authUser is { IsAuth: true }
                         && await ValidateToken(authUser)
                         && await ValidateTokenWithApi(authUser);
            }
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
        }
        ChangeState(_localizationService.GetString(ResourceKeysDictionary.Launching), true);
        await Task.Delay(500);
    }

    private async Task<bool> ValidateToken(AuthLauncherUser launcherUser)
    {
        var handler = new JwtSecurityTokenHandler();

        if (!handler.CanReadToken(launcherUser.AccessToken))
            return false;

        var jwtToken = handler.ReadJwtToken(launcherUser.AccessToken);

        var claims = jwtToken.Claims.FirstOrDefault(c => c.Type == ClaimTypes.Name);

        if (claims?.Value == launcherUser.Name)
            return true;

        await _storageService.SetAsync<ILauncherUser?>(StorageConstants.User, null).ConfigureAwait(false);

        return false;
    }

    private async Task<bool> ValidateTokenWithApi(AuthLauncherUser launcherUser)
    {
        var userData = await _manager.Auth(launcherUser.AccessToken)
            .ConfigureAwait(false);

        if (userData.User.IsAuth)
        {
            // The backend may rotate the access token while validating it.
            await _storageService.SetAsync(StorageConstants.User, userData.User).ConfigureAwait(false);
            return true;
        }

        await _storageService.SetAsync<ILauncherUser?>(StorageConstants.User, null).ConfigureAwait(false);

        return userData.User.IsAuth;
    }

    private void ChangeState(string text, bool isInfinity)
    {
        StatusText = text;
        InfinityLoading = isInfinity;
    }

    private static async Task InitializeSentryAsync()
    {
        Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Start sentry initialization");

        var manager = Locator.Current.GetService<IGmlClientManager>()
            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));

        await SentryInitializer.RefreshAsync(manager.HostUri.ToString()).ConfigureAwait(false);
    }
}
