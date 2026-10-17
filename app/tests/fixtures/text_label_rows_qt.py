"""Qt5 twin of text_label_rows_gtk.py, for probe_linux_text_label_meaning.py: four buttons, a toggle,
a check box and two unnamed line edits whose rows can be rebound.
usage: text_label_rows_qt.py LOG CMD   (QT_ACCESSIBILITY=1 QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1)
Commands (JSON per line): {"op":"relabel","old":..,"new":..} | {"op":"rebind","texts":[..]}"""
import json, os, sys, time
from PyQt5 import QtCore, QtWidgets
LOG, CMD = sys.argv[1], sys.argv[2]
def log(**e):
    e.update(t=time.time(), pid=os.getpid())
    with open(LOG, 'a') as f: f.write(json.dumps(e) + '\n')
app = QtWidgets.QApplication(sys.argv[:1])
win = QtWidgets.QWidget(); win.setWindowTitle('Meaning edges'); win.resize(360, 520)
box = QtWidgets.QVBoxLayout(win)
buttons = []
def make(label, serial, cls=QtWidgets.QPushButton, checkable=False):
    b = cls(label); b.serial = serial
    if checkable: b.setCheckable(True)
    def clicked(*_):
        log(hit='clicked', serial=b.serial, label=b.text(), active=b.isChecked() if hasattr(b, 'isChecked') else None)
        if b.serial == 100: b.setText('Pause' if b.isChecked() else 'Play')
    b.clicked.connect(clicked); buttons.append(b); box.addWidget(b)
for i, name in enumerate(('Bravo', 'Delta', 'Echo', 'Foxtrot')): make(name, i + 1)
make('Play', 100, checkable=True)
make('Mute', 101, cls=QtWidgets.QCheckBox)
entries = []
for i, text in enumerate(('alice', 'bob')):
    e = QtWidgets.QLineEdit(text); e.setCursorPosition(0); e.serial = 200 + i; e.record = text
    e.textChanged.connect(lambda t, e=e: log(hit='entry_changed', serial=e.serial, record=e.record, text=t))
    entries.append(e); box.addWidget(e)
def apply(cmd):
    if cmd['op'] == 'relabel':
        next(b for b in buttons if b.text() == cmd['old']).setText(cmd['new'])
    elif cmd['op'] == 'rebind':
        for e, text in zip(entries, cmd['texts']):
            e.record = text; e.setText(text); e.setCursorPosition(0)  # GTK inserts at 0; so does this
    log(hit='applied', cmd=cmd, buttons=[(b.text(), b.serial) for b in buttons], entries=[(e.serial, e.record, e.text()) for e in entries])
def poll():
    if os.path.exists(CMD):
        lines = [l for l in open(CMD) if l.strip()]; os.remove(CMD)
        for l in lines:
            try: apply(json.loads(l))
            except Exception as err: log(hit='cmd_error', cmd=l, error=repr(err))
t = QtCore.QTimer(); t.timeout.connect(poll); t.start(50)
win.show()
QtCore.QTimer.singleShot(800, lambda: log(hit='ready'))
sys.exit(app.exec_())
