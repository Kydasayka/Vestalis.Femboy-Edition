using System;
using System.Reactive.Disposables;
using Avalonia.Controls;
using Avalonia.Markup.Xaml;
using Avalonia.Media;
using Avalonia.ReactiveUI;
using Avalonia.Threading;
using Gml.Launcher.ViewModels.Pages;
using ReactiveUI;

namespace Gml.Launcher.Views.Pages;

public partial class OverviewPageView : ReactiveUserControl<OverviewPageViewModel>
{
    private Control? _profileHost;
    private TranslateTransform? _profileShift;

    public OverviewPageView()
    {
        this.WhenActivated(disposables =>
        {
            _profileHost ??= this.FindControl<Control>("ProfileHost");
            _profileShift ??= _profileHost?.RenderTransform as TranslateTransform;

            ViewModel?.ListViewModel.ProfileChanged
                .Subscribe(_ => AnimateProfileChange())
                .DisposeWith(disposables);
        });

        AvaloniaXamlLoader.Load(this);
    }

    /// <summary>
    /// Fades and lifts the profile block whenever another profile is picked, so switching builds
    /// reads as a transition instead of the text swapping in place.
    /// </summary>
    private void AnimateProfileChange()
    {
        if (_profileHost is null || _profileShift is null)
            return;

        // The profile list loads in the background and raises this event from a worker thread.
        // Touching Transitions from there throws "Call from invalid thread", so hop to the UI thread.
        if (!Dispatcher.UIThread.CheckAccess())
        {
            Dispatcher.UIThread.Post(AnimateProfileChange);
            return;
        }

        var hostTransitions = _profileHost.Transitions;
        var shiftTransitions = _profileShift.Transitions;

        // Jump to the starting state without easing towards it, then let the transitions run back.
        _profileHost.Transitions = null;
        _profileShift.Transitions = null;
        _profileHost.Opacity = 0.1;
        _profileShift.Y = 18;

        Dispatcher.UIThread.Post(() =>
        {
            _profileHost.Transitions = hostTransitions;
            _profileShift.Transitions = shiftTransitions;
            _profileHost.Opacity = 1;
            _profileShift.Y = 0;
        }, DispatcherPriority.Render);
    }
}
