using System;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.ReactiveUI;
using Avalonia.Threading;
using GamerVII.Notification.Avalonia;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Helpers;
using Gml.Launcher.ViewModels;
using Gml.Launcher.ViewModels.Pages;

namespace Gml.Launcher.Views.SplashScreen;

public partial class SplashScreen : ReactiveWindow<SplashScreenViewModel>
{
    private const string EditionBrushKey = "EditionShimmerBrush";

    // Time for the colour sweep to cross the "Femboy Edition" label once.
    private static readonly TimeSpan ShimmerDuration = TimeSpan.FromSeconds(1.6);
    private static readonly TimeSpan ShimmerFrame = TimeSpan.FromMilliseconds(32);

    // The loading card dissolves into the launcher window instead of popping away.
    private static readonly TimeSpan DissolveDuration = TimeSpan.FromMilliseconds(260);

    private readonly DispatcherTimer _shimmerTimer = new() { Interval = ShimmerFrame };
    private readonly LinearGradientBrush? _editionBrush;
    private double _shimmerOffset;

    public SplashScreen()
    {
        InitializeComponent();

        _editionBrush = this.FindResource(EditionBrushKey) as LinearGradientBrush;

        if (_editionBrush is not null)
        {
            _shimmerTimer.Tick += OnShimmerTick;
            _shimmerTimer.Start();
        }

        Closed += (_, _) => _shimmerTimer.Stop();
    }

    public MainWindow GetMainWindow()
    {
        var mainWindow = new MainWindow
        {
            DataContext = new MainWindowViewModel()
        };

        return mainWindow;
    }

    /// <summary>
    /// Turns the loading card into the launcher window: the window expands to the launcher
    /// bounds first, then the card fades out while the launcher content fades in.
    /// </summary>
    public Task PlayOutroAsync()
    {
        _shimmerTimer.Stop();

        return WindowTransitions.FadeAndScaleAsync(RootBorder,
            fromOpacity: 1,
            toOpacity: 0,
            fromScale: 1,
            toScale: 1.04,
            DissolveDuration);
    }

    /// <summary>
    /// Slides the brand gradient. The brush uses the repeat spread, so wrapping the offset
    /// back to 0 continues the pattern without a visible seam.
    /// </summary>
    private void OnShimmerTick(object? sender, EventArgs e)
    {
        if (_editionBrush is null)
            return;

        _shimmerOffset = (_shimmerOffset + ShimmerFrame.TotalSeconds / ShimmerDuration.TotalSeconds) % 1.0;

        _editionBrush.StartPoint = new RelativePoint(_shimmerOffset, 0.5, RelativeUnit.Relative);
        _editionBrush.EndPoint = new RelativePoint(_shimmerOffset + 1.0, 0.5, RelativeUnit.Relative);
    }
}
