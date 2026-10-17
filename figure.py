import matplotlib; matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch
import matplotlib.image as mpimg

BG="#0f1115"; CARD="#171a21"; GRN="#39d353"; RED="#f85149"; TXT="#e6edf3"; SUB="#8b949e"; ACC="#339cff"
fig=plt.figure(figsize=(12,8.6),dpi=170); fig.patch.set_facecolor(BG)
fig.text(0.055,0.945,"Invisible computer use — verified primitives on macOS 26.4",fontsize=19,color=TXT,weight="bold")
fig.text(0.055,0.912,"Synthetic input into an occluded, unfocused, background window. Measured on this machine, not asserted.",fontsize=11,color=SUB)

rows=[
 ("AX click on occluded window","AXUIElementPerformAction(AXPress) on the button","buttonPressed fired · activation events = 0",True),
 ("AX set value on occluded field","AXUIElementSetAttributeValue(AXValue/AXFocused)","field updated · app never became key",True),
 ("Type into background window","CGEventPostToPid keyboard events → first responder","all 18 chars landed · hasKeyWindow = false",True),
 ("No focus theft","NSWorkspace.frontmostApplication before vs after","Warp(723) → Warp(723) · target never activated",True),
 ("Real cursor not moved","CGEvent.location before vs after","988,778 → 988,778 · exact",True),
 ("Faithful capture while fully covered","screencapture -l <windowID> / SCContentFilter","center px 78,140,221 = target blue, not occluder red",True),
 ("Occluder window untouched","event log of the covering window","zero mouse events reached it",True),
 ("Raw mouse posted to pid clicks control","CGEventPostToPid mouse + window fields 91/92","delivered but NOT hit-tested → button never fired",False),
]
x0,x1=0.055,0.945; top=0.86; rh=0.083
for i,(name,how,res,ok) in enumerate(rows):
    y=top-i*rh
    ax=fig.add_axes([x0,y-rh+0.012,x1-x0,rh-0.016]); ax.set_facecolor(CARD); ax.set_xticks([]); ax.set_yticks([])
    for s in ax.spines.values(): s.set_visible(False)
    ax.add_patch(FancyBboxPatch((0,0),1,1,boxstyle="round,pad=0.002,rounding_size=0.03",transform=ax.transAxes,facecolor=CARD,edgecolor="#232833",lw=1))
    c=GRN if ok else RED
    ax.text(0.014,0.5,"●",color=c,fontsize=15,va="center")
    ax.text(0.045,0.66,name,color=TXT,fontsize=12.5,weight="bold",va="center")
    ax.text(0.045,0.28,how,color=SUB,fontsize=9.5,va="center",family="monospace")
    ax.text(0.985,0.5,res,color=(TXT if ok else "#ffb3ae"),fontsize=9.8,va="center",ha="right",family="monospace")
    ax.text(0.86,0.5,("PASS" if ok else "EXPECTED FAIL"),color=c,fontsize=10,va="center",ha="right",weight="bold")
fig.text(0.055,0.045,"Clicks go through Accessibility, not synthetic mouse — the posted-mouse row is why. Keys and capture bypass focus. Cursor overlay is a separate click-through window clipped to the visible region.",fontsize=9.5,color=ACC,wrap=True)
fig.savefig("results_scoreboard.png",facecolor=BG,bbox_inches="tight",pad_inches=0.25)
print("wrote results_scoreboard.png")
