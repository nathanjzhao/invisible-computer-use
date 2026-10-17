#!/usr/bin/env python3
import json, subprocess, time, os
DRV=os.path.expanduser("~/re/cua-src/libs/cua-driver/rust/target/debug/cua-driver")
DISP=":99"; ENV={**os.environ,"DISPLAY":DISP}; OUT=os.path.expanduser("~/re/measure_out"); os.makedirs(OUT,exist_ok=True)
FX=os.path.expanduser("~/re/fixture2.py")
def sh(c): return subprocess.run(c,shell=True,capture_output=True,text=True,env=ENV).stdout.strip()
def drv(t,o):
    p=subprocess.run([DRV,"call",t],input=json.dumps(o),capture_output=True,text=True,env=ENV)
    try: return json.loads(p.stdout)
    except: return {"_raw":p.stdout[:160]}
def active(): return sh("xdotool getactivewindow 2>/dev/null")
def ptr():
    d=dict(l.split("=") for l in sh("xdotool getmouselocation --shell 2>/dev/null").splitlines() if "=" in l); return (d.get("X"),d.get("Y"))
def logs(p):
    try: return [json.loads(l) for l in open(p) if l.strip()]
    except: return []
def launch(n,x,y,lp): open(lp,"w").close(); subprocess.Popen(["python3",FX,lp,str(x),str(y),n],env=ENV,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def wpid(lp,to=6):
    t=time.time()
    while time.time()-t<to:
        for l in logs(lp):
            if l.get("hit")=="ready": return l["pid"]
        time.sleep(0.2)
    return None
def wid(pid): ws=drv("list_windows",{}).get("windows",[]); return next((w["window_id"] for w in ws if w.get("pid")==pid),None)
T,B="/tmp/t.log","/tmp/b.log"
launch("Target",200,150,T); tp=wpid(T); launch("Bystander",780,150,B); bp=wpid(B); time.sleep(1.0)
tw=wid(tp); bw=wid(bp)
def snap():
    st=drv("get_window_state",{"pid":tp,"window_id":tw,"include_screenshot":False,"include_text":True}); return st.get("snapshot_id"),(st.get("elements") or [])
sn,els=snap()
btn=next((e["element_index"] for e in els if e.get("role")=="push button"),0)
fld=next((e["element_index"] for e in els if e.get("role")=="text"),1)
def measure(name, tool, args, pred, focus_first=False):
    global sn
    if focus_first:
        st=drv("get_window_state",{"pid":tp,"window_id":tw,"include_screenshot":False,"include_text":True})
        wb=st.get("window_bounds") or {}
        fe=next((e for e in (st.get("elements") or []) if e.get("role")=="text"),None)
        if fe and fe.get("frame") and wb:
            fr=fe["frame"]; cx=int(fr["x"]-wb.get("x",0)+fr["w"]/2); cy=int(fr["y"]-wb.get("y",0)+fr["h"]/2)
            drv("click",{"pid":tp,"window_id":tw,"x":cx,"y":cy,"delivery_mode":"foreground"}); time.sleep(0.4)
    def attempt(mode):
        n=len(logs(T)); nb=len(logs(B)); a0=active(); p0=ptr()
        a=dict(args); 
        if mode: a["delivery_mode"]=mode
        r=drv(tool,a); time.sleep(0.6)
        nw=logs(T)[n:]; nbl=logs(B)[nb:]; a1=active(); p1=ptr()
        return {"delivered":pred(nw),"focus_preserved":a0==a1,"cursor_unmoved":p0==p1,"no_input_leak":len(nbl)==0,
                "not_activated":not any(e.get("hit")=="activeChanged" and e.get("active") for e in nw),
                "code":r.get("code"),"route":(r.get("delivery") or {}).get("mode") or r.get("route")}
    bg=attempt(None)
    fg=attempt("foreground") if not bg["delivered"] else None
    return {"action":name,"background":bg,"foreground":fg,
            "final_delivers": bg["delivered"] or (fg and fg["delivered"]),
            "invisible_bg": bg["delivered"] and bg["focus_preserved"] and bg["not_activated"]}
R=[]
R.append(measure("click","click",{"pid":tp,"window_id":tw,"element_index":btn,"snapshot_id":sn}, lambda ev:any(e.get("hit")=="buttonPressed" for e in ev)))
sn,els=snap()
R.append(measure("set_value","set_value",{"pid":tp,"window_id":tw,"element_index":fld,"value":"hi","snapshot_id":sn}, lambda ev:any(e.get("hit")=="textChanged" for e in ev)))
R.append(measure("double_click","double_click",{"pid":tp,"window_id":tw,"x":150,"y":190}, lambda ev:sum(1 for e in ev if e.get("hit")=="pad_press")>=1 and any(e.get("kind")=="double" for e in ev if e.get("hit")=="pad_press")))
R.append(measure("right_click","right_click",{"pid":tp,"window_id":tw,"x":150,"y":190}, lambda ev:any(e.get("hit")=="pad_press" and e.get("button")==3 for e in ev)))
R.append(measure("scroll","scroll",{"pid":tp,"window_id":tw,"direction":"down","amount":3}, lambda ev:any(e.get("hit")=="scroll" for e in ev)))
R.append(measure("drag","drag",{"pid":tp,"window_id":tw,"from_x":50,"from_y":190,"to_x":260,"to_y":190}, lambda ev:any(e.get("hit")=="drag_motion" for e in ev)))
R.append(measure("type_text","type_text",{"pid":tp,"window_id":tw,"text":"xyz"}, lambda ev:any(e.get("hit") in ("textChanged","key") for e in ev), focus_first=True))
R.append(measure("press_key","press_key",{"pid":tp,"window_id":tw,"key":"b"}, lambda ev:any(e.get("hit")=="key" for e in ev) or any(e.get("hit")=="textChanged" for e in ev), focus_first=True))
R.append(measure("hotkey","hotkey",{"pid":tp,"window_id":tw,"keys":["ctrl","a"]}, lambda ev:any(e.get("hit")=="key" and e.get("ctrl") for e in ev), focus_first=True))
json.dump({"env":"E1 Xvfb/X11+picom","rows":R},open(f"{OUT}/parity.json","w"),indent=2)
print("%-14s %-9s %-9s %-9s %s"%("ACTION","bg_deliv","fg_deliv","invisible","note"))
for r in R:
    bg=r["background"]; fg=r["foreground"]
    print("%-14s %-9s %-9s %-9s %s"%(r["action"], bg["delivered"], (fg["delivered"] if fg else "-"), r["invisible_bg"], (bg["code"] or bg["route"] or "")))
print("\nSUMMARY: all deliver =", all(r["final_delivers"] for r in R), "| invisible-bg actions =", [r["action"] for r in R if r["invisible_bg"]])
