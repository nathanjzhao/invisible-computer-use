fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        for variable in ["CXX", "PKG_CONFIG_PATH", "PKG_CONFIG_SYSROOT_DIR", "PKG_CONFIG_ALLOW_CROSS"] {
            println!("cargo:rerun-if-env-changed={variable}");
        }
        for file in ["bridge.cpp", "exports.map", "build.py"] {
            println!("cargo:rerun-if-changed=qt5-compat/{file}");
        }
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("libcopilot-qt5.so");
        let status = std::process::Command::new("python3")
            .arg("qt5-compat/build.py").arg(output).status()
            .expect("Qt5 compatibility build requires Python 3, a C++ compiler and Qt5DBus development files");
        assert!(status.success(), "Qt5 accessibility compatibility build failed");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Tauri embeds resources in the app binary, but the library's unit-test
        // executable also links dialogs that require Common Controls v6.
        // Without its own activation manifest, Windows rejects the test process
        // before main with STATUS_ENTRYPOINT_NOT_FOUND for TaskDialogIndirect.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // Let the linker supply this same manifest to every executable. Keep
        // Tauri's icon and version resources, without a duplicate manifest in
        // the production binary's resource object.
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        ))
        .expect("failed to build application resources");
        return;
    }
    tauri_build::build()
}
