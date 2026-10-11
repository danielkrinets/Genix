// Встраивает значок в genix.exe (только при сборке под Windows).
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/windows/genix.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=не удалось встроить значок: {e}");
        }
    }
}
