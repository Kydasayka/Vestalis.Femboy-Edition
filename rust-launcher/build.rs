fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");

    #[cfg(windows)]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/logo.ico");
        resource
            .compile()
            .expect("failed to embed the Windows executable icon");
    }
}
