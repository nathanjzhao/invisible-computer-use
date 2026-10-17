using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

// Read and arrange the interactive session's monitors: positions through
// ChangeDisplaySettingsEx and per-monitor scale through the display-config
// DPI records that the Settings app uses. Setup only, on a disposable host.
public sealed class MonitorRecord {
    public string device; public bool primary;
    public int left, top, right, bottom, dpi;
    public int scale_percent; public long adapter; public uint source;
}
public static class DisplayLayout {
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int left,top,right,bottom; }
    [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct MONITORINFOEX {
        public uint size; public RECT monitor, work; public uint flags;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string device; }
    [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string deviceName;
        public ushort specVersion,driverVersion,size,driverExtra; public uint fields;
        public int positionX,positionY; public uint orientation,fixedOutput;
        public short color,duplex,yResolution,ttOption,collate;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string formName;
        public ushort logPixels; public uint bitsPerPel,pelsWidth,pelsHeight,displayFlags,displayFrequency;
        public uint icmMethod,icmIntent,mediaType,ditherType,reserved1,reserved2,panningWidth,panningHeight; }
    [StructLayout(LayoutKind.Sequential)] struct LUID { public uint low; public int high; }
    [StructLayout(LayoutKind.Sequential)] struct HEADER { public int type; public uint size; public LUID adapter; public uint id; }
    [StructLayout(LayoutKind.Sequential)] struct DPI_GET { public HEADER header; public int min, cur, max; }
    [StructLayout(LayoutKind.Sequential)] struct DPI_SET { public HEADER header; public int scale; }
    [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct SOURCE_NAME { public HEADER header;
        [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)] public string device; }
    [StructLayout(LayoutKind.Sequential)] struct PATH {
        public LUID sourceAdapter; public uint sourceId, sourceMode, sourceFlags;
        public LUID targetAdapter; public uint targetId, targetMode, outputTechnology, rotation, scaling;
        public uint refreshNum, refreshDen, scanline, targetAvailable, targetFlags, flags; }
    [StructLayout(LayoutKind.Explicit,Size=64)] struct MODE { [FieldOffset(0)] public uint infoType; }
    delegate bool MonitorProc(IntPtr monitor,IntPtr dc,ref RECT rect,IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr dc,IntPtr clip,MonitorProc proc,IntPtr data);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool GetMonitorInfoW(IntPtr monitor,ref MONITORINFOEX info);
    [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr monitor,int type,out uint x,out uint y);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern bool EnumDisplaySettingsW(string device,int mode,ref DEVMODE devmode);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int ChangeDisplaySettingsExW(string device,ref DEVMODE devmode,IntPtr window,uint flags,IntPtr param);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int ChangeDisplaySettingsExW(string device,IntPtr devmode,IntPtr window,uint flags,IntPtr param);
    [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags,out uint paths,out uint modes);
    [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags,ref uint paths,[Out] PATH[] pathArray,ref uint modes,[Out] MODE[] modeArray,IntPtr topology);
    [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(ref DPI_GET info);
    [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(ref SOURCE_NAME info);
    [DllImport("user32.dll")] static extern int DisplayConfigSetDeviceInfo(ref DPI_SET info);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr context);
    [StructLayout(LayoutKind.Sequential)] struct POINT { public int x,y; }
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr window,IntPtr after,int x,int y,int width,int height,uint flags);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr window,out RECT rect);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr window,out RECT rect);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr window,ref POINT point);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr window);
    static readonly int[] Scales = {100,125,150,175,200,225,250,300,350,400,450,500};

    // Usage: ICUDisplayLayout.exe list | scale <device> <percent> |
    //        arrange <primary> <device> <x> <y> <width|0> <height|0> [...] |
    //        move <hwnd> <x> <y> [<width> <height>] | window <hwnd>
    // Window commands use physical pixels, so the caller's own DPI mode
    // cannot rescale a placement on a mixed-DPI desktop.
    public static int Main(string[] args) {
        // Physical coordinates for every monitor, whatever its scale. Fails
        // harmlessly when the process already has an awareness context.
        SetProcessDpiAwarenessContext(new IntPtr(-4));
        var json=new System.Web.Script.Serialization.JavaScriptSerializer();
        if(args[0]=="move") {
            IntPtr target=new IntPtr(long.Parse(args[1]));
            bool size=args.Length>5;
            // SWP_NOZORDER|SWP_NOACTIVATE, plus SWP_NOSIZE for a pure move.
            if(!SetWindowPos(target,IntPtr.Zero,int.Parse(args[2]),int.Parse(args[3]),size?int.Parse(args[4]):0,size?int.Parse(args[5]):0,size?0x14u:0x15u))
                throw new Exception("move failed");
            args=new[]{"window",args[1]};
        }
        if(args[0]=="window") {
            IntPtr target=new IntPtr(long.Parse(args[1])); RECT rect, client; POINT origin=new POINT();
            GetWindowRect(target,out rect); GetClientRect(target,out client); ClientToScreen(target,ref origin);
            Console.WriteLine(json.Serialize(new { left=rect.left, top=rect.top, right=rect.right, bottom=rect.bottom,
                client_left=origin.x, client_top=origin.y, client_width=client.right, client_height=client.bottom,
                dpi=GetDpiForWindow(target), iconic=IsIconic(target) }));
            return 0;
        }
        if(args[0]=="colors") {
            // Centroid of each fixture color in a driver screenshot: the click
            // point is derived from pixels the driver returned, not from layout.
            using(var image=new System.Drawing.Bitmap(args[1])) {
                var found=new List<object>();
                for(int i=0;i<DpiFixture.Colors.Length;i++) {
                    var c=DpiFixture.Colors[i]; long sx=0,sy=0,n=0,opaque=0;
                    int minx=-1,miny=-1,maxx=-1,maxy=-1;
                    for(int y=0;y<image.Height;y++) for(int x=0;x<image.Width;x++) {
                        var p=image.GetPixel(x,y);
                        if(Math.Abs(p.R-c.R)<=6 && Math.Abs(p.G-c.G)<=6 && Math.Abs(p.B-c.B)<=6) {
                            sx+=x; sy+=y; if(p.A==255) opaque++;
                            if(n==0){minx=maxx=x;miny=maxy=y;} else {minx=Math.Min(minx,x);maxx=Math.Max(maxx,x);miny=Math.Min(miny,y);maxy=Math.Max(maxy,y);}
                            n++;
                        }
                    }
                    // Opaque count: a transparent PNG is a blank image to a viewer or model.
                    // The bounding box locates points other than the centroid (a region's corner).
                    found.Add(new { button=((char)('A'+i)).ToString(), pixels=n, opaque, x=n>0?(double)sx/n:-1, y=n>0?(double)sy/n:-1, minx, miny, maxx, maxy });
                }
                Console.WriteLine(json.Serialize(new { width=image.Width, height=image.Height, buttons=found }));
            }
            return 0;
        }
        if(args[0]=="shot") {
            // The whole virtual desktop in physical pixels, including monitors
            // at negative coordinates.
            int left=int.MaxValue,top=int.MaxValue,right=int.MinValue,bottom=int.MinValue;
            foreach(var m in List()) { left=Math.Min(left,m.left); top=Math.Min(top,m.top); right=Math.Max(right,m.right); bottom=Math.Max(bottom,m.bottom); }
            using(var image=new System.Drawing.Bitmap(right-left,bottom-top))
            using(var graphics=System.Drawing.Graphics.FromImage(image)) {
                graphics.CopyFromScreen(left,top,0,0,image.Size);
                image.Save(args[1],System.Drawing.Imaging.ImageFormat.Png);
            }
            Console.WriteLine(json.Serialize(new { left, top, right, bottom }));
            return 0;
        }
        if(args[0]=="scale") SetScale(args[1],int.Parse(args[2]));
        else if(args[0]=="arrange") {
            int n=(args.Length-2)/5; var d=new string[n]; var x=new int[n]; var y=new int[n]; var w=new int[n]; var h=new int[n];
            for(int i=0;i<n;i++) { int o=2+i*5; d[i]=args[o]; x[i]=int.Parse(args[o+1]); y[i]=int.Parse(args[o+2]); w[i]=int.Parse(args[o+3]); h[i]=int.Parse(args[o+4]); }
            Arrange(d,x,y,w,h,args[1]);
        } else if(args[0]!="list") throw new ArgumentException("unknown command");
        Console.WriteLine(json.Serialize(List()));
        return 0;
    }

    static List<PATH> Paths() {
        uint pathCount, modeCount;
        if(GetDisplayConfigBufferSizes(2,out pathCount,out modeCount)!=0) throw new Exception("display config size query failed");
        var paths=new PATH[pathCount]; var modes=new MODE[modeCount];
        int error=QueryDisplayConfig(2,ref pathCount,paths,ref modeCount,modes,IntPtr.Zero);
        if(error!=0) throw new Exception("QueryDisplayConfig failed: "+error);
        var result=new List<PATH>();
        for(int i=0;i<pathCount;i++) result.Add(paths[i]);
        return result;
    }
    static string SourceName(PATH path) {
        var name=new SOURCE_NAME{header=new HEADER{type=1,size=(uint)Marshal.SizeOf(typeof(SOURCE_NAME)),adapter=path.sourceAdapter,id=path.sourceId}};
        if(DisplayConfigGetDeviceInfo(ref name)!=0) throw new Exception("source name query failed");
        return name.device;
    }
    static int CurrentScale(PATH path,out int recommended) {
        var get=new DPI_GET{header=new HEADER{type=-3,size=(uint)Marshal.SizeOf(typeof(DPI_GET)),adapter=path.sourceAdapter,id=path.sourceId}};
        if(DisplayConfigGetDeviceInfo(ref get)!=0) throw new Exception("DPI scale query failed");
        recommended=Math.Abs(get.min);
        int index=Math.Max(0,Math.Min(Scales.Length-1,recommended+get.cur));
        return Scales[index];
    }

    public static MonitorRecord[] List() {
        var paths=Paths(); var result=new List<MonitorRecord>();
        EnumDisplayMonitors(IntPtr.Zero,IntPtr.Zero,(IntPtr monitor,IntPtr dc,ref RECT rect,IntPtr data)=>{
            var info=new MONITORINFOEX{size=(uint)Marshal.SizeOf(typeof(MONITORINFOEX))};
            GetMonitorInfoW(monitor,ref info);
            uint dx,dy; GetDpiForMonitor(monitor,0,out dx,out dy);
            var record=new MonitorRecord{device=info.device,primary=(info.flags&1)!=0,left=info.monitor.left,top=info.monitor.top,
                right=info.monitor.right,bottom=info.monitor.bottom,dpi=(int)dx};
            foreach(var path in paths) if(SourceName(path)==info.device) {
                int recommended; record.scale_percent=CurrentScale(path,out recommended);
                record.adapter=((long)path.sourceAdapter.high<<32)|path.sourceAdapter.low; record.source=path.sourceId;
            }
            result.Add(record); return true;
        },IntPtr.Zero);
        return result.ToArray();
    }

    public static void SetScale(string device,int percent) {
        foreach(var path in Paths()) {
            if(SourceName(path)!=device) continue;
            int recommended; CurrentScale(path,out recommended);
            int index=Array.IndexOf(Scales,percent);
            if(index<0) throw new ArgumentException("unsupported scale");
            var set=new DPI_SET{header=new HEADER{type=-4,size=(uint)Marshal.SizeOf(typeof(DPI_SET)),adapter=path.sourceAdapter,id=path.sourceId},scale=index-recommended};
            int error=DisplayConfigSetDeviceInfo(ref set);
            if(error!=0) throw new Exception("DPI scale change failed: "+error);
            return;
        }
        throw new ArgumentException("no active display "+device);
    }

    // Stage every requested mode, then apply them together so Windows never
    // sees an intermediate overlapping layout.
    public static void Arrange(string[] devices,int[] x,int[] y,int[] width,int[] height,string primary) {
        const uint DM_POSITION=0x20, DM_PELSWIDTH=0x80000, DM_PELSHEIGHT=0x100000;
        const uint CDS_UPDATEREGISTRY=1, CDS_SET_PRIMARY=0x10, CDS_NORESET=0x10000000;
        for(int i=0;i<devices.Length;i++) {
            var mode=new DEVMODE{size=(ushort)Marshal.SizeOf(typeof(DEVMODE))};
            if(!EnumDisplaySettingsW(devices[i],-1,ref mode)) throw new Exception("no current mode for "+devices[i]);
            mode.fields=DM_POSITION; mode.positionX=x[i]; mode.positionY=y[i];
            if(width[i]>0) { mode.fields|=DM_PELSWIDTH|DM_PELSHEIGHT; mode.pelsWidth=(uint)width[i]; mode.pelsHeight=(uint)height[i]; }
            uint flags=CDS_UPDATEREGISTRY|CDS_NORESET|(devices[i]==primary?CDS_SET_PRIMARY:0);
            int error=ChangeDisplaySettingsExW(devices[i],ref mode,IntPtr.Zero,flags,IntPtr.Zero);
            if(error!=0) throw new Exception("stage "+devices[i]+" failed: "+error);
        }
        int applied=ChangeDisplaySettingsExW(null,IntPtr.Zero,IntPtr.Zero,0,IntPtr.Zero);
        if(applied!=0) throw new Exception("apply layout failed: "+applied);
    }
}
