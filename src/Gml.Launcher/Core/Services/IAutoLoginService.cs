using System.Threading.Tasks;
using Gml.Client.Interfaces;
using Gml.Launcher.Models;

namespace Gml.Launcher.Core.Services;

public enum AutoLoginStatus
{
    NoCredentials,
    Success,
    TwoFactorRequired,
    Failed
}

public sealed record AutoLoginResult(AutoLoginStatus Status, ILauncherUser? User = null, string Message = "")
{
    public bool IsAuthenticated => Status == AutoLoginStatus.Success && User?.IsAuth == true;
    public bool RequiresTwoFactor => Status == AutoLoginStatus.TwoFactorRequired;

    public static AutoLoginResult NoCredentials { get; } = new(AutoLoginStatus.NoCredentials);

    public static AutoLoginResult Success(ILauncherUser user) =>
        new(AutoLoginStatus.Success, user);

    public static AutoLoginResult TwoFactorRequired(string message = "") =>
        new(AutoLoginStatus.TwoFactorRequired, Message: message);

    public static AutoLoginResult Failed(string message = "") =>
        new(AutoLoginStatus.Failed, Message: message);
}

/// <summary>
///     Stores the credentials used for the first successful sign-in and replays
///     the sign-in request when the session is no longer accepted by the backend.
/// </summary>
public interface IAutoLoginService
{
    Task<SavedLogin?> GetSavedLoginAsync();
    Task SaveCredentialsAsync(string login, string password);
    Task ClearCredentialsAsync();
    Task<AutoLoginResult> TrySignInAsync();
}
