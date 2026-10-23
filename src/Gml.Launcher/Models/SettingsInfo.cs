using System.Globalization;

namespace Gml.Launcher.Models;

public record SettingsInfo
{
    public SettingsInfo()
    {
    }

    public SettingsInfo(int gameWidth,
        int gameHeight,
        bool fullScreen,
        bool isDynamicRam,
        double ramValue,
        string? languageCode,
        string? customJvmArguments = null,
        int? xmsRamValueMb = null,
        int? xmxRamValueMb = null)
    {
        GameHeight = gameHeight;
        GameWidth = gameWidth;
        FullScreen = fullScreen;
        IsDynamicRam = isDynamicRam;
        RamValue = ramValue;
        LanguageCode = languageCode;
        CustomJvmArguments = customJvmArguments ?? string.Empty;
        XmsRamValueMb = xmsRamValueMb;
        XmxRamValueMb = xmxRamValueMb;
    }

    public int GameWidth { get; set; }
    public int GameHeight { get; set; }
    public bool FullScreen { get; set; }

    // Kept for compatibility with settings files created by older launcher versions.
    public bool IsDynamicRam { get; set; }
    public double RamValue { get; set; }
    public string? LanguageCode { get; set; }
    public string CustomJvmArguments { get; set; } = string.Empty;

    /// <summary>Initial Java heap size (-Xms), in megabytes.</summary>
    public int? XmsRamValueMb { get; set; }

    /// <summary>Maximum Java heap size (-Xmx), in megabytes.</summary>
    public int? XmxRamValueMb { get; set; }

    public static SettingsInfo Default =>
        new SettingsInfo(900, 600, false, false, 8192, CultureInfo.CurrentUICulture.Name,
            string.Empty, 2048, 8192);

    public static SettingsInfo Empty => new();
}
