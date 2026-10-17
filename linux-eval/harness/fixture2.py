import gi, json, time, sys, os
gi.require_version("Gtk","3.0")
from gi.repository import Gtk, Gdk
LOG=sys.argv[1]; X=int(sys.argv[2]); Y=int(sys.argv[3]); NAME=sys.argv[4] if len(sys.argv)>4 else "Fixture"
def log(d):
    d["t"]=time.time(); d["name"]=NAME
    open(LOG,"a").write(json.dumps(d)+"\n")
KIND={Gdk.EventType.BUTTON_PRESS:"single",getattr(Gdk.EventType,"DOUBLE_BUTTON_PRESS","2"):"double",getattr(Gdk.EventType,"TRIPLE_BUTTON_PRESS","3"):"triple"}
class W(Gtk.Window):
    def __init__(self):
        super().__init__(title=NAME); self.set_default_size(340,240)
        v=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=8); v.set_margin_top(12); v.set_margin_start(12); v.set_margin_end(12)
        self.btn=Gtk.Button(label="Press me"); self.btn.connect("clicked",lambda w: log({"hit":"buttonPressed"}))
        self.entry=Gtk.Entry(); self.entry.set_placeholder_text("type here")
        self.entry.connect("changed",lambda w: log({"hit":"textChanged","value":w.get_text()}))
        self.entry.connect("key-press-event",lambda w,e: log({"hit":"key","name":Gdk.keyval_name(e.keyval),"ctrl":bool(e.state & Gdk.ModifierType.CONTROL_MASK),"shift":bool(e.state & Gdk.ModifierType.SHIFT_MASK)}))
        # interaction pad for coordinate actions (scroll/drag/right/double click)
        self.pad=Gtk.EventBox(); lbl=Gtk.Label(label="pad"); self.pad.add(lbl)
        self.pad.set_size_request(300,90)
        self.pad.add_events(Gdk.EventMask.BUTTON_PRESS_MASK|Gdk.EventMask.SCROLL_MASK|Gdk.EventMask.POINTER_MOTION_MASK|Gdk.EventMask.BUTTON_MOTION_MASK)
        self.pad.connect("button-press-event",lambda w,e: log({"hit":"pad_press","button":e.button,"kind":KIND.get(e.type,str(e.type))}))
        self.pad.connect("scroll-event",lambda w,e: log({"hit":"scroll","dir":Gdk.keyval_name(0) or str(e.direction.value_nick if hasattr(e.direction,"value_nick") else e.direction)}))
        self._drag=0
        def motion(w,e):
            if e.state & Gdk.ModifierType.BUTTON1_MASK: self._drag+=1; 
            if self._drag in (1,) or (e.state & Gdk.ModifierType.BUTTON1_MASK and self._drag%10==0): log({"hit":"drag_motion","n":self._drag})
        self.pad.connect("motion-notify-event",motion)
        v.pack_start(self.btn,False,False,0); v.pack_start(self.entry,False,False,0); v.pack_start(self.pad,True,True,0); self.add(v)
        self.connect("notify::is-active",lambda w,p: log({"hit":"activeChanged","active":w.props.is_active}))
        self.connect("destroy",Gtk.main_quit)
    def go(self): self.show_all(); self.move(X,Y); log({"hit":"ready","pid":os.getpid()})
w=W(); w.go(); Gtk.main()
