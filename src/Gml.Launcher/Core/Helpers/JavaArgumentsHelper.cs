using System;
using System.Text.RegularExpressions;

namespace Gml.Launcher.Core.Helpers;

/// <summary>Helpers for composing Java Virtual Machine launch arguments.</summary>
internal static class JavaArgumentsHelper
{
    private static readonly Regex ClassPathArgument = new(
        @"(?<!\S)(?:-cp|-classpath|--class-path)(?=\s)",
        RegexOptions.IgnoreCase | RegexOptions.Compiled);

    private static readonly Regex LaunchTargetArgument = new(
        @"(?<!\S)(?:-jar|-m|--module)(?=\s)",
        RegexOptions.IgnoreCase | RegexOptions.Compiled);

    private static readonly Regex HeapSizeArgument = new(
        @"(?<!\S)-Xm(?:s|x)[0-9]+(?:[kKmMgGtT])?(?=\s|$)",
        RegexOptions.IgnoreCase | RegexOptions.Compiled);

    /// <summary>
    /// Replaces any pre-existing -Xms/-Xmx flags with values selected in the UI,
    /// then inserts the remaining user-supplied JVM options before the class path.
    /// Heap-size flags in the free-form field are intentionally ignored so the sliders
    /// remain the single source of truth for Xms and Xmx.
    /// </summary>
    public static string ConfigureJvmArguments(
        string? existingArguments,
        string? customJvmArguments,
        int xmsRamValueMb,
        int xmxRamValueMb)
    {
        var arguments = HeapSizeArgument.Replace(existingArguments ?? string.Empty, string.Empty).Trim();
        var custom = HeapSizeArgument.Replace(customJvmArguments ?? string.Empty, string.Empty).Trim();

        var xmx = Math.Max(1024, xmxRamValueMb);
        var xms = Math.Clamp(xmsRamValueMb, 512, xmx);
        var memoryArguments = $"-Xms{xms}M -Xmx{xmx}M";
        var injectedArguments = string.IsNullOrWhiteSpace(custom)
            ? memoryArguments
            : $"{memoryArguments} {custom}";

        var match = ClassPathArgument.Match(arguments);
        if (!match.Success)
            match = LaunchTargetArgument.Match(arguments);

        if (match.Success)
            return arguments.Insert(match.Index, injectedArguments + " ");

        // Fallback for non-standard launch layouts without an explicit class path.
        return string.IsNullOrWhiteSpace(arguments)
            ? injectedArguments
            : injectedArguments + " " + arguments;
    }
}
