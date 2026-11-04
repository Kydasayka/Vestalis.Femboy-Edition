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

    // The loading card grows to the launcher bounds, then dissolves into the launcher.
    private static readonly TimeSpan GrowDuration = TimeSpan.FromMilliseconds(420);
    private static readonly TimeSpan FadeDuration = TimeSpan.FromMilliseconds(200);

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
    /// <summary>
    /// Turns the loading card into the launcher window. The splash window jumps to the launcher
    /// rect instantly and the card is scaled down to its original size, then grows back to 100%
    /// while dissolving — no per-frame window resizing, so the content never lags behind.
    /// </summary>
    /// <summary>
    /// Grows the loading card to the launcher window bounds. The launcher is shown afterwards,
    /// so nothing of it peeks around the card while it expands.
    /// </summary>
    public Task GrowToWindowAsync(MainWindow targetWindow)
    {
        _shimmerTimer.Stop();

        // The brand lockup dissolves as the card starts growing: once the card is launcher-sized
        // the content would look stretched, and the hand-over reads cleaner with an empty surface.
        _ = WindowTransitions.FadeAsync(CardContent, 1, 0, TimeSpan.FromMilliseconds(180));

        return WindowTransitions.MorphBoundsAsync(this,
            targetWindow.Position,
            targetWindow.Width,
            targetWindow.Height,
            GrowDuration);
    }

    /// <summary>
    /// Dissolves the card to reveal the launcher underneath.
    /// </summary>
    public Task FadeOutAsync()
    {
        return WindowTransitions.FadeAsync(RootBorder, 1, 0, FadeDuration);
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
