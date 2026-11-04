public static class GlobalVariables
{
    public static bool BackendInactive { get; set; }

    /// <summary>"local", "proxy", "direct" or null while unknown.</summary>
    public static string? BackendMode { get; set; }

    /// <summary>True when the saved service key was accepted, so mode switching is allowed.</summary>
    public static bool CanControlBackend { get; set; }
}
