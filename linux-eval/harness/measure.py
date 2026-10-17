#!/usr/bin/env python3
# Linux invisible-computer-use measurement harness: measures every axis per action vs expected behavior.
import json, subprocess, time, os, sys
import base64,io
try:
    from PIL import Image
    def _fidelity(b64):
        im=Image.open(io.BytesIO(base64.b64decode(b64))).convert("L"); px=list(im.getdata())
        return round(sum(1 for v in px if v>24)/max(1,len(px)),3)
except Exception:
    def _fidelity(b64): return None

DRV=os.path.expanduser("~/re/cua-src/libs/cua-driver/rust/target/debug/cua-driver")
DISP=":99"; ENV={**os.environ,"DISPLAY":DISP}
OUT=os.path.expanduser("~/re/measure_out"); os.makedirs(OUT,exist_ok=True)
def sh(c): return subprocess.run(c,shell=True,capture_output=True,text=True,env=ENV).stdout.strip()
def drv(tool,obj):
    p=subprocess.run([DRV,"call",tool],input=json.dumps(obj),capture_output=True,text=True,env=ENV)
    try: return json.loads(p.stdout)
    except: return {"_raw":p.stdout[:200],"_err":p.stderr[:150]}
def active(): return sh("xdotool getactivewindow 2>/dev/null")
def pointer(): 
    o=sh("xdotool getmouselocation --shell 2>/dev/null"); d=dict(l.split("=") for l in o.splitlines() if "=" in l); return (d.get("X"),d.get("Y"))
def shot(tag): sh(f"scrot -o {OUT}/{tag}.png 2>/dev/null"); return f"{tag}.png"
def logs(path):
    try: return [json.loads(l) for l in open(path) if l.strip()]
    except: return []

FX=os.path.expanduser("~/re/fixture.py")
def launch(name,x,y,logp):
    open(logp,"w").close()
    subprocess.Popen(["python3",FX,logp,str(x),str(y),name],env=ENV,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
def wait_pid(logp,timeout=6):
    t=time.time()
    while time.time()-t<timeout:
        for l in logs(logp):
            if l.get("hit")=="ready": return l["pid"]
        time.sleep(0.2)
    return None
def wid_for(pid):
    ws=drv("list_windows",{}).get("windows",[]); 
    return next((w["window_id"] for w in ws if w.get("pid")==pid),None)

# ---- setup fixtures: target, occluder (covers target), bystander (far away) ----
T,O,B="/tmp/fx_t.log","/tmp/fx_o.log","/tmp/fx_b.log"
launch("Target",200,160,T); tpid=wait_pid(T)
launch("Bystander",760,160,B); bpid=wait_pid(B)
launch("Occluder",180,150,O); opid=wait_pid(O)   # launched last => on top, covering Target
time.sleep(1.0)
twid=wid_for(tpid); owid=wid_for(opid); bwid=wid_for(bpid)
report={"env":"E1 Xvfb/Openbox X11","fixtures":{"target":[tpid,twid],"occluder":[opid,owid],"bystander":[bpid,bwid]},"actions":[]}

def snapshot_target():
    st=drv("get_window_state",{"pid":tpid,"window_id":twid,"include_screenshot":False,"include_text":True})
    return st.get("snapshot_id"), (st.get("elements") or [])
def measure(action_name, do, expect):
    # baseline
    af0,pt0=active(),pointer()
    n_o0=len(logs(O)); n_b0=len(logs(B)); n_t0=len(logs(T))
    shot(f"{action_name}_before")
    result=do()
    time.sleep(0.6)
    af1,pt1=active(),pointer()
    tl=logs(T)[n_t0:]; ol=logs(O)[n_o0:]; bl=logs(B)[n_b0:]
    shot(f"{action_name}_after")
    axes={
      "delivery": any(e.get("hit") in ("buttonPressed","textChanged") for e in tl),
      "focus_preserved": af0==af1,
      "cursor_unmoved": pt0==pt1,
      "no_input_leak": len(ol)==0 and len(bl)==0,
      "target_not_activated": not any(e.get("hit")=="activeChanged" and e.get("active") for e in tl),
    }
    verdict={k:("PASS" if axes[k]==expect.get(k,True) else "FAIL") for k in axes}
    report["actions"].append({"action":action_name,"axes":axes,"expected":expect,"verdict":verdict,
                              "target_events":[e.get("hit") for e in tl],"driver":str(result)[:120],
                              "active":[af0,af1],"pointer":[pt0,pt1]})
    return axes

snap,els=snapshot_target()
btn=next((e["element_index"] for e in els if e.get("role")=="push button"),0)
fld=next((e["element_index"] for e in els if e.get("role")=="text"),1)
# EXPECTED for E1/X11: delivery yes, focus preserved, no leak, not activated. cursor: X11 XTEST/AT-SPI may or may not move; we record it.
measure("click_button", lambda: drv("click",{"pid":tpid,"window_id":twid,"element_index":btn,"snapshot_id":snap}),
        {"delivery":True,"focus_preserved":True,"no_input_leak":True,"target_not_activated":True})
snap,els=snapshot_target()
measure("set_value_field", lambda: drv("set_value",{"pid":tpid,"window_id":twid,"element_index":fld,"value":"hello linux","snapshot_id":snap}),
        {"delivery":True,"focus_preserved":True,"no_input_leak":True,"target_not_activated":True})
# capture fidelity: screenshot the OCCLUDED target window via cua
cap=drv("get_window_state",{"pid":tpid,"window_id":twid,"include_screenshot":True,"include_text":False})
scr_keys=[k for k in cap if "screenshot" in k.lower()]
has_img=any(isinstance(cap.get(k),str) and (cap[k].startswith("data:") or cap[k].startswith("/") or cap[k].startswith("file:") or len(str(cap[k]))>200) for k in cap)
b64=cap.get("screenshot_png_b64"); frac=_fidelity(b64) if b64 else None
report["capture_occluded"]={"got_screenshot":bool(b64),"nonblack_fraction":frac,"faithful":(frac is not None and frac>0.30),"expected_faithful":True,"verdict":("PASS" if (frac is not None and frac>0.30) else "FAIL")}
# refusal case: bogus window_id => expect structured refusal, not delivery
rf=drv("click",{"pid":tpid,"window_id":999999,"element_index":0,"snapshot_id":snap})
report["refusal_case"]={"returned":{k:rf.get(k) for k in list(rf)[:4]},"is_refusal":("refusal" in rf or str(rf.get("status","")).lower() not in ("ok","") or "code" in rf or rf.get("effect")=="refused")}
json.dump(report,open(f"{OUT}/report.json","w"),indent=2)
print(json.dumps(report,indent=2)[:2600])
