using System;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using Gml.Client.Interfaces;
using Gml.Launcher.Core.Exceptions;
using Splat;

namespace Gml.Launcher.Core.Services;

/// <summary>
/// Talks to <c>/api/v1/mirror/mode</c>. The service key is kept only in the local launcher
/// database (<see cref="StorageConstants.MirrorControlKey"/>) and is never baked into the build,
/// so a regular user cannot switch the backend mode.
/// </summary>
public sealed class MirrorControlService : IMirrorControlService
{
    private const string ControlKeyHeader = "X-Mirror-Control-Key";

    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(10) };

    private readonly IStorageService _storage;
    private readonly IGmlClientManager _manager;
    private string? _key;

    public MirrorControlService(IStorageService? storage = null, IGmlClientManager? manager = null)
    {
        _storage = storage ?? Locator.Current.GetService<IStorageService>()
            ?? throw new ServiceNotFoundException(typeof(IStorageService));
        _manager = manager ?? Locator.Current.GetService<IGmlClientManager>()
            ?? throw new ServiceNotFoundException(typeof(IGmlClientManager));
    }

    public bool IsUnlocked { get; private set; }

    public bool HasKey => !string.IsNullOrEmpty(_key);

    public async Task LoadAsync()
    {
        try
        {
            _key = (await _storage.GetAsync<string>(StorageConstants.MirrorControlKey))?.Trim();
        }
        catch (Exception)
        {
            _key = null;
        }

        if (string.IsNullOrEmpty(_key))
        {
            SetUnlocked(false);
            return;
        }

        // Validate quietly: the key may have been rotated on the server since the last run.
        await GetStateAsync();
    }

    public async Task<bool> SaveKeyAsync(string? key)
    {
        key = key?.Trim();

        if (string.IsNullOrEmpty(key))
            return false;

        var previous = _key;
        _key = key;

        var state = await GetStateAsync();

        if (state?.CanControl != true)
        {
            _key = previous;
            SetUnlocked(false);
            return false;
        }

        try
        {
            await _storage.SetAsync(StorageConstants.MirrorControlKey, key);
        }
        catch (Exception)
        {
            // Keeping the key in memory for this session is still useful.
        }

        return true;
    }

    public async Task ClearKeyAsync()
    {
        _key = null;
        SetUnlocked(false);

        try
        {
            await _storage.SetAsync<string?>(StorageConstants.MirrorControlKey, null);
        }
        catch (Exception)
        {
            // Nothing to do: the in-memory key is already gone.
        }
    }

    public async Task<MirrorModeState?> GetStateAsync()
    {
        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, BuildUri("api/v1/mirror/mode"));

            if (!string.IsNullOrEmpty(_key))
                request.Headers.TryAddWithoutValidation(ControlKeyHeader, _key);

            using var response = await Http.SendAsync(request).ConfigureAwait(false);

            if (!response.IsSuccessStatusCode)
            {
                SetUnlocked(false);
                return null;
            }

            await using var stream = await response.Content.ReadAsStreamAsync().ConfigureAwait(false);
            using var document = await JsonDocument.ParseAsync(stream).ConfigureAwait(false);

            if (!document.RootElement.TryGetProperty("data", out var data))
                return null;

            var mode = data.TryGetProperty("mode", out var modeElement)
                ? modeElement.GetString()?.ToLowerInvariant() ?? "local"
                : "local";
            var upstream = data.TryGetProperty("upstream", out var upstreamElement)
                ? upstreamElement.GetString()
                : null;
            var syncEnabled = data.TryGetProperty("syncEnabled", out var syncElement) &&
                              syncElement.ValueKind == JsonValueKind.True;
            var canControl = data.TryGetProperty("canControl", out var controlElement) &&
                             controlElement.ValueKind == JsonValueKind.True;

            SetUnlocked(canControl);

            return new MirrorModeState(mode, upstream, syncEnabled, canControl);
        }
        catch (Exception exception) when (exception is HttpRequestException or TaskCanceledException or JsonException)
        {
            return null;
        }
    }

    public async Task<(bool Ok, string Message)> SetModeAsync(string mode)
    {
        mode = mode?.Trim().ToLowerInvariant() ?? string.Empty;

        if (mode is not ("local" or "proxy"))
            return (false, "Неизвестный режим бэкенда");

        if (!IsUnlocked || string.IsNullOrEmpty(_key))
            return (false, "Сервисный ключ не подтверждён");

        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Put, BuildUri("api/v1/mirror/mode"));
            request.Headers.TryAddWithoutValidation(ControlKeyHeader, _key);
            request.Content = new StringContent(JsonSerializer.Serialize(new { mode }), Encoding.UTF8,
                "application/json");

            using var response = await Http.SendAsync(request).ConfigureAwait(false);
            var body = await response.Content.ReadAsStringAsync().ConfigureAwait(false);

            if (response.IsSuccessStatusCode)
                return (true, ReadMessage(body) ?? $"Режим переключён на \"{mode}\"");

            if (response.StatusCode is HttpStatusCode.Forbidden or HttpStatusCode.Unauthorized)
            {
                SetUnlocked(false);
                return (false, "Сервисный ключ не принят сервером");
            }

            return (false, ReadMessage(body) ?? "Не удалось переключить режим");
        }
        catch (Exception exception) when (exception is HttpRequestException or TaskCanceledException)
        {
            return (false, "Бэкенд недоступен");
        }
    }

    private Uri BuildUri(string path)
    {
        return new Uri(_manager.HostUri, path);
    }

    private void SetUnlocked(bool value)
    {
        IsUnlocked = value;
        GlobalVariables.CanControlBackend = value;
    }

    private static string? ReadMessage(string body)
    {
        try
        {
            using var document = JsonDocument.Parse(body);

            return document.RootElement.TryGetProperty("message", out var message)
                ? message.GetString()
                : null;
        }
        catch (JsonException)
        {
            return null;
        }
    }
}
