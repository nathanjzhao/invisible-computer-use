using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;

public sealed class OwnershipSample {
    public long foreground;
    public long focus;
    public int x;
    public int y;
    public uint lastInput;
}
public static class NativeObserver {
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int x,y; }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int left,top,right,bottom; }
    [StructLayout(LayoutKind.Sequential)] struct GUIINFO {
        public uint size,flags;
        public IntPtr active,focus,capture,menuOwner,moveSize,caret;
        public RECT caretRect;
    }
    [StructLayout(LayoutKind.Sequential)] struct LASTINPUT { public uint size,time; }
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT point);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] static extern bool GetGUIThreadInfo(uint thread,ref GUIINFO info);
    [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref LASTINPUT info);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window,int command);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window,IntPtr after,int x,int y,int width,int height,uint flags);
    [DllImport("user32.dll")] static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window,out RECT rect);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
    [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from,uint to,bool attach);
    static readonly List<OwnershipSample> Samples = new List<OwnershipSample>();
    static volatile bool Running;
    static volatile string Failure;
    public static string Error { get { return Failure; } }
    static Thread Worker;
    public static OwnershipSample Read() {
        POINT point;
        GUIINFO gui=new GUIINFO { size=(uint)Marshal.SizeOf(typeof(GUIINFO)) };
        LASTINPUT input=new LASTINPUT { size=(uint)Marshal.SizeOf(typeof(LASTINPUT)) };
        if(!GetCursorPos(out point) || !GetGUIThreadInfo(0,ref gui) || !GetLastInputInfo(ref input))
            throw new InvalidOperationException("Desktop ownership could not be read");
        return new OwnershipSample { foreground=GetForegroundWindow().ToInt64(), focus=gui.focus.ToInt64(),
            x=point.x,y=point.y,lastInput=input.time };
    }
    public static void EstablishRecentInput() {
        // Setup only, on the dedicated VM. End the harmless Shift tap before
        // sampling so the normal foreground-lock condition is exercised.
        keybd_event(0x10,0,0,UIntPtr.Zero);
        keybd_event(0x10,0,2,UIntPtr.Zero);
    }
    public static void FocusGuardForSetup(IntPtr window) {
        // Dedicated VM only, before sampling or replying to the model. A real
        // window-manager click establishes the normal foreground-lock state.
        POINT point; RECT rect;
        if(!GetCursorPos(out point) || !GetWindowRect(window,out rect))
            throw new InvalidOperationException("Cannot locate focus guard");
        uint ignored;
        uint current=GetCurrentThreadId();
        uint front=GetWindowThreadProcessId(GetForegroundWindow(),out ignored);
        uint target=GetWindowThreadProcessId(window,out ignored);
        bool attachedFront=false,attachedTarget=false;
        try {
            if(front!=0 && front!=current) attachedFront=AttachThreadInput(current,front,true);
            if(target!=0 && target!=current && target!=front) attachedTarget=AttachThreadInput(current,target,true);
            SetForegroundWindow(window);
            if(!SetCursorPos((rect.left+rect.right)/2,(rect.top+rect.bottom)/2))
                throw new InvalidOperationException("Cannot reach focus guard");
            mouse_event(0x0002,0,0,0,UIntPtr.Zero);
            mouse_event(0x0004,0,0,0,UIntPtr.Zero);
            Thread.Sleep(100);
        } finally {
            if(attachedTarget) AttachThreadInput(current,target,false);
            if(attachedFront) AttachThreadInput(current,front,false);
            if(!SetCursorPos(point.x,point.y)) throw new InvalidOperationException("Cannot restore setup pointer");
        }
    }
    public static void PerturbPointerForNegativeControl() {
        // Dedicated VM only. Restore the pointer before the assertion so the
        // negative control tests the sampler, not just a final-state check.
        POINT point;
        if(!GetCursorPos(out point)) throw new InvalidOperationException("Cannot read pointer");
        try {
            if(!SetCursorPos(point.x+17,point.y+17)) throw new InvalidOperationException("Cannot perturb pointer");
            Thread.Sleep(150);
        } finally {
            if(!SetCursorPos(point.x,point.y)) throw new InvalidOperationException("Cannot restore pointer");
        }
    }
    delegate bool EnumProc(IntPtr window,IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc proc,IntPtr data);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr window,System.Text.StringBuilder name,int size);
    [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
    public static string ClassOf(IntPtr window) {
        var name=new System.Text.StringBuilder(256); GetClassNameW(window,name,256); return name.ToString();
    }
    // A process's visible top-level window of one class and at least a given
    // width. Process.MainWindowHandle is not used: a Tauri app also owns a
    // visible 16x16 "Tao Thread Event Target" window, and MainWindowHandle
    // returned it in 11 of 16 launches in the 2026-09-25 review.
    public static IntPtr FindTopLevel(int process,string className,int minWidth) {
        IntPtr found=IntPtr.Zero;
        EnumWindows((window,data)=>{
            uint owner; GetWindowThreadProcessId(window,out owner);
            RECT rect;
            if(owner==(uint)process && IsWindowVisible(window) && ClassOf(window)==className
                && GetWindowRect(window,out rect) && rect.right-rect.left>=minWidth) { found=window; return false; }
            return true;
        },IntPtr.Zero);
        return found;
    }
    // Every top-level window of a process: "handle class visible WxH".
    public static string[] TopLevels(int process) {
        var found=new List<string>();
        EnumWindows((window,data)=>{
            uint owner; GetWindowThreadProcessId(window,out owner);
            RECT rect;
            if(owner==(uint)process) {
                GetWindowRect(window,out rect);
                found.Add(window.ToInt64()+" "+ClassOf(window)+" "+(IsWindowVisible(window)?"visible":"hidden")+" "+(rect.right-rect.left)+"x"+(rect.bottom-rect.top));
            }
            return true;
        },IntPtr.Zero);
        return found.ToArray();
    }
    public static bool PostClose(IntPtr window) { return PostMessageW(window,0x0010,IntPtr.Zero,IntPtr.Zero); }
    public static void Start() {
        lock(Samples) Samples.Clear();
        Failure=null; Running=true;
        Worker=new Thread(()=> {
            while(Running) {
                try { var sample=Read(); lock(Samples) Samples.Add(sample); }
                catch(Exception error) { Failure=error.Message; Running=false; }
                Thread.Sleep(10);
            }
        });
        Worker.IsBackground=true; Worker.Start();
    }
    public static OwnershipSample[] Stop() {
        Running=false;
        if(Worker!=null) Worker.Join(2000);
        lock(Samples) return Samples.ToArray();
    }
}
