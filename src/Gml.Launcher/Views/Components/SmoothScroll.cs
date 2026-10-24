using System;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;

namespace Gml.Launcher.Views.Components;

/// <summary>
/// Eased wheel scrolling for a regular <see cref="ScrollViewer"/>.
/// <para>
/// Every wheel notch starts a short glide towards the target offset instead of jumping there,
/// which keeps long lists (mods, settings, news) from feeling jerky. It is an attached property
/// rather than a subclass on purpose: Avalonia resolves the control theme by the exact type, so a
/// derived viewer would fall back to the plain content template and lose scrolling altogether.
/// </para>
/// </summary>
public sealed class SmoothScroll
{
    private SmoothScroll()
    {
    }

    // Pixels per wheel notch.
    private const double WheelStep = 120;

    private static readonly ConditionalWeakTable<ScrollViewer, Scroller> Scrollers = new();

    /// <summary>
    /// Enables eased wheel scrolling on the target viewer.
    /// </summary>
    public static readonly AttachedProperty<bool> IsEnabledProperty =
        AvaloniaProperty.RegisterAttached<SmoothScroll, ScrollViewer, bool>("IsEnabled");

    static SmoothScroll()
    {
        IsEnabledProperty.Changed.AddClassHandler<ScrollViewer>((viewer, args) =>
        {
            if (args.GetNewValue<bool>())
                Attach(viewer);
            else
                Detach(viewer);
        });
    }

    public static bool GetIsEnabled(ScrollViewer viewer) => viewer.GetValue(IsEnabledProperty);

    public static void SetIsEnabled(ScrollViewer viewer, bool value) => viewer.SetValue(IsEnabledProperty, value);

    private static void Attach(ScrollViewer viewer)
    {
        if (Scrollers.TryGetValue(viewer, out _))
            return;

        var scroller = new Scroller(viewer);
        Scrollers.Add(viewer, scroller);
        scroller.Start();
    }

    private static void Detach(ScrollViewer viewer)
    {
        if (!Scrollers.TryGetValue(viewer, out var scroller))
            return;

        scroller.Stop();
        Scrollers.Remove(viewer);
    }

    private sealed class Scroller
    {
        // Time constant of the glide: how fast the offset catches up with the pending target. The
        // step is derived from the real elapsed time, so a dropped frame does not slow the motion
        // down and a fast frame does not overshoot - that is what keeps the scroll even.
        private const double TimeConstantSeconds = 0.055;

        // Below this distance the remaining movement is invisible, so the glide snaps and stops.
        private const double StopThreshold = 0.5;

        private readonly ScrollViewer _viewer;
        private readonly DispatcherTimer _timer;
        private readonly System.Diagnostics.Stopwatch _clock = System.Diagnostics.Stopwatch.StartNew();

        private double _targetOffset;
        private long _lastTickMs;

        public Scroller(ScrollViewer viewer)
        {
            _viewer = viewer;

            _timer = new DispatcherTimer(
                TimeSpan.FromMilliseconds(16),
                DispatcherPriority.Render,
                OnAnimationTick);

            _timer.Stop();
        }

        public void Start()
        {
            // The ScrollContentPresenter in the template handles the bubbling wheel event and marks
            // it handled, so the viewer itself would never see it. Tunnelling gets us the event
            // first, which is also why the handler marks it handled - otherwise the presenter would
            // jump straight to the new offset.
            _viewer.AddHandler(
                InputElement.PointerWheelChangedEvent,
                OnWheel,
                RoutingStrategies.Tunnel,
                handledEventsToo: true);

            // Any press (including the one that starts dragging the scrollbar) stops the glide, so
            // the animation never fights with the pointer.
            _viewer.AddHandler(
                InputElement.PointerPressedEvent,
                OnPointerPressed,
                RoutingStrategies.Tunnel,
                handledEventsToo: true);
        }

        public void Stop()
        {
            _timer.Stop();

            _viewer.RemoveHandler(InputElement.PointerWheelChangedEvent, OnWheel);
            _viewer.RemoveHandler(InputElement.PointerPressedEvent, OnPointerPressed);
        }

        private void OnWheel(object? sender, PointerWheelEventArgs e)
        {
            var maxOffset = Math.Max(0, _viewer.Extent.Height - _viewer.Viewport.Height);

            // Nothing to scroll: leave the event alone so the default handling stays untouched.
            if (e.Delta.Y == 0 || maxOffset <= 0)
                return;

            e.Handled = true;

            // Notches that arrive while the glide is running add up on the pending target, so fast
            // wheel turns keep their momentum instead of restarting the animation.
            var origin = _timer.IsEnabled ? _targetOffset : _viewer.Offset.Y;
            _targetOffset = Math.Clamp(origin - e.Delta.Y * WheelStep, 0, maxOffset);

            if (!_timer.IsEnabled)
            {
                _lastTickMs = _clock.ElapsedMilliseconds;
                _timer.Start();
            }
        }

        private void OnPointerPressed(object? sender, PointerPressedEventArgs e)
        {
            _timer.Stop();
        }

        private void OnAnimationTick(object? sender, EventArgs e)
        {
            var now = _clock.ElapsedMilliseconds;
            var elapsedMs = Math.Max(1, now - _lastTickMs);
            _lastTickMs = now;

            var offset = _viewer.Offset.Y;
            var distance = _targetOffset - offset;

            if (Math.Abs(distance) <= StopThreshold)
            {
                _viewer.Offset = new Vector(_viewer.Offset.X, _targetOffset);
                _timer.Stop();
                return;
            }

            // Exponential approach, frame-rate independent: the share of the remaining distance
            // covered in this frame depends on how long the frame actually took.
            var alpha = 1 - Math.Exp(-elapsedMs / 1000.0 / TimeConstantSeconds);
            var step = distance * alpha;

            _viewer.Offset = new Vector(_viewer.Offset.X, offset + step);
        }
    }
}
