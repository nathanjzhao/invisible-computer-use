import gi, json, time, sys
gi.require_version("Gtk","3.0")
from gi.repository import Gtk, GLib
LOG=sys.argv[1] if len(sys.argv)>1 else "/tmp/fx.log"
X=int(sys.argv[2]) if len(sys.argv)>2 else 100
Y=int(sys.argv[3]) if len(sys.argv)>3 else 100
NAME=sys.argv[4] if len(sys.argv)>4 else "Fixture"
def log(d):
    d["t"]=time.time(); d["name"]=NAME
    open(LOG,"a").write(json.dumps(d)+"\n")
class W(Gtk.Window):
    def __init__(self):
        super().__init__(title=NAME); self.set_default_size(320,170)
        b=Gtk.Box(orientation=Gtk.Orientation.VERTICAL,spacing=10); b.set_margin_top(16); b.set_margin_start(16)
        self.btn=Gtk.Button(label="Press me"); self.btn.connect("clicked",lambda w: log({"hit":"buttonPressed"}))
        self.entry=Gtk.Entry(); self.entry.set_placeholder_text("type here")
        self.entry.connect("changed",lambda w: log({"hit":"textChanged","value":w.get_text()}))
        b.pack_start(self.btn,False,False,0); b.pack_start(self.entry,False,False,0); self.add(b)
        self.connect("notify::is-active",lambda w,p: log({"hit":"activeChanged","active":w.props.is_active}))
        self.connect("destroy",Gtk.main_quit)
    def go(self): self.show_all(); self.move(X,Y); log({"hit":"ready","pid":__import__("os").getpid()})
w=W(); w.go(); Gtk.main()
