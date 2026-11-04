using System;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Threading.Tasks;
using Avalonia.Controls;
using Gml.Client;
using Gml.Client.Interfaces;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Exceptions;
using Gml.Launcher.Core.Services;
using Sentry;
using Splat;

namespace Gml.Launcher.Core.Services;

public class BackendChecker: IBackendChecker
{
    private static readonly TimeSpan BackendCheckTimeout = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan ModeCheckTimeout = TimeSpan.FromSeconds(5);
    private readonly IGmlClientManager _manager;
    public IGmlClientManager Manager => _manager;
    public bool IsOffline => GlobalVariables.BackendInactive;
    public string? BackendMode { get; private set; }

    public BackendChecker(
        IGmlClientManager? manager=null)
    {
        _manager = manager ?? Locator.Current.GetService<IGmlClientManager>()
            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));
    }

    public async Task UpdateBackendStatus()
    {
        GlobalVariables.BackendInactive = await BackendIsActive() == false;

        if (GlobalVariables.BackendInactive)
            SetMode(null);
        else
            await GetBackendModeAsync();
    }

    public Task<bool> BackendIsActive()
    {
        try
        {
            return GmlClientManager.CheckApiAsync(_manager.HostUri.ToString(), BackendCheckTimeout);
        }
        catch (TaskCanceledException exception)
        {
            SentrySdk.CaptureException(exception);
            Console.WriteLine(exception);
            return Task.FromResult(false);
        }
        catch (HttpRequestException exception)
        {
            SentrySdk.CaptureException(exception);
            Console.WriteLine(exception);
            return Task.FromResult(false);
        }
    }

    public async Task<string?> GetBackendModeAsync()
    {
        try
        {
            using var client = new HttpClient { Timeout = ModeCheckTimeout };
            using var response = await client.GetAsync(new Uri(_manager.HostUri, "api/v1/mirror/mode"))
                .ConfigureAwait(false);

            // A backend without the mirror endpoints is the main backend itself.
            if (response.StatusCode == HttpStatusCode.NotFound)
            {
                SetMode("direct");
                return BackendMode;
            }

            if (!response.IsSuccessStatusCode)
            {
                SetMode(null);
                return BackendMode;
            }

            await using var stream = await response.Content.ReadAsStreamAsync().ConfigureAwait(false);
            using var document = await JsonDocument.ParseAsync(stream).ConfigureAwait(false);

            if (document.RootElement.TryGetProperty("data", out var data) &&
                data.TryGetProperty("mode", out var mode))
                SetMode(mode.GetString()?.ToLowerInvariant());
            else
                SetMode(null);
        }
        catch (Exception exception) when (exception is HttpRequestException or TaskCanceledException)
        {
            SetMode(null);
        }

        return BackendMode;
    }

    private void SetMode(string? mode)
    {
        BackendMode = mode;
        GlobalVariables.BackendMode = mode;
    }
}
