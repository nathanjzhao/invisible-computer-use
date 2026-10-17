// Delay Qt5 Wayland accessibility activation until its actual bus is connected.
#include <QDBusConnection>
#include <dlfcn.h>
#include <cstring>
#include <cstdlib>
#include <cstdio>
#include <climits>
#include <fcntl.h>
#include <unistd.h>
static bool compatibility_enabled = false;
__attribute__((constructor)) static void initialize_compatibility() {
    const char *mode = std::getenv("COPILOT_QT5_ACCESSIBILITY_COMPAT");
    if (!mode || std::strcmp(mode, "1")) return;
    const char *number = std::getenv("COPILOT_QT5_COMPAT_FD");
    char *end = nullptr;
    long descriptor = number ? std::strtol(number, &end, 10) : -1;
    char expected[64];
    std::snprintf(expected, sizeof(expected), "/proc/self/fd/%ld", descriptor);
    const char *preload = std::getenv("LD_PRELOAD");
    const int required = F_SEAL_WRITE | F_SEAL_SHRINK | F_SEAL_GROW | F_SEAL_SEAL;
    const int actual = descriptor >= 3 && descriptor <= INT_MAX ? fcntl(descriptor, F_GET_SEALS) : -1;
    if (!number || !end || *end || descriptor < 3 || descriptor > INT_MAX || !preload
        || std::strcmp(preload, expected) || actual < 0 || (actual & required) != required) _exit(124);
    compatibility_enabled = true;
    // The mapping stays loaded. Descendants must not inherit a recycled fd path.
    unsetenv("LD_PRELOAD");
    unsetenv("COPILOT_QT5_ACCESSIBILITY_COMPAT");
    unsetenv("COPILOT_QT5_COMPAT_FD");
    close(descriptor);
}
extern "C" void deferred_active(void *, bool) asm("_ZN22QPlatformAccessibility9setActiveEb");
extern "C" void deferred_active(void *self, bool enabled) {
    using Function = void (*)(void *, bool);
    static auto original = reinterpret_cast<Function>(dlvsym(RTLD_NEXT, "_ZN22QPlatformAccessibility9setActiveEb", "Qt_5_PRIVATE_API"));
    if (!original) _exit(123);
    Dl_info caller{};
    if (enabled && compatibility_enabled && dladdr(__builtin_return_address(0), &caller) && caller.dli_fname
        && std::strstr(caller.dli_fname, "libQt5WaylandClient.so")
        && !QDBusConnection(QStringLiteral("a11y")).isConnected()) {
        enabled = false;
    }
    original(self, enabled);
}
