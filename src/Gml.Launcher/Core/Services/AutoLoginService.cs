using System;
using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using Gml.Client.Interfaces;
using Gml.Launcher.Models;
using Sentry;

namespace Gml.Launcher.Core.Services;

public sealed class AutoLoginService : IAutoLoginService
{
    private readonly IGmlClientManager _gmlClientManager;
    private readonly IStorageService _storageService;
    private readonly ISystemService _systemService;

    public AutoLoginService(
        IStorageService storageService,
        IGmlClientManager gmlClientManager,
        ISystemService systemService)
    {
        _storageService = storageService;
        _gmlClientManager = gmlClientManager;
        _systemService = systemService;
    }

    public async Task<SavedLogin?> GetSavedLoginAsync()
    {
        var saved = await _storageService.GetAsync<SavedCredentials>(StorageConstants.SavedCredentials);

        if (saved is null
            || string.IsNullOrWhiteSpace(saved.Login)
            || string.IsNullOrWhiteSpace(saved.ProtectedPassword)
            || !IsCurrentHost(saved.Host))
            return null;

        var password = CredentialProtector.Unprotect(saved.ProtectedPassword);

        return password is null ? null : new SavedLogin(saved.Login, password);
    }

    public async Task SaveCredentialsAsync(string login, string password)
    {
        if (string.IsNullOrWhiteSpace(login) || string.IsNullOrEmpty(password))
            return;

        var credentials = new SavedCredentials
        {
            Host = _gmlClientManager.HostUri.ToString(),
            Login = login,
            ProtectedPassword = CredentialProtector.Protect(password)
        };

        await _storageService.SetAsync(StorageConstants.SavedCredentials, credentials);
    }

    public Task ClearCredentialsAsync()
    {
        return _storageService.SetAsync<SavedCredentials?>(StorageConstants.SavedCredentials, null);
    }

    public async Task<AutoLoginResult> TrySignInAsync()
    {
        var saved = await GetSavedLoginAsync();

        if (saved is null)
            return AutoLoginResult.NoCredentials;

        try
        {
            var authResult = await _gmlClientManager.Auth(saved.Login, saved.Password, _systemService.GetHwid());

            if (authResult.User is { IsAuth: true })
            {
                await _storageService.SetAsync(StorageConstants.User, authResult.User);
                return AutoLoginResult.Success(authResult.User);
            }

            if (authResult.User is { Has2Fa: true })
                return AutoLoginResult.TwoFactorRequired(authResult.Message);

            await _storageService.SetAsync<ILauncherUser?>(StorageConstants.User, null);

            Debug.WriteLine($"[AutoLogin] Sign-in rejected: {authResult.Message}");

            return AutoLoginResult.Failed(authResult.Message);
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[AutoLogin] Sign-in failed: {exception}");
            SentrySdk.CaptureException(exception);

            return AutoLoginResult.Failed(exception.Message);
        }
    }

    private bool IsCurrentHost(string savedHost)
    {
        return Uri.TryCreate(savedHost, UriKind.Absolute, out var savedUri)
               && Uri.TryCreate(_gmlClientManager.HostUri.ToString(), UriKind.Absolute, out var currentUri)
               && savedUri == currentUri;
    }
}

/// <summary>
///     Best-effort local protection for the remembered password. The key is
///     bound to the current machine and Windows user account, so copying the
///     launcher database to another machine does not expose the password.
/// </summary>
internal static class CredentialProtector
{
    private const string Prefix = "v1:";
    private const int KeySize = 32;
    private const int NonceSize = 12;
    private const int TagSize = 16;

    private static readonly byte[] Key = CreateKey();

    public static string Protect(string value)
    {
        var plainBytes = Encoding.UTF8.GetBytes(value);
        var nonce = RandomNumberGenerator.GetBytes(NonceSize);
        var cipherBytes = new byte[plainBytes.Length];
        var tag = new byte[TagSize];

        try
        {
            using var aes = new AesGcm(Key, TagSize);
            aes.Encrypt(nonce, plainBytes, cipherBytes, tag);

            var payload = new byte[NonceSize + TagSize + cipherBytes.Length];
            Buffer.BlockCopy(nonce, 0, payload, 0, NonceSize);
            Buffer.BlockCopy(tag, 0, payload, NonceSize, TagSize);
            Buffer.BlockCopy(cipherBytes, 0, payload, NonceSize + TagSize, cipherBytes.Length);

            return Prefix + Convert.ToBase64String(payload);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(plainBytes);
        }
    }

    public static string? Unprotect(string protectedValue)
    {
        try
        {
            if (!protectedValue.StartsWith(Prefix, StringComparison.Ordinal))
                return null;

            var payload = Convert.FromBase64String(protectedValue[Prefix.Length..]);

            if (payload.Length <= NonceSize + TagSize)
                return null;

            var nonce = payload.AsSpan(0, NonceSize);
            var tag = payload.AsSpan(NonceSize, TagSize);
            var cipherBytes = payload.AsSpan(NonceSize + TagSize);
            var plainBytes = new byte[cipherBytes.Length];

            try
            {
                using var aes = new AesGcm(Key, TagSize);
                aes.Decrypt(nonce, cipherBytes, tag, plainBytes);

                return Encoding.UTF8.GetString(plainBytes);
            }
            finally
            {
                CryptographicOperations.ZeroMemory(plainBytes);
            }
        }
        catch
        {
            return null;
        }
    }

    private static byte[] CreateKey()
    {
        var seed = $"Gml.Launcher.Credentials.v1|{Environment.MachineName}|{Environment.UserName}";

        return SHA256.HashData(Encoding.UTF8.GetBytes(seed))[..KeySize];
    }
}
