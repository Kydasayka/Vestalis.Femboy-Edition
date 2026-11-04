using System;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Markup.Xaml;
using Avalonia.ReactiveUI;
using Gml.Launcher.Core.Helpers;
using Gml.Launcher.ViewModels;
using ReactiveUI;

namespace Gml.Launcher.Views;

public partial class MainWindow : ReactiveWindow<MainWindowViewModel>
{
    private static readonly TimeSpan IntroDuration = TimeSpan.FromMilliseconds(220);
    private static readonly TimeSpan IntroDelay = TimeSpan.FromMilliseconds(240);

    public MainWindow()
    {
        this.WhenActivated(_ => { });
        AvaloniaXamlLoader.Load(this);

#if DEBUG
        this.AttachDevTools();
#endif
    }

    protected override void OnDataContextChanged(EventArgs e)
    {
        base.OnDataContextChanged(e);

        if (DataContext is MainWindowViewModel viewModel)
            viewModel.GameLaunched.Subscribe(isActive =>
            {
                if (isActive)
                    Hide();
                else
                    Show();
            });
    }

    protected override void OnClosed(EventArgs e)
    {
        SendCloseEvent();

        base.OnClosed(e);
    }

    private void SendCloseEvent()
    {
        if (DataContext is MainWindowViewModel viewModel)
            viewModel.OnClosed.OnNext(false);
    }

    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        // The window has no system frame, so the resize grips are implemented by hand.
        if (TryBeginResize(e))
            return;

        if (e.Source is Rectangle rectangle)
        {
            BeginMoveDrag(e);
        }
    }

    protected override void OnPointerMoved(PointerEventArgs e)
    {
        base.OnPointerMoved(e);

        if (WindowState == WindowState.Maximized)
        {
            Cursor = Cursor.Default;
            return;
        }

        var edge = HitTestEdge(e.GetPosition(this));

        Cursor = edge switch
        {
            WindowEdge.North or WindowEdge.South => new Cursor(StandardCursorType.SizeNorthSouth),
            WindowEdge.West or WindowEdge.East => new Cursor(StandardCursorType.SizeWestEast),
            WindowEdge.NorthWest or WindowEdge.SouthEast => new Cursor(StandardCursorType.TopLeftCorner),
            WindowEdge.NorthEast or WindowEdge.SouthWest => new Cursor(StandardCursorType.TopRightCorner),
            _ => Cursor.Default
        };
    }

    private bool TryBeginResize(PointerPressedEventArgs e)
    {
        if (WindowState == WindowState.Maximized)
            return false;

        var edge = HitTestEdge(e.GetPosition(this));
        if (edge is null)
            return false;

        BeginResizeDrag(edge.Value, e);
        return true;
    }

    private WindowEdge? HitTestEdge(Point position)
    {
        const double grip = 6;

        var width = Bounds.Width;
        var height = Bounds.Height;

        var left = position.X <= grip;
        var right = position.X >= width - grip;
        var top = position.Y <= grip;
        var bottom = position.Y >= height - grip;

        if (left && top) return WindowEdge.NorthWest;
        if (right && top) return WindowEdge.NorthEast;
        if (left && bottom) return WindowEdge.SouthWest;
        if (right && bottom) return WindowEdge.SouthEast;
        if (left) return WindowEdge.West;
        if (right) return WindowEdge.East;
        if (top) return WindowEdge.North;
        if (bottom) return WindowEdge.South;

        return null;
    }

    private void MinimizeClick(object? sender, RoutedEventArgs e)
    {
        WindowState = WindowState.Minimized;
    }

    private void MaximizeClick(object? sender, RoutedEventArgs e)
    {
        WindowState = WindowState == WindowState.Maximized
            ? WindowState.Normal
            : WindowState.Maximized;
    }

    private void CloseClick(object? sender, RoutedEventArgs e)
    {
        Close();
    }

    private void WindowClosed(object? sender, EventArgs e)
    {
        SendCloseEvent();
    }
}
