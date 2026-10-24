using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Selection;
using Avalonia.Interactivity;
using Avalonia.Platform;
using Gml.Launcher.Views.Components;

namespace Gml.Launcher.Core.Services;

/// <summary>
/// Plays the short interface feedback sounds that live in <c>Assets/Sounds</c> and wires them to
/// the controls that are used across the launcher. Sounds are optional and can be turned off from
/// the settings page; on non-Windows platforms playback is a no-op.
/// </summary>
public static class SoundService
{
    private const string ClickAsset = "avares://Gml.Launcher/Assets/Sounds/click.wav";
    private const string ToggleAsset = "avares://Gml.Launcher/Assets/Sounds/toggle.wav";

    private const uint SndAsync = 0x0001;
    private const uint SndNoDefault = 0x0002;
    private const uint SndMemory = 0x0004;

    // Never interrupt a sound that is still playing: cutting a short sample in half is what makes
    // rapid clicking sound glitchy, so a new request is simply dropped while the old one finishes.
    private const uint SndNoStop = 0x0010;

    // A single user action can surface as several routed events (a combo box click also changes
    // the selection), so anything inside this window is treated as the same action.
    private const int MinIntervalMs = 50;

    private static readonly object SyncRoot = new();

    // PlaySound(SND_MEMORY | SND_ASYNC) keeps reading the buffer after the call returns, so the
    // pinned handles must live for the whole process lifetime.
    private static readonly List<GCHandle> PinnedBuffers = [];

    private static readonly Lazy<IntPtr> ClickSound =
        new(() => Load(ClickAsset), isThreadSafe: true);

    private static readonly Lazy<IntPtr> ToggleSound =
        new(() => Load(ToggleAsset), isThreadSafe: true);

    private static volatile bool _isEnabled = true;
    private static bool _isInstalled;
    private static long _lastPlaybackMs = -MinIntervalMs;

    /// <summary>Gets or sets whether interface sounds are played.</summary>
    public static bool IsEnabled
    {
        get => _isEnabled;
        set => _isEnabled = value;
    }

    /// <summary>
    /// Subscribes to the click/selection events of the common controls. Safe to call more than once.
    /// </summary>
    public static void Install()
    {
        if (_isInstalled)
            return;

        _isInstalled = true;

        Button.ClickEvent.AddClassHandler<Button>(OnButtonClick);
        GmlButton.ClickEvent.AddClassHandler<GmlButton>(OnGmlButtonClick);
        ComboBox.SelectionChangedEvent.AddClassHandler<ComboBox>(OnSelectionChanged);
        ListBox.SelectionChangedEvent.AddClassHandler<ListBox>(OnSelectionChanged);
    }

    public static void PlayClick()
    {
        Play(ClickSound);
    }

    public static void PlayToggle()
    {
        Play(ToggleSound);
    }

    private static void OnButtonClick(Button button, RoutedEventArgs args)
    {
        // The event bubbles through composite controls, so react only to the button that raised it.
        if (!ReferenceEquals(args.Source, button))
            return;

        if (IsSilent(button))
            return;

        if (button is ToggleButton)
            PlayToggle();
        else
            PlayClick();
    }

    private static void OnGmlButtonClick(GmlButton button, RoutedEventArgs args)
    {
        if (!ReferenceEquals(args.Source, button))
            return;

        PlayClick();
    }

    private static void OnSelectionChanged(object? sender, SelectionChangedEventArgs args)
    {
        if (args.AddedItems is { Count: > 0 })
            PlayClick();
    }

    /// <summary>
    /// Parts of compound controls (scrollbars, sliders, the drop-down arrow of a combo box) emit
    /// clicks while the user drags or scrolls. Those are not discrete actions, so they stay silent.
    /// </summary>
    private static bool IsSilent(Button button)
    {
        return button is RepeatButton
               || button.TemplatedParent is ScrollBar
               || button.TemplatedParent is Slider
               || button.TemplatedParent is ComboBox;
    }

    private static void Play(Lazy<IntPtr> sound)
    {
        if (!_isEnabled || !OperatingSystem.IsWindows())
            return;

        var now = Environment.TickCount64;

        if (now - Interlocked.Read(ref _lastPlaybackMs) < MinIntervalMs)
            return;

        Interlocked.Exchange(ref _lastPlaybackMs, now);

        try
        {
            var buffer = sound.Value;

            if (buffer == IntPtr.Zero)
                return;

            PlaySound(buffer, IntPtr.Zero, SndAsync | SndMemory | SndNoDefault | SndNoStop);
        }
        catch (Exception exception)
        {
            Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Sound playback failed: {exception.Message}");
        }
    }

    private static IntPtr Load(string asset)
    {
        try
        {
            using var assetStream = AssetLoader.Open(new Uri(asset));
            using var buffer = new MemoryStream();
            assetStream.CopyTo(buffer);

            var data = buffer.ToArray();
            var handle = GCHandle.Alloc(data, GCHandleType.Pinned);

            lock (SyncRoot)
                PinnedBuffers.Add(handle);

            return handle.AddrOfPinnedObject();
        }
        catch (Exception exception)
        {
            Debug.WriteLine(
                $"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Sound asset '{asset}' is unavailable: {exception.Message}");

            return IntPtr.Zero;
        }
    }

    [DllImport("winmm.dll", EntryPoint = "PlaySoundW", SetLastError = true)]
    private static extern bool PlaySound(IntPtr sound, IntPtr module, uint flags);
}
