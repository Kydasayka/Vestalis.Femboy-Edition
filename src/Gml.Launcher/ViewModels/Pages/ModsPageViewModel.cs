using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Reactive.Concurrency;
using System.Reactive.Linq;
using System.Threading.Tasks;
using System.Windows.Input;
using Avalonia.Threading;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Client.Models;
using Gml.Dto.Messages;
using Gml.Dto.Mods;
using Gml.Dto.Profile;
using Gml.Launcher.Assets;
using Gml.Launcher.Core;
using Gml.Launcher.Core.Exceptions;
using Gml.Launcher.Core.Services;
using Gml.Launcher.ViewModels.Base;
using GmlCore.Interfaces.User;
using ReactiveUI;
using ReactiveUI.Fody.Helpers;
using Sentry;
using Splat;

namespace Gml.Launcher.ViewModels.Pages;

public class ModsPageViewModel : PageViewModelBase
{
    private readonly ProfileReadDto _profile;
    private ILauncherUser _user;
    private readonly IGmlClientManager _gmlManager;
    private readonly ISystemService _systemService;
    private readonly IAutoLoginService _autoLoginService;
    private readonly string _modsDirectory;
    private readonly List<ExternalModReadDto> _allMods = [];
    private ResponseMessage<ProfileReadInfoDto?>? _profileInfo;
    [Reactive] public ConcurrentDictionary<string, ModsDetailsInfoDto> OptionalModsDetails { get; set; } = [];
    [Reactive] public ObservableCollection<ExternalModReadDto> ProfileOptionalMods { get; set; } = [];
    [Reactive] public bool ModsListIsEmpty { get; set; }
    [Reactive] public bool IsSearchEmpty { get; set; }
    [Reactive] public string SearchText { get; set; } = string.Empty;
    [Reactive] public int TotalMods { get; set; }
    [Reactive] public int EnabledMods { get; set; }
    [Reactive] public ICommand ChangeOptionalModState { get; set; }

    public ModsPageViewModel(IScreen screen,
        ProfileReadDto profile,
        ILauncherUser user,
        IGmlClientManager gmlManager,
        ISystemService systemService) : base(screen)
    {
        _profile = profile;
        _modsDirectory = Path.Combine(gmlManager.InstallationDirectory, "game data", profile.Name, "mods");
        _user = user;
        _gmlManager = gmlManager;
        _systemService = systemService;
        _autoLoginService = Locator.Current.GetService<IAutoLoginService>()
                            ?? throw new ServiceNotFoundException(typeof(IAutoLoginService));

        ChangeOptionalModState = ReactiveCommand.Create<ExternalModReadDto>(ChangeModState);

        this.WhenAnyValue(x => x.SearchText)
            .Throttle(TimeSpan.FromMilliseconds(150), RxApp.MainThreadScheduler)
            .Subscribe(_ => ApplyFilter());

        RxApp.MainThreadScheduler.Schedule(LoadData);
    }

    private async void ChangeModState(ExternalModReadDto mod)
    {
        try
        {
            var file = Directory.GetFiles(_modsDirectory, $"{mod.Name}*").FirstOrDefault();

            var fileNeedLoad = file is null || !File.Exists(file) || new FileInfo(file).Length == 0;

            if (fileNeedLoad && _profileInfo?.Data is { } profileInfo)
            {
                var fileToDownload = profileInfo.Files.FirstOrDefault(c => c.Name == mod.FileName);
                if (fileToDownload != null)
                    await _gmlManager.DownloadFiles([fileToDownload]);
                file = Directory.GetFiles(_modsDirectory, $"{mod.Name}*").FirstOrDefault();
            }

            var isFullLoaded = file is not null && File.Exists(file) && new FileInfo(file).Length > 0;

            if (isFullLoaded && _gmlManager.ToggleOptionalMod(file, mod.IsSelected))
            {
                UpdateCounters();
                // ShowSuccess(SystemConstants.Success,
                //     mod.IsSelected
                //         ? LocalizationService.GetString(SystemConstants.ModEnabled)
                //         : LocalizationService.GetString(SystemConstants.ModDisabled));
            }
        }
        catch (Exception exception)
        {
            ShowError(ResourceKeysDictionary.Error, exception.Message);
            SentrySdk.CaptureException(exception);
        }

    }

    private void LoadData()
    {
        _ = LoadDataAsync(true);
    }

    private Task LoadDataAsync(bool canRecover)
    {
        return ExecuteFromNewThread(async () =>
        {
            try
            {
                var optionalModsInfo = await _gmlManager.GetOptionalModsInfo(_user.AccessToken);
                var optionalMods = await _gmlManager.GetOptionalMods(_profile.Name, _user.AccessToken);
                var enabledMods = GetEnabledMods(_modsDirectory);
                var models = (optionalMods.Data ?? []).Select(c => new ExternalModReadDto(c, enabledMods.Contains(c.Name)));

                Dispatcher.UIThread.Invoke(() =>
                {
                    OptionalModsDetails = new ConcurrentDictionary<string, ModsDetailsInfoDto>(
                        optionalModsInfo.Data?.ToDictionary(mod => mod.Key, mod => mod) ?? []
                    );

                    _allMods.Clear();
                    _allMods.AddRange(models);
                    ApplyFilter();
                    UpdateCounters();
                });

                _profileInfo = await _gmlManager.GetProfileInfo(new ProfileCreateInfoDto
                {
                    ProfileName = _profile.Name,
                    RamSize = 0,
                    OsType = ((int)_systemService.GetOsType()).ToString(),
                    OsArchitecture = _systemService.GetOsArchitecture(),
                    UserAccessToken = _user.AccessToken,
                    UserName = _user.Name,
                    UserUuid = _user.Uuid
                });
            }
            catch (UnauthorizedAccessException exception)
            {
                SentrySdk.CaptureException(exception);

                if (!canRecover)
                    return;

                var result = await _autoLoginService.TrySignInAsync();

                if (!result.IsAuthenticated || result.User is null)
                    return;

                _user = result.User;
                await LoadDataAsync(false);
            }
            catch (Exception exception)
            {
                SentrySdk.CaptureException(exception);
            }
        });
    }

    private void ApplyFilter()
    {
        var query = SearchText?.Trim() ?? string.Empty;

        var filtered = string.IsNullOrEmpty(query)
            ? _allMods.ToList()
            : _allMods.Where(mod => Matches(mod, query)).ToList();

        ProfileOptionalMods = new ObservableCollection<ExternalModReadDto>(filtered);
        ModsListIsEmpty = _allMods.Count == 0;
        IsSearchEmpty = _allMods.Count > 0 && filtered.Count == 0;
    }

    private static bool Matches(ExternalModReadDto mod, string query)
    {
        return mod.Name.Contains(query, StringComparison.OrdinalIgnoreCase)
               || (mod.Description?.Contains(query, StringComparison.OrdinalIgnoreCase) ?? false);
    }

    private void UpdateCounters()
    {
        TotalMods = _allMods.Count;
        EnabledMods = _allMods.Count(mod => mod.IsSelected);
    }

    private ICollection<string> GetEnabledMods(string modsDirectory)
    {
        if (!Directory.Exists(modsDirectory))
        {
            Directory.CreateDirectory(modsDirectory);
        }

        var optionalMods = Directory.GetFiles(modsDirectory, $"*-optional-mod.jar");

        return optionalMods.Select(Path.GetFileNameWithoutExtension).ToArray()!;
    }
}

public class ExternalModReadDto : ModReadDto
{
    public bool IsSelected { get; set; }

    public string FileName { get; set; }
    public ExternalModReadDto(ModReadDto baseDto, bool isEnabled = false)
    {
        Type = baseDto.Type;
        Name = baseDto.Name;
        FileName = $"{baseDto.Name}.jar";
        Description = baseDto.Description;
        IsSelected = isEnabled;
    }
}
