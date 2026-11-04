using System;
using System.Diagnostics;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Gml.Launcher.Core.Helpers;
using Gml.Launcher.Core.Services;
using Gml.Launcher.ViewModels;
using Gml.Launcher.Views;
using Gml.Launcher.Views.SplashScreen;

namespace Gml.Launcher;

public partial class App : Application
{
    public override void Initialize()
    {
        AvaloniaXamlLoader.Load(this);
    }

    public override async void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            try
            {
                SoundService.Install();

                var splashViewModel = new SplashScreenViewModel();
                var splashScreen = new SplashScreen
                {
                    DataContext = splashViewModel
                };

                desktop.MainWindow = splashScreen;

                // The lifetime shows the window right after this method yields, so raise it once
                // the window exists: visible on top, but still without stealing focus.
                await Task.Yield();
                WindowTransitions.RaiseWithoutActivation(splashScreen);

                await splashViewModel.InitializeAsync();

                var mainWindow = splashScreen.GetMainWindow();
                desktop.MainWindow = mainWindow;

                // Grow the loading card to the launcher bounds first, so the small window visibly
                // turns into the launcher while the launcher itself is still hidden.
                await splashScreen.GrowToWindowAsync(mainWindow);

                mainWindow.Show();
                WindowTransitions.RaiseWithoutActivation(splashScreen);

                await splashScreen.FadeOutAsync();

                splashScreen.Close();
                WindowTransitions.RaiseWithoutActivation(mainWindow);
            }
            catch (Exception exception)
            {
                Debug.WriteLine($"[Gml] splash transition failed: {exception}");
            }
        }

        base.OnFrameworkInitializationCompleted();
    }
}
