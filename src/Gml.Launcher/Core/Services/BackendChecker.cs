using System;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Threading.Tasks;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Launcher.Core.Exceptions;
using Splat;

namespace Gml.Launcher.Core.Services;

public class BackendChecker: IBackendChecker
{
    private static readonly TimeSpan BackendCheckTimeout = TimeSpan.FromSeconds(10);
    private readonly IGmlClientManager _manager;
    private readonly IMirrorControlService _mirrorControl;
    public IGmlClientManager Manager => _manager;
    public bool IsOffline => GlobalVariables.BackendInactive;
    public string? BackendMode { get; private set; }

    public BackendChecker(
        IGmlClientManager? manager=null,
        IMirrorControlService? mirrorControl=null)
    {
        _manager = manager ?? Locator.Current.GetService<IGmlClientManager>()
            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));

        _mirrorControl = mirrorControl ?? Locator.Current.GetService<IMirrorControlService>()
            ?? throw new ServiceNotFoundException(typeof(IMirrorControlService));
    }

    public async Task UpdateBackendStatus()
    {
        GlobalVariables.BackendInactive = await BackendIsActive() == false;

        if (GlobalVariables.BackendInactive)
        {
            SetMode(null);
            GlobalVariables.CanControlBackend = false;
        }
        else
        {
            await GetBackendModeAsync();
        }
    }

    public Task<bool> BackendIsActive()
    {
        try
        {
            return GmlClientManager.CheckApiAsync(_manager.HostUri.ToString(), BackendCheckTimeout);
        }
        catch (TaskCanceledException exception)
        {
            Sentry.SentrySdk.CaptureException(exception);
            Console.WriteLine(exception);
            return Task.FromResult(false);
        }
        catch (HttpRequestException exception)
        {
            Sentry.SentrySdk.CaptureException(exception);
            Console.WriteLine(exception);
            return Task.FromResult(false);
        }
    }

    public async Task<string?> GetBackendModeAsync()
    {
        var state = await _mirrorControl.GetStateAsync();

        if (state is null)
        {
            // A backend without the mirror endpoints is the main backend itself.
            SetMode(GlobalVariables.BackendInactive ? null : "direct");
            GlobalVariables.CanControlBackend = false;
            return BackendMode;
        }

        SetMode(state.Mode);
        GlobalVariables.CanControlBackend = state.CanControl;

        return BackendMode;
    }

    private void SetMode(string? mode)
    {
        BackendMode = mode;
        GlobalVariables.BackendMode = mode;
    }
}
