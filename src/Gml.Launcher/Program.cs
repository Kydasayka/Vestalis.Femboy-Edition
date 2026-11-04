using System;
using System.Diagnostics;
using System.Reactive;
using Avalonia;
using Avalonia.ReactiveUI;
using Gml.Client;
using Gml.Launcher.Assets;
using Gml.Launcher.Core.Extensions;
using ReactiveUI;
using Sentry;

namespace Gml.Launcher;

internal class Program
{
    private const string AppUserModelId = "Vestalis.FemboyEdition.Launcher";

    [System.Runtime.InteropServices.DllImport("shell32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
    private static extern int SetCurrentProcessExplicitAppUserModelID(string appId);

    [STAThread]
#if DEBUG
    public static void Main(string[] args)
    {
        try
        {
            // Tell Windows which app this process is, so the taskbar uses the launcher icon
            // instead of a generic placeholder for a borderless window.
            SetCurrentProcessExplicitAppUserModelID(AppUserModelId);

            Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Application started");
            //InitializeSentry();
            // RxApp.DefaultExceptionHandler = Observer.Create<Exception>(GlobalExceptionHandler);
            BuildAvaloniaApp()
                .StartWithClassicDesktopLifetime(args);
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
            Console.WriteLine(exception);
        }
    }
#else
    public static void Main(string[] args)
    {
        try
        {
            SetCurrentProcessExplicitAppUserModelID(AppUserModelId);

            Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Application started");
            //InitializeSentry();
            RxApp.DefaultExceptionHandler = Observer.Create<Exception>(GlobalExceptionHandler);
            BuildAvaloniaApp()
                .StartWithClassicDesktopLifetime(args);
        }
        catch (Exception exception)
        {
            SentrySdk.CaptureException(exception);
            Console.WriteLine(exception);
        }
    }
#endif

    private static void GlobalExceptionHandler(Exception exception)
    {
        SentrySdk.CaptureException(exception);
    }

#if DEBUG
    public static AppBuilder BuildAvaloniaApp()
    {
        Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Configuring launcher");
        return AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .RegisterServices()
            .LogToTrace()
            .UseReactiveUI();
    }
#else
    public static AppBuilder BuildAvaloniaApp()
    {
        Debug.WriteLine($"[Gml][{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff}] Configuring launcher");
        return AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .RegisterServices()
            .LogToTrace()
            .UseReactiveUI();
    }
#endif
}
