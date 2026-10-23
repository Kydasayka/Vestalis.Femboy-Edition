using System;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Linq;
using System.Reactive;
using System.Reactive.Concurrency;
using System.Reactive.Linq;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Launcher.Assets.Resources;
using Gml.Launcher.Core.Services;
using Gml.Launcher.Models;
using Gml.Launcher.ViewModels.Base;
using ReactiveUI;
using ReactiveUI.Fody.Helpers;
using Sentry;

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

    private double _xmsRamValue = 2048;
    private double _xmxRamValue = 8192;
    private int _windowHeight;
    private int _windowWidth;

    public SettingsPageViewModel(
        IScreen screen,
        ILocalizationService? localizationService,
        ISettingsService settingsService,
        IGmlClientManager gmlManager) : base(screen, localizationService)
    {
        MainViewModel = (MainWindowViewModel)screen;
        _settingsService = settingsService;
        _gmlManager = gmlManager;

        this.WhenAnyValue(
                x => x.XmsRamValue,
                x => x.XmxRamValue,
                x => x.WindowWidth,
                x => x.WindowHeight,
                x => x.FullScreen,
                x => x.SelectedLanguage,
                x => x.CustomJvmArguments)
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
    }

    private bool ValidateParams(
        (double xmsRamValue, double xmxRamValue, string width, string height,
            bool isFullScreen, Language? selectedLanguage, string? customJvmArguments) update)
    {
        if (update.xmsRamValue < MinXmsRamMb ||
            update.xmxRamValue < MinXmxRamMb ||
            update.xmsRamValue > update.xmxRamValue ||
            update.xmxRamValue > MaxJvmRamValue)
        {
            return false;
        }

        if (!int.TryParse(update.width, out var width) || width <= 0)
            return false;

        if (!int.TryParse(update.height, out var height) || height <= 0)
            return false;

        return true;
    }

    [Reactive] public bool FullScreen { get; set; }
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
        WindowWidth = settings.GameWidth.ToString(CultureInfo.InvariantCulture);
        WindowHeight = settings.GameHeight.ToString(CultureInfo.InvariantCulture);
        FullScreen = settings.FullScreen;
        CustomJvmArguments = settings.CustomJvmArguments ?? string.Empty;
        SelectedLanguage = AvailableLanguages.FirstOrDefault(c => c.Culture.Name == settings.LanguageCode);
        InstallationFolder = _gmlManager.InstallationDirectory;

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

    private async void SaveSettings(
        (double xmsRamValue, double xmxRamValue, string width, string height,
            bool isFullScreen, Language? selectedLanguage, string? customJvmArguments) update)
    {
        try
        {
            var newSettings = new SettingsInfo(
                int.Parse(update.width, CultureInfo.InvariantCulture),
                int.Parse(update.height, CultureInfo.InvariantCulture),
                update.isFullScreen,
                false,
                update.xmxRamValue,
                update.selectedLanguage?.Culture.Name,
                update.customJvmArguments,
                Convert.ToInt32(update.xmsRamValue),
                Convert.ToInt32(update.xmxRamValue));

            await _settingsService.UpdateSettingsAsync(newSettings);
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
        }
    }
}
