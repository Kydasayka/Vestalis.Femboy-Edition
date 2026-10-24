namespace Gml.Launcher.Models;

/// <summary>
///     Credentials remembered after a successful sign-in so the launcher can
///     re-authenticate silently when the backend invalidates the session.
/// </summary>
public sealed class SavedCredentials
{
    public string Host { get; set; } = string.Empty;
    public string Login { get; set; } = string.Empty;
    public string ProtectedPassword { get; set; } = string.Empty;
}

public sealed record SavedLogin(string Login, string Password);
