# Qt5 accessibility compatibility

Some Qt5 Wayland builds activate their accessibility bridge before the actual
accessibility bus is connected. This creates an adapter on a disconnected bus
and leaves ordinary fields unavailable. The compatibility library defers that
premature activation. It does not implement capture or record input.

From a Wayland desktop session, launch an affected Qt5 application with:

```sh
/path/to/Copilot-executable --qt5-accessibility -- /path/to/qt5-application [arguments]
```

Use the target application's normal arguments. Close existing instances first
if the application forwards new launches to an existing process. The explicit
launcher replaces its process with the target application. Normal Copilot
launches do not enable this mode. Recording still uses the existing History
consent, rules and collector.

The helper is compiled into the app, passed through a sealed file descriptor,
and applies only to Qt5 Wayland bridge activation. It does not replace system
libraries or change desktop accessibility settings. Set-user/group targets and
existing preload overrides are refused. Loader auditing is preserved. App-private configuration and
model credentials are removed from the launched process's environment. The
helper removes its own preload environment and closes its sealed descriptor
after loading, so descendant processes do not inherit the override.

Native Linux builds require Python 3, a C++ compiler, pkg-config and Qt5DBus
headers, supplied by `qtbase5-dev` on Ubuntu or `qt5-qtbase-devel` on Fedora.
The target application's Qt5 runtime must be available to the dynamic loader.
This mode has been developed against the affected Qt 5.15 Wayland bridge; it is
not a general workaround for applications without an accessibility provider.
