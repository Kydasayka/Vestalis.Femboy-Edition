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
/// Every wheel notch starts a short animation towards the target offset instead of jumping there,
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

    // Pixels per wheel notch and the share of the remaining distance covered per frame.
    private const double WheelStep = 120;
    private const double EasingFactor = 0.24;
    private const double StopThreshold = 0.4;

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
        private readonly ScrollViewer _viewer;
        private readonly DispatcherTimer _timer;
        private double _targetOffset;

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

            // Any press (including the one that starts dragging the scrollbar) stops the easing, so
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

            // Notches that arrive while the animation is running add up on the pending target, so
            // fast wheel turns keep their momentum instead of restarting the animation.
            var origin = _timer.IsEnabled ? _targetOffset : _viewer.Offset.Y;
            _targetOffset = Math.Clamp(origin - e.Delta.Y * WheelStep, 0, maxOffset);

            if (!_timer.IsEnabled)
                _timer.Start();
        }

        private void OnPointerPressed(object? sender, PointerPressedEventArgs e)
        {
            _timer.Stop();
        }

        private void OnAnimationTick(object? sender, EventArgs e)
        {
            var distance = _targetOffset - _viewer.Offset.Y;

            if (Math.Abs(distance) <= StopThreshold)
            {
                _viewer.Offset = new Vector(_viewer.Offset.X, _targetOffset);
                _timer.Stop();
                return;
            }

            _viewer.Offset = new Vector(_viewer.Offset.X, _viewer.Offset.Y + distance * EasingFactor);
        }
    }
}
