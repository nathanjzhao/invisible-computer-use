using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

// Create one root-enumerated device node and bind the signed INF to it. This
// is what devcon.exe install does; Server 2022's pnputil lacks /add-device.
public static class DeviceNode {
    [StructLayout(LayoutKind.Sequential)] struct SP_DEVINFO_DATA { public uint cbSize; public Guid ClassGuid; public uint DevInst; public IntPtr Reserved; }
    [DllImport("setupapi.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool SetupDiGetINFClassW(string inf,out Guid guid,char[] name,uint size,out uint required);
    [DllImport("setupapi.dll",SetLastError=true)] static extern IntPtr SetupDiCreateDeviceInfoList(ref Guid guid,IntPtr parent);
    [DllImport("setupapi.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool SetupDiCreateDeviceInfoW(IntPtr set,string name,ref Guid guid,string description,IntPtr parent,uint flags,ref SP_DEVINFO_DATA data);
    [DllImport("setupapi.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool SetupDiSetDeviceRegistryPropertyW(IntPtr set,ref SP_DEVINFO_DATA data,uint property,byte[] buffer,uint size);
    [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiCallClassInstaller(uint function,IntPtr set,ref SP_DEVINFO_DATA data);
    [DllImport("setupapi.dll",SetLastError=true)] static extern bool SetupDiDestroyDeviceInfoList(IntPtr set);
    [DllImport("newdev.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool UpdateDriverForPlugAndPlayDevicesW(IntPtr parent,string hardwareId,string inf,uint flags,out bool reboot);
    const uint DICD_GENERATE_ID=1, SPDRP_HARDWAREID=1, DIF_REGISTERDEVICE=0x19, INSTALLFLAG_FORCE=1;

    public static bool Install(string inf,string hardwareId) {
        Guid guid; uint required; char[] name=new char[256];
        if(!SetupDiGetINFClassW(inf,out guid,name,(uint)name.Length,out required)) throw new Win32Exception();
        IntPtr set=SetupDiCreateDeviceInfoList(ref guid,IntPtr.Zero);
        if(set==new IntPtr(-1)) throw new Win32Exception();
        try {
            var data=new SP_DEVINFO_DATA{cbSize=(uint)Marshal.SizeOf(typeof(SP_DEVINFO_DATA))};
            string className=new string(name,0,Array.IndexOf(name,'\0'));
            if(!SetupDiCreateDeviceInfoW(set,className,ref guid,null,IntPtr.Zero,DICD_GENERATE_ID,ref data)) throw new Win32Exception();
            byte[] ids=System.Text.Encoding.Unicode.GetBytes(hardwareId+"\0\0");
            if(!SetupDiSetDeviceRegistryPropertyW(set,ref data,SPDRP_HARDWAREID,ids,(uint)ids.Length)) throw new Win32Exception();
            if(!SetupDiCallClassInstaller(DIF_REGISTERDEVICE,set,ref data)) throw new Win32Exception();
        } finally { SetupDiDestroyDeviceInfoList(set); }
        bool reboot;
        if(!UpdateDriverForPlugAndPlayDevicesW(IntPtr.Zero,hardwareId,inf,INSTALLFLAG_FORCE,out reboot)) throw new Win32Exception();
        return reboot;
    }
}
