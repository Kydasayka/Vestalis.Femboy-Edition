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
            SoundService.Install();

            var splashViewModel = new SplashScreenViewModel();
            var splashScreen = new SplashScreen
            {
                DataContext = splashViewModel
            };

            desktop.MainWindow = splashScreen;
            await splashViewModel.InitializeAsync();

            // The launcher must not steal focus from whatever the user is watching, but it still
            // has to be visible, so it is raised in the z-order without being activated.
            WindowTransitions.RaiseWithoutActivation(splashScreen);

            var mainWindow = splashScreen.GetMainWindow();
            mainWindow.PrepareIntro();
            desktop.MainWindow = mainWindow;
            mainWindow.Show();
            WindowTransitions.RaiseWithoutActivation(mainWindow);

            // The splash grows into the launcher window while its content dissolves, then the
            // launcher content fades in. A broken animation must never strand the user on the
            // splash, hence the guard.
            try
            {
                await Task.WhenAll(
                    splashScreen.PlayOutroAsync(mainWindow.Position, mainWindow.Width, mainWindow.Height),
                    mainWindow.PlayIntroAsync());
            }
            catch (Exception exception)
            {
                Debug.WriteLine($"[Gml] splash transition failed: {exception}");
            }

            splashScreen.Close();
        }

        base.OnFrameworkInitializationCompleted();
    }
}
