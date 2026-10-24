using System;
using System.Diagnostics;
using System.Linq;
using System.Threading.Tasks;
using Gml.Client;
using Sentry;

namespace Gml.Launcher.Core.Services;

/// <summary>
/// Binds Sentry to the backend that is currently selected. Sentry is configured from the active
/// host, so switching the server must also switch (or disable) the DSN instead of keeping the
/// project of the server the launcher started with.
/// </summary>
internal static class SentryInitializer
{
    private static readonly object SyncRoot = new();

    private static IDisposable? _sentryHandle;
    private static string? _currentDsn;

    public static async Task RefreshAsync(string host)
    {
        try
        {
            var dsn = await GmlClientManager.GetSentryLinkAsync(host).ConfigureAwait(false);
            Apply(dsn);
        }
        catch (Exception exception)
        {
            Debug.WriteLine(
                $"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Sentry initialization skipped: {exception.Message}");
            Console.WriteLine(exception);

            // Never keep reporting to a backend that is no longer selected.
            Disable();
        }
    }

    public static void Disable()
    {
        lock (SyncRoot)
        {
            if (_currentDsn is null)
                return;

            _sentryHandle?.Dispose();
            _sentryHandle = null;
            _currentDsn = null;

            Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Sentry disabled");
        }
    }

    private static void Apply(string? dsn)
    {
        lock (SyncRoot)
        {
            if (!IsValidDsn(dsn))
            {
                // The backend may return a DSN without a public key (e.g. "https://gmlfr.vestalis.xyz/").
                // Sentry throws "Invalid DSN: No public key provided" for such values, so skip the
                // initialization instead of failing the whole pipeline.
                Debug.WriteLine(
                    $"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Sentry disabled: invalid DSN '{dsn}'");

                if (_currentDsn is null)
                    return;

                _sentryHandle?.Dispose();
                _sentryHandle = null;
                _currentDsn = null;
                return;
            }

            var normalized = dsn!.Trim();

            if (string.Equals(_currentDsn, normalized, StringComparison.Ordinal))
                return;

            _sentryHandle?.Dispose();
            _sentryHandle = SentrySdk.Init(options =>
            {
                options.Dsn = normalized;
#if DEBUG
                options.Debug = true;
#endif
                options.TracesSampleRate = 1.0;
                options.DiagnosticLevel = SentryLevel.Debug;
                options.IsGlobalModeEnabled = true;
                options.SendDefaultPii = true;
                options.MaxAttachmentSize = 10 * 1024 * 1024;
            });

            _currentDsn = normalized;

            Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Sentry initialized");
        }
    }

    /// <summary>
    /// Validates a Sentry DSN: it must be an absolute http(s) URI that contains a public key
    /// (user info part) and a project id (non-empty path).
    /// Expected format: <c>https://&lt;publicKey&gt;@host/&lt;projectId&gt;</c>
    /// </summary>
    private static bool IsValidDsn(string? dsn)
    {
        if (string.IsNullOrWhiteSpace(dsn))
            return false;

        if (!Uri.TryCreate(dsn.Trim(), UriKind.Absolute, out var uri))
            return false;

        if (uri.Scheme != Uri.UriSchemeHttps && uri.Scheme != Uri.UriSchemeHttp)
            return false;

        if (string.IsNullOrWhiteSpace(uri.UserInfo))
            return false;

        return uri.Segments.Any(segment => segment.Trim('/').Length > 0);
    }
}
