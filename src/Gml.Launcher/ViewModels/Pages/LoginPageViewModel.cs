using System;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Linq;
using System.Reactive.Concurrency;
using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Input;
using GamerVII.Notification.Avalonia;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Client.Models;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Exceptions;
using Gml.Launcher.Core.Services;
using Gml.Launcher.Models;
using Gml.Launcher.ViewModels.Base;
using ReactiveUI;
using ReactiveUI.Fody.Helpers;
using Sentry;
using Splat;

namespace Gml.Launcher.ViewModels.Pages;

public class LoginPageViewModel : PageViewModelBase
{
    private readonly IGmlClientManager _gmlClientManager;
    private readonly IObservable<bool> _onClosed;
    private readonly MainWindowViewModel _screen;
    private readonly IBackendChecker _backendChecker;
    private readonly IStorageService _storageService;
    private readonly ISettingsService _settingsService;
    private readonly IAutoLoginService _autoLoginService;
    private ObservableCollection<string> _errorList = [];
    private bool _isProcessing;
    private bool _isAutoLoginRunning;

    internal LoginPageViewModel(IScreen screen,
        IObservable<bool> onClosed,
        IGmlClientManager? gmlClientManager = null,
        IStorageService? storageService = null,
        IBackendChecker? backendChecker = null,
        ILocalizationService? localizationService = null,
        IAutoLoginService? autoLoginService = null,
        ISettingsService? settingsService = null) : base(screen, localizationService)
    {
        _screen = (MainWindowViewModel)screen;
        _onClosed = onClosed;

        _storageService = storageService
                          ?? Locator.Current.GetService<IStorageService>()
                          ?? throw new ServiceNotFoundException(typeof(IStorageService));

        _gmlClientManager = gmlClientManager
                            ?? Locator.Current.GetService<IGmlClientManager>()
                            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));

        _backendChecker = backendChecker
                          ?? Locator.Current.GetService<IBackendChecker>()
                          ?? throw new ServiceNotFoundException(typeof(IBackendChecker));

        _autoLoginService = autoLoginService
                            ?? Locator.Current.GetService<IAutoLoginService>()
                            ?? throw new ServiceNotFoundException(typeof(IAutoLoginService));

        _settingsService = settingsService
                           ?? Locator.Current.GetService<ISettingsService>()
                           ?? throw new ServiceNotFoundException(typeof(ISettingsService));


        _screen.OnClosed.Subscribe(DisposeConnections);

        LoginCommand = ReactiveCommand.CreateFromTask(OnAuth);
        Verify2FaCommand = ReactiveCommand.CreateFromTask(OnVerify2Fa);

        // The preference can be flipped right on this page, because a backend that blocks the
        // device by HWID would otherwise leave no way to sign in and reach the settings page.
        this.WhenAnyValue(x => x.SendHwid)
            .Skip(1)
            .Subscribe(OnSendHwidChanged);

        RxApp.MainThreadScheduler.Schedule(CheckAuth);
    }

    [Reactive] public string Login { get; set; }
    [Reactive] public string Password { get; set; }
    [Reactive] public string TwoFactorCode { get; set; }
    [Reactive] public bool Is2FaVisible { get; set; }

    /// <summary>Mirrors <see cref="SettingsInfo.SendHwid"/> so it can be changed before signing in.</summary>
    [Reactive] public bool SendHwid { get; set; } = true;

    public bool IsProcessing
    {
        get => _isProcessing;
        set
        {
            this.RaiseAndSetIfChanged(ref _isProcessing, value);
            this.RaisePropertyChanged(nameof(IsNotProcessing));
        }
    }

    public ObservableCollection<string> Errors
    {
        get => _errorList;
        set => this.RaiseAndSetIfChanged(ref _errorList, value);
    }

    public bool IsNotProcessing => !_isProcessing;

    public bool BackendIsActive => !_backendChecker.IsOffline;

    public ICommand LoginCommand { get; set; }
    public ICommand Verify2FaCommand { get; set; }

    private void DisposeConnections(bool isClosed)
    {
        _gmlClientManager.Dispose();
    }

    private async void CheckAuth()
    {
        // Load the preference first: the silent sign-in below sends the HWID and must already
        // respect the toggle.
        await LoadHwidSettingAsync();

        var authUser = await _storageService.GetAsync<AuthLauncherUser>(StorageConstants.User);

        if (authUser is { IsAuth: true } && authUser.ExpiredDate > DateTime.Now)
        {
            _screen.Router.Navigate.Execute(new OverviewPageViewModel(_screen, authUser, _onClosed));
            await _gmlClientManager.OpenServerConnection(authUser);
            return;
        }

        await TryAutoLoginAsync();
    }

    private async Task LoadHwidSettingAsync()
    {
        try
        {
            var settings = await _settingsService.GetSettings();
            SendHwid = settings.SendHwid;
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[Hwid] Failed to read the HWID preference: {exception}");
            SentrySdk.CaptureException(exception);
        }
    }

    private async void OnSendHwidChanged(bool sendHwid)
    {
        try
        {
            await _settingsService.SetHwidSendingEnabledAsync(sendHwid);
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[Hwid] Failed to save the HWID preference: {exception}");
            SentrySdk.CaptureException(exception);
        }
    }

    private async Task TryAutoLoginAsync()
    {
        if (_isAutoLoginRunning)
            return;

        var savedLogin = await GetSavedLoginSafeAsync();

        if (savedLogin is null)
            return;

        _isAutoLoginRunning = true;
        IsProcessing = true;

        // Keep the previous credentials on screen in case the silent sign-in fails
        // or the backend asks for a 2FA code.
        Login = savedLogin.Login;
        Password = savedLogin.Password;

        try
        {
            var result = await _autoLoginService.TrySignInAsync();

            if (result.IsAuthenticated && result.User is not null)
            {
                Debug.WriteLine("[AutoLogin] Session restored successfully");
                _screen.Router.Navigate.Execute(new OverviewPageViewModel(_screen, result.User, _onClosed));
                return;
            }

            if (result.RequiresTwoFactor)
            {
                Debug.WriteLine("[AutoLogin] 2FA required to restore session");
                Is2FaVisible = true;
                TwoFactorCode = string.Empty;
                return;
            }

            if (!string.IsNullOrWhiteSpace(result.Message))
                Errors = new ObservableCollection<string> { result.Message };
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[AutoLogin] Unexpected error: {exception}");
            SentrySdk.CaptureException(exception);
        }
        finally
        {
            _isAutoLoginRunning = false;
            IsProcessing = false;
        }
    }

    private async Task<SavedLogin?> GetSavedLoginSafeAsync()
    {
        try
        {
            return await _autoLoginService.GetSavedLoginAsync();
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[AutoLogin] Failed to read saved credentials: {exception}");
            SentrySdk.CaptureException(exception);

            return null;
        }
    }

    private async Task RememberCredentialsAsync()
    {
        try
        {
            await _autoLoginService.SaveCredentialsAsync(Login, Password);
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[AutoLogin] Failed to remember credentials: {exception}");
            SentrySdk.CaptureException(exception);
        }
    }

    private async Task OnAuth(CancellationToken arg)
    {
        try
        {
            IsProcessing = true;
            Errors.Clear();

            Debug.WriteLine("Starting authentication...");
            var hwid = await _settingsService.GetHwidForAuthAsync();
            var authInfo = await _gmlClientManager.Auth(Login, Password, hwid);
            Debug.WriteLine($"Auth response - IsAuth: {authInfo.User?.IsAuth}, Has2Fa: {authInfo.User?.Has2Fa}");
            Debug.WriteLine($"Auth message: {authInfo.Message}");
            Debug.WriteLine($"Auth details: {string.Join(", ", authInfo.Details)}");

            // Проверяем, требуется ли 2FA по сообщению об ошибке
            if (authInfo.User is not null && authInfo.User.Has2Fa)
            {
                Debug.WriteLine("2FA required based on error message");
                Is2FaVisible = true;
                TwoFactorCode = string.Empty;
                return;
            }

            if (authInfo.User?.IsAuth == true)
            {
                if (authInfo.User.Has2Fa)
                {
                    Debug.WriteLine("User has 2FA enabled, showing 2FA input");
                    Is2FaVisible = true;
                    TwoFactorCode = string.Empty;
                    return;
                }

                Debug.WriteLine("Authentication successful, no 2FA required");
                await _storageService.SetAsync(StorageConstants.User, authInfo.User);
                await RememberCredentialsAsync();
                _screen.Router.Navigate.Execute(new OverviewPageViewModel(_screen, authInfo.User, _onClosed));
                return;
            }

            Debug.WriteLine("Authentication failed");
            if (_screen is { } mainView)
            {
                if (!authInfo.Details.Any())
                    mainView.Manager
                        .CreateMessage(true, "#D03E3E",
                            LocalizationService.GetString(ResourceKeysDictionary.InvalidAuthData),
                            authInfo.Message)
                        .Dismiss()
                        .WithDelay(TimeSpan.FromSeconds(3))
                        .Queue();

                Errors = new ObservableCollection<string>(authInfo.Details);
            }
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"Authentication error: {exception}");
            if (_screen is { } mainView)
            {
                mainView.Manager
                    .CreateMessage(true, "#D03E3E",
                        LocalizationService.GetString(ResourceKeysDictionary.InvalidAuthData),
                        exception.Message)
                    .Dismiss()
                    .WithDelay(TimeSpan.FromSeconds(3))
                    .Queue();
            }

            Debug.WriteLine(exception);
            SentrySdk.CaptureException(exception);
        }
        finally
        {
            IsProcessing = false;
        }
    }

    private async Task OnVerify2Fa(CancellationToken arg)
    {
        try
        {
            IsProcessing = true;
            Errors.Clear();

            Debug.WriteLine($"Verifying 2FA code: {TwoFactorCode}");
            var hwid = await _settingsService.GetHwidForAuthAsync();
            var authInfo = await _gmlClientManager.AuthWith2Fa(Login, Password, hwid, TwoFactorCode);
            Debug.WriteLine($"2FA verification response - IsAuth: {authInfo.User?.IsAuth}, Has2Fa: {authInfo.User?.Has2Fa}");
            Debug.WriteLine($"2FA verification message: {authInfo.Message}");
            Debug.WriteLine($"2FA verification details: {string.Join(", ", authInfo.Details)}");

            if (authInfo.User?.IsAuth == true)
            {
                Debug.WriteLine("2FA verification successful");
                await _storageService.SetAsync(StorageConstants.User, authInfo.User);
                await RememberCredentialsAsync();
                Is2FaVisible = false; // Скрываем окно 2FA
                _screen.Router.Navigate.Execute(new OverviewPageViewModel(_screen, authInfo.User, _onClosed));
                return;
            }

            Debug.WriteLine("2FA verification failed");
            if (_screen is { } mainView)
            {
                // Если код неверный, показываем ошибку, но оставляем окно 2FA открытым
                mainView.Manager
                    .CreateMessage(true, "#D03E3E",
                        LocalizationService.GetString(ResourceKeysDictionary.InvalidAuthData),
                        authInfo.Message ?? "Invalid 2FA code")
                    .Dismiss()
                    .WithDelay(TimeSpan.FromSeconds(3))
                    .Queue();

                if (authInfo.Details.Any())
                {
                    Errors = new ObservableCollection<string>(authInfo.Details);
                }

                // Очищаем поле для ввода кода
                TwoFactorCode = string.Empty;
            }
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"2FA verification error: {exception}");
            if (_screen is { } mainView)
            {
                mainView.Manager
                    .CreateMessage(true, "#D03E3E",
                        LocalizationService.GetString(ResourceKeysDictionary.InvalidAuthData),
                        exception.Message)
                    .Dismiss()
                    .WithDelay(TimeSpan.FromSeconds(3))
                    .Queue();
            }

            Debug.WriteLine(exception);
            SentrySdk.CaptureException(exception);
        }
        finally
        {
            IsProcessing = false;
        }
    }
}
