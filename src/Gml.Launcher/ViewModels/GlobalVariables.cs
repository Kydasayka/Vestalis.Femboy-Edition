public static class GlobalVariables
{
    public static bool BackendInactive { get; set; }

    /// <summary>"local", "proxy", "direct" or null while unknown.</summary>
    public static string? BackendMode { get; set; }
}
