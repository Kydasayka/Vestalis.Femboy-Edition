using System;
using System.Diagnostics;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Animation;
using Avalonia.Animation.Easings;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Styling;
using Avalonia.Threading;

namespace Gml.Launcher.Core.Helpers;

/// <summary>
/// Transition helpers used when the splash hands the screen over to the main window.
/// </summary>
public static class WindowTransitions
{
    /// <summary>
    /// Fades and zooms a control. Used to dissolve the splash card and to reveal the
    /// launcher content.
    /// </summary>
    public static async Task FadeAndScaleAsync(Control control,
        double fromOpacity,
        double toOpacity,
        double fromScale,
        double toScale,
        TimeSpan duration)
    {
        var scale = new ScaleTransform(fromScale, fromScale);

        control.Opacity = fromOpacity;
        control.RenderTransformOrigin = RelativePoint.Center;
        control.RenderTransform = scale;

        var fade = new Animation
        {
            Duration = duration,
            FillMode = FillMode.Forward,
            Easing = new CubicEaseOut(),
            Children =
            {
                new KeyFrame
                {
                    Cue = new Cue(0d),
                    Setters = { new Setter(Visual.OpacityProperty, fromOpacity) }
                },
                new KeyFrame
                {
                    Cue = new Cue(1d),
                    Setters = { new Setter(Visual.OpacityProperty, toOpacity) }
                }
            }
        };

        var zoom = new Animation
        {
            Duration = duration,
            FillMode = FillMode.Forward,
            Easing = new CubicEaseOut(),
            Children =
            {
                new KeyFrame
                {
                    Cue = new Cue(0d),
                    Setters =
                    {
                        new Setter(ScaleTransform.ScaleXProperty, fromScale),
                        new Setter(ScaleTransform.ScaleYProperty, fromScale)
                    }
                },
                new KeyFrame
                {
                    Cue = new Cue(1d),
                    Setters =
                    {
                        new Setter(ScaleTransform.ScaleXProperty, toScale),
                        new Setter(ScaleTransform.ScaleYProperty, toScale)
                    }
                }
            }
        };

        await Task.WhenAll(fade.RunAsync(control), zoom.RunAsync(scale));

        control.Opacity = toOpacity;
    }

    /// <summary>
    /// Puts the window on top of the z-order without taking focus, so the launcher stays
    /// visible for the user while their other apps keep keyboard focus.
    /// </summary>
    public static void RaiseWithoutActivation(Window window)
    {
        if (!OperatingSystem.IsWindows())
            return;

        var handle = window.TryGetPlatformHandle()?.Handle ?? IntPtr.Zero;
        if (handle == IntPtr.Zero)
            return;

        // HWND_TOP, keep position and size, do not activate.
        SetWindowPos(handle, IntPtr.Zero, 0, 0, 0, 0, SwpNoMove | SwpNoSize | SwpNoActivate | SwpShowWindow);
        HideSystemBorder(handle);
    }

    /// <summary>
    /// Windows 11 paints a thin matte/accent border around windows. The launcher draws its own
    /// rounded surface, so that border is switched off (ignored on older Windows).
    /// </summary>
    private static void HideSystemBorder(IntPtr handle)
    {
        const int DwmwaBorderColor = 34;
        const uint DwmColorNone = 0xFFFFFFFE;

        var color = DwmColorNone;

        try
        {
            DwmSetWindowAttribute(handle, DwmwaBorderColor, ref color, sizeof(uint));
        }
        catch (DllNotFoundException)
        {
            // dwmapi is always present on Windows; nothing to do on other platforms.
        }
    }

    private const uint SwpNoSize = 0x0001;
    private const uint SwpNoMove = 0x0002;
    private const uint SwpNoActivate = 0x0010;
    private const uint SwpShowWindow = 0x0040;

    [System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetWindowPos(IntPtr window, IntPtr insertAfter,
        int x, int y, int width, int height, uint flags);

    [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr window, int attribute,
        ref uint value, int size);

    /// <summary>
    /// Grows the splash window to the bounds of the launcher window, so the small loading
    /// card visibly turns into the launcher instead of being replaced by it.
    /// </summary>
    public static async Task MorphBoundsAsync(Window window,
        PixelPoint targetPosition,
        double targetWidth,
        double targetHeight,
        TimeSpan duration)
    {
        var startPosition = window.Position;
        var startWidth = window.Width;
        var startHeight = window.Height;

        var stopwatch = Stopwatch.StartNew();
        var completion = new TaskCompletionSource();

        var timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(16) };

        timer.Tick += (_, _) =>
        {
            var progress = Math.Clamp(stopwatch.Elapsed.TotalMilliseconds / duration.TotalMilliseconds, 0, 1);
            var eased = 1 - Math.Pow(1 - progress, 3);

            window.Width = startWidth + (targetWidth - startWidth) * eased;
            window.Height = startHeight + (targetHeight - startHeight) * eased;
            window.Position = new PixelPoint(
                (int)Math.Round(startPosition.X + (targetPosition.X - startPosition.X) * eased),
                (int)Math.Round(startPosition.Y + (targetPosition.Y - startPosition.Y) * eased));

            if (progress < 1)
                return;

            timer.Stop();
            completion.TrySetResult();
        };

        timer.Start();
        await completion.Task;
    }
}
