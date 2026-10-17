using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

// Holds one synthetic pen device for a while, as any process injecting pen
// input does while it injects (adapted from the round-3 review's R3Pen).
// Measured there: while it is held, GetSystemMetrics(SM_DIGITIZER) goes from
// 0 to 132 (NID_READY | NID_INTEGRATED_PEN) and a WPF window started
// meanwhile sees one tablet.
// Usage: ICUPenDevice.exe <work> <label> <seconds>
// Writes pen-<label>.json with SM_DIGITIZER before and while the device is held.
public static class PenDevice {
    [DllImport("user32.dll", SetLastError = true)] static extern IntPtr CreateSyntheticPointerDevice(int type, uint maxCount, int mode);
    [DllImport("user32.dll")] static extern void DestroySyntheticPointerDevice(IntPtr device);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    const int SM_DIGITIZER = 94, PT_PEN = 3, POINTER_FEEDBACK_DEFAULT = 1;

    public static int Main(string[] args) {
        string work = args[0], label = args[1]; int seconds = int.Parse(args[2]);
        int before = GetSystemMetrics(SM_DIGITIZER);
        IntPtr device = CreateSyntheticPointerDevice(PT_PEN, 1, POINTER_FEEDBACK_DEFAULT);
        int error = Marshal.GetLastWin32Error();
        Thread.Sleep(300);
        int held = GetSystemMetrics(SM_DIGITIZER);
        File.WriteAllText(Path.Combine(work, "pen-" + label + ".json"),
            "{\"before\":" + before + ",\"held\":" + held + ",\"device\":" + device.ToInt64() + ",\"error\":" + error + "}");
        Thread.Sleep(seconds * 1000);
        if (device != IntPtr.Zero) DestroySyntheticPointerDevice(device);
        return device == IntPtr.Zero ? 1 : 0;
    }
}
