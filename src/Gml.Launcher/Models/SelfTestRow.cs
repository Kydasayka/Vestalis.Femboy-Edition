using Avalonia.Media;

namespace Gml.Launcher.Models;

/// <summary>One row of the backend self-test report shown in the settings.</summary>
public class SelfTestRow
{
    public required string Name { get; init; }
    public required string Detail { get; init; }
    public required string StatusText { get; init; }
    public required string Duration { get; init; }
    public required IBrush Brush { get; init; }
}
