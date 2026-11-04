using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Linq;
using System.Reactive;
using System.Reactive.Concurrency;
using System.Reactive.Linq;
using System.Threading.Tasks;
using Avalonia.Media;
using GamerVII.Notification.Avalonia;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Launcher.Assets;
using Gml.Launcher.Assets.Resources;
using Gml.Launcher.Core.Services;
using Gml.Launcher.Models;
using Gml.Launcher.ViewModels.Base;
using ReactiveUI;
using ReactiveUI.Fody.Helpers;
using Sentry;
using Splat;

namespace Gml.Launcher.ViewModels.Pages;

public class SettingsPageViewModel : PageViewModelBase
{
    private const int MinXmsRamMb = 512;
    private const int MinXmxRamMb = 1024;
    private const int RamSliderStepMb = 512;
    private const int MaxXmxRamMb = 32768;
    private const int ReservedSystemRamMb = 4096;

    private readonly IGmlClientManager _gmlManager;
    private readonly ISettingsService _settingsService;
    private readonly IMirrorControlService _mirrorControl;

    private double _xmsRamValue = 2048;
    private double _xmxRamValue = 8192;
    private int _windowHeight;
    private int _windowWidth;
    private bool _suppressTorToggle;

    public SettingsPageViewModel(
        IScreen screen,
        ILocalizationService? localizationService,
        ISettingsService settingsService,
        IGmlClientManager gmlManager,
        IMirrorControlService? mirrorControl = null) : base(screen, localizationService)
    {
        MainViewModel = (MainWindowViewModel)screen;
        _settingsService = settingsService;
        _gmlManager = gmlManager;
        _mirrorControl = mirrorControl ?? Locator.Current.GetService<IMirrorControlService>()
            ?? new MirrorControlService();

        VerifyServiceKeyCommand = ReactiveCommand.CreateFromTask(VerifyServiceKeyAsync);
        ClearServiceKeyCommand = ReactiveCommand.CreateFromTask(ClearServiceKeyAsync);
        SetBackendModeCommand = ReactiveCommand.CreateFromTask<string>(SetBackendModeAsync);
        RunSelfTestCommand = ReactiveCommand.CreateFromTask(RunSelfTestAsync);

        // The toggle is bound two-way. Programmatic state refreshes set the same property, so
        // they are guarded to avoid sending the value back to the server.
        this.WhenAnyValue(x => x.IsBackendTorEnabled)
            .Skip(1)
            .Subscribe(enabled =>
            {
                if (!_suppressTorToggle)
                    _ = SetBackendTorAsync(enabled);
            });
        var settingsChanged = this.WhenAnyValue(
                x => x.XmsRamValue,
                x => x.XmxRamValue,
                x => x.WindowWidth,
                x => x.WindowHeight,
                x => x.FullScreen,
                x => x.SelectedLanguage,
                x => x.CustomJvmArguments)
            .Select(_ => CreateUpdate());

        // Toggling a switch alone does not touch the values above, so the switches are merged in
        // separately to keep the whole settings object in sync.
        var switchesChanged = this.WhenAnyValue(x => x.InterfaceSounds, x => x.SendHwid)
            .Select(_ => CreateUpdate());

        settingsChanged
            .Merge(switchesChanged)
            .Where(ValidateParams)
            .Throttle(TimeSpan.FromMilliseconds(400), RxApp.TaskpoolScheduler)
            .ObserveOn(RxApp.TaskpoolScheduler)
            .Subscribe(SaveSettings);

        this.WhenAnyValue(x => x.SelectedLanguage)
            .Skip(2)
            .Subscribe(ChangeLanguage);

        this.WhenAnyValue(x => x.Settings)
            .WhereNotNull()
            .ObserveOn(RxApp.MainThreadScheduler)
            .Subscribe(UpdateSettings);

        RxApp.TaskpoolScheduler.Schedule(LoadSettings);
        RxApp.TaskpoolScheduler.Schedule(LoadMirrorControl);
    }

    private bool ValidateParams(SettingsUpdate update)
    {
        if (update.XmsRamValue < MinXmsRamMb ||
            update.XmxRamValue < MinXmxRamMb ||
            update.XmsRamValue > update.XmxRamValue ||
            update.XmxRamValue > MaxJvmRamValue)
        {
            return false;
        }

        if (!int.TryParse(update.WindowWidth, out var width) || width <= 0)
            return false;

        if (!int.TryParse(update.WindowHeight, out var height) || height <= 0)
            return false;

        return true;
    }

    private SettingsUpdate CreateUpdate()
    {
        return new SettingsUpdate(XmsRamValue, XmxRamValue, WindowWidth, WindowHeight, FullScreen,
            SelectedLanguage, CustomJvmArguments, InterfaceSounds, SendHwid);
    }

    private sealed record SettingsUpdate(
        double XmsRamValue,
        double XmxRamValue,
        string WindowWidth,
        string WindowHeight,
        bool FullScreen,
        Language? SelectedLanguage,
        string? CustomJvmArguments,
        bool InterfaceSounds,
        bool SendHwid);

    [Reactive] public bool FullScreen { get; set; }
    [Reactive] public bool InterfaceSounds { get; set; } = true;
    [Reactive] public bool SendHwid { get; set; } = true;
    [Reactive] public string CustomJvmArguments { get; set; } = string.Empty;
    [Reactive] public double MinJvmRamValue { get; set; } = MinXmsRamMb;
    [Reactive] public double MinXmxRamValue { get; set; } = MinXmxRamMb;
    [Reactive] public double MaxJvmRamValue { get; set; } = 8192;
    [Reactive] public double RamTickValue { get; set; } = RamSliderStepMb;
    [Reactive] public string? XmsRamValueView { get; set; }
    [Reactive] public string? XmxRamValueView { get; set; }
    [Reactive] public Language? SelectedLanguage { get; set; }
    [Reactive] public string? InstallationFolder { get; set; }
    [Reactive] private SettingsInfo? Settings { get; set; }
    [Reactive] public ObservableCollection<Language> AvailableLanguages { get; set; } = [];

    // --- Optional mirror control (service key) -------------------------------
    [Reactive] public string ServiceKey { get; set; } = string.Empty;
    [Reactive] public bool IsServiceKeySaved { get; private set; }
    [Reactive] public bool IsServiceKeyBusy { get; private set; }
    [Reactive] public bool IsBackendControlAvailable { get; private set; }
    [Reactive] public bool IsBackendLocalMode { get; private set; }
    [Reactive] public bool IsBackendProxyMode { get; private set; }
    [Reactive] public bool IsBackendTorEnabled { get; set; }
    [Reactive] public bool IsBackendTorAvailable { get; private set; }
    [Reactive] public string BackendControlStatus { get; private set; } = string.Empty;
    [Reactive] public bool IsSelfTestRunning { get; private set; }
    [Reactive] public bool HasSelfTestResults { get; private set; }
    [Reactive] public string SelfTestSummary { get; private set; } = string.Empty;

    public ObservableCollection<SelfTestRow> SelfTestChecks { get; } = [];

    public ReactiveCommand<Unit, Unit> VerifyServiceKeyCommand { get; }
    public ReactiveCommand<Unit, Unit> ClearServiceKeyCommand { get; }
    public ReactiveCommand<string, Unit> SetBackendModeCommand { get; }
    public ReactiveCommand<Unit, Unit> RunSelfTestCommand { get; }

    public MainWindowViewModel MainViewModel { get; }

    public double XmsRamValue
    {
        get => _xmsRamValue;
        set
        {
            var maximum = Math.Max(MinXmsRamMb, XmxRamValue);
            var normalized = NormalizeRamValue(value, MinXmsRamMb, maximum);
            if (Math.Abs(normalized - _xmsRamValue) < 0.1)
            {
                XmsRamValueView = FormatRamValue(normalized);
                return;
            }

            this.RaiseAndSetIfChanged(ref _xmsRamValue, normalized);
            XmsRamValueView = FormatRamValue(normalized);
        }
    }

    public double XmxRamValue
    {
        get => _xmxRamValue;
        set
        {
            var maximum = Math.Max(MinXmxRamMb, MaxJvmRamValue);
            var normalized = NormalizeRamValue(value, MinXmxRamMb, maximum);

            // Xms must never exceed Xmx; lower Xms automatically when Xmx is reduced.
            if (XmsRamValue > normalized)
                XmsRamValue = normalized;

            if (Math.Abs(normalized - _xmxRamValue) < 0.1)
            {
                XmxRamValueView = FormatRamValue(normalized);
                return;
            }

            this.RaiseAndSetIfChanged(ref _xmxRamValue, normalized);
            XmxRamValueView = FormatRamValue(normalized);
        }
    }

    public string WindowWidth
    {
        get => _windowWidth.ToString(CultureInfo.InvariantCulture);
        set
        {
            var isNumeric = int.TryParse(string.Concat(value.Where(char.IsDigit)), out var numericValue);
            if (isNumeric)
                this.RaiseAndSetIfChanged(ref _windowWidth, numericValue);
            else
                this.RaiseAndSetIfChanged(ref _windowWidth, 600);
        }
    }

    public string WindowHeight
    {
        get => _windowHeight.ToString(CultureInfo.InvariantCulture);
        set
        {
            var isNumeric = int.TryParse(string.Concat(value.Where(char.IsDigit)), out var numericValue);
            if (isNumeric)
                this.RaiseAndSetIfChanged(ref _windowHeight, numericValue);
            else
                this.RaiseAndSetIfChanged(ref _windowHeight, 900);
        }
    }

    private static double NormalizeRamValue(double value, double minimum, double maximum)
    {
        var clamped = Math.Clamp(value, minimum, maximum);
        var snapped = Math.Round(clamped / RamSliderStepMb, MidpointRounding.AwayFromZero) * RamSliderStepMb;
        return Math.Clamp(snapped, minimum, maximum);
    }

    private static string FormatRamValue(double value)
    {
        if (value < 1024)
            return $"{value:0} MB";

        return $"{(value / 1024d).ToString("0.##", CultureInfo.CurrentCulture)} GB";
    }

    private void ChangeLanguage(Language? language)
    {
        if (language == null) return;

        Resources.Culture = language.Culture;
        GoBackCommand.Execute(Unit.Default);
    }

    internal void ChangeFolder()
    {
        if (InstallationFolder != null)
            _gmlManager.ChangeInstallationFolder(InstallationFolder);

        _settingsService.UpdateInstallationDirectory(InstallationFolder);
    }

    private void UpdateSettings(SettingsInfo settings)
    {
        // Restoring the saved values also sets the language ComboBox selection programmatically.
        // That raises SelectionChanged just like a click, so keep it silent.
        using (SoundService.Suppress())
        {
            WindowWidth = settings.GameWidth.ToString(CultureInfo.InvariantCulture);
            WindowHeight = settings.GameHeight.ToString(CultureInfo.InvariantCulture);
            FullScreen = settings.FullScreen;
            InterfaceSounds = settings.InterfaceSounds;
            SendHwid = settings.SendHwid;
            SoundService.IsEnabled = settings.InterfaceSounds;
            CustomJvmArguments = settings.CustomJvmArguments ?? string.Empty;
            SelectedLanguage = AvailableLanguages.FirstOrDefault(c => c.Culture.Name == settings.LanguageCode);
            InstallationFolder = _gmlManager.InstallationDirectory;
        }

        // Migrate settings from the old single-RAM slider. Previously automatic mode
        // used the profile's recommendation, so use 8 GB as the new starting point.
        var legacyXmx = settings.XmxRamValueMb ??
                        (settings.IsDynamicRam ? 8192 : Convert.ToInt32(settings.RamValue));
        var xmx = (int)Math.Clamp(legacyXmx, MinXmxRamMb, MaxJvmRamValue);
        var legacyXms = settings.XmsRamValueMb ?? Math.Min(2048, xmx);

        XmxRamValue = xmx;
        XmsRamValue = Math.Min(legacyXms, xmx);
    }

    private async void LoadSettings()
    {
        try
        {
            AvailableLanguages = new ObservableCollection<Language>(_settingsService.GetAvailableLanguages());

            var totalRamMb = _settingsService.GetMaxRam();
            var totalRamSafe = (int)Math.Min(totalRamMb, int.MaxValue);
            MaxJvmRamValue = Math.Min(MaxXmxRamMb, Math.Max(MinXmxRamMb, totalRamSafe - ReservedSystemRamMb));

            Settings = await _settingsService.GetSettings();
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
        }
    }

    private async void SaveSettings(SettingsUpdate update)
    {
        try
        {
            SoundService.IsEnabled = update.InterfaceSounds;

            var newSettings = new SettingsInfo(
                int.Parse(update.WindowWidth, CultureInfo.InvariantCulture),
                int.Parse(update.WindowHeight, CultureInfo.InvariantCulture),
                update.FullScreen,
                false,
                update.XmxRamValue,
                update.SelectedLanguage?.Culture.Name,
                update.CustomJvmArguments,
                Convert.ToInt32(update.XmsRamValue),
                Convert.ToInt32(update.XmxRamValue),
                update.InterfaceSounds,
                update.SendHwid);

            await _settingsService.UpdateSettingsAsync(newSettings);
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
        }
    }

    // --- Optional mirror control ---------------------------------------------

    private async void LoadMirrorControl()
    {
        try
        {
            await _mirrorControl.LoadAsync();
            await RefreshBackendControlAsync();
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
        }
    }

    private async Task RefreshBackendControlAsync()
    {
        var state = await _mirrorControl.GetStateAsync();

        IsServiceKeySaved = _mirrorControl.HasKey;
        IsBackendControlAvailable = state?.CanControl == true;
        IsBackendLocalMode = state?.Mode == "local";
        IsBackendProxyMode = state?.Mode == "proxy";

        _suppressTorToggle = true;
        IsBackendTorAvailable = state?.TorAvailable == true;
        IsBackendTorEnabled = state?.TorEnabled == true;
        _suppressTorToggle = false;

        GlobalVariables.CanControlBackend = IsBackendControlAvailable;

        BackendControlStatus = state switch
        {
            null => "Сервер не отвечает на запрос режима",
            { CanControl: true } =>
                $"Ключ принят. Режим: {(state.Mode == "proxy" ? "прокси" : "зеркало")}; Tor: {(state.TorEnabled ? "включён" : "выключен")}",
            _ when _mirrorControl.HasKey => "Ключ не принят этим сервером",
            _ => "Введите сервисный ключ, чтобы управлять режимом"
        };
    }

    private async Task VerifyServiceKeyAsync()
    {
        var key = ServiceKey.Trim();

        if (key.Length == 0)
        {
            BackendControlStatus = "Введите сервисный ключ";
            return;
        }

        IsServiceKeyBusy = true;

        try
        {
            var accepted = await _mirrorControl.SaveKeyAsync(key);
            ServiceKey = string.Empty;
            await RefreshBackendControlAsync();

            Notify(accepted, accepted
                ? "Сервисный ключ сохранён на этом компьютере"
                : "Сервисный ключ не принят сервером");
        }
        finally
        {
            IsServiceKeyBusy = false;
        }
    }

    private async Task ClearServiceKeyAsync()
    {
        IsServiceKeyBusy = true;

        try
        {
            await _mirrorControl.ClearKeyAsync();
            await RefreshBackendControlAsync();
            Notify(true, "Сервисный ключ удалён с этого компьютера");
        }
        finally
        {
            IsServiceKeyBusy = false;
        }
    }

    private async Task SetBackendModeAsync(string mode)
    {
        IsServiceKeyBusy = true;

        try
        {
            var (ok, message) = await _mirrorControl.SetModeAsync(mode);
            await RefreshBackendControlAsync();
            Notify(ok, message);
            await MainViewModel.RefreshBackendModeAsync();
        }
        finally
        {
            IsServiceKeyBusy = false;
        }
    }

    private async Task SetBackendTorAsync(bool enabled)
    {
        if (IsServiceKeyBusy)
            return;

        IsServiceKeyBusy = true;

        try
        {
            var (ok, message) = await _mirrorControl.SetTorAsync(enabled);
            await RefreshBackendControlAsync();
            Notify(ok, message);
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
            Notify(false, "Не удалось изменить маршрутизацию через Tor");
        }
        finally
        {
            IsServiceKeyBusy = false;
        }
    }

    private async Task RunSelfTestAsync()
    {
        IsSelfTestRunning = true;

        try
        {
            var report = await _mirrorControl.RunSelfTestAsync();

            SelfTestChecks.Clear();

            if (report is null)
            {
                HasSelfTestResults = false;
                SelfTestSummary = string.Empty;
                Notify(false, "Самодиагностика недоступна: сервер не ответил или ключ не принят");
                return;
            }

            foreach (var check in report.Checks)
                SelfTestChecks.Add(CreateSelfTestRow(check));

            HasSelfTestResults = SelfTestChecks.Count > 0;
            SelfTestSummary = BuildSelfTestSummary(report);

            Notify(report.Status == "ok", SelfTestSummary);

            // The mode check runs against the live server, so refresh the controls afterwards.
            await RefreshBackendControlAsync();
        }
        finally
        {
            IsSelfTestRunning = false;
        }
    }

    private static SelfTestRow CreateSelfTestRow(MirrorSelfTestCheck check)
    {
        var (statusText, brush) = check.Status switch
        {
            "ok" => ("ОК", new SolidColorBrush(Color.Parse("#2E9E5B"))),
            "warn" => ("ВНИМАНИЕ", new SolidColorBrush(Color.Parse("#D9822B"))),
            _ => ("ОШИБКА", new SolidColorBrush(Color.Parse("#D03E3E")))
        };

        return new SelfTestRow
        {
            Name = check.Name,
            Detail = check.Message,
            StatusText = statusText,
            Duration = $"{check.DurationMs} мс",
            Brush = brush
        };
    }

    private static string BuildSelfTestSummary(MirrorSelfTestReport report)
    {
        var failed = report.Checks.Count(c => c.Status == "fail");
        var warnings = report.Checks.Count(c => c.Status == "warn");
        var mode = report.Mode == "proxy" ? "прокси" : "зеркало";
        var elapsed = $"{report.DurationMs} мс, режим: {mode}";

        if (failed == 0 && warnings == 0)
            return $"Всё в порядке ({elapsed})";

        var parts = new List<string>();

        if (failed > 0)
            parts.Add($"ошибок: {failed}");

        if (warnings > 0)
            parts.Add($"замечаний: {warnings}");

        return $"Проблемы: {string.Join(", ", parts)} ({elapsed})";
    }

    private void Notify(bool success, string message)
    {
        MainViewModel.Manager
            .CreateMessage(!success, success ? "#2E9E5B" : "#D03E3E",
                LocalizationService.GetString(ResourceKeysDictionary.Information), message)
            .Dismiss()
            .WithDelay(TimeSpan.FromSeconds(4))
            .Queue();
    }
}
