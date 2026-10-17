"""Qt5 receiver for meaning-bound stale targets. Same controls and commands as
stale_meaning_gtk.py: Alpha, Bravo, Charlie, a pool of five "Item NN" row
buttons rebound on scroll, and one line edit.

usage: stale_meaning_qt.py LOG CMD   (run with QT_ACCESSIBILITY=1)
"""
import json
import os
import sys
import time

from PyQt5 import QtCore, QtWidgets

LOG, CMD = sys.argv[1], sys.argv[2]
app = QtWidgets.QApplication(sys.argv)
serial = [0]


def log(**event):
    event.update(t=time.time(), pid=os.getpid())
    with open(LOG, 'a') as f:
        f.write(json.dumps(event) + '\n')


win = QtWidgets.QWidget()
win.setWindowTitle('StaleMeaning Qt5')
win.resize(320, 420)
win.move(420, 80)
layout = QtWidgets.QVBoxLayout(win)
buttons = []


def make(label):
    serial[0] += 1
    button = QtWidgets.QPushButton(label)
    button.serial = serial[0]
    button.clicked.connect(lambda _=False, b=button: log(hit='clicked', serial=b.serial, label=b.text()))
    buttons.append(button)
    layout.addWidget(button)
    return button


for name in ('Alpha', 'Bravo', 'Charlie'):
    make(name)
first_item = [0]
rows = [make(f'Item {i:02d}') for i in range(5)]
line = QtWidgets.QLineEdit()
line.textChanged.connect(lambda text: log(hit='entry_changed', text=text))
layout.addWidget(line)


def by_label(label):
    return next(b for b in buttons if b.text() == label)


def apply(command):
    op, *args = command.split(' ', 1)
    if op == 'relabel':
        old, new = args[0].split()
        by_label(old).setText(new)
    elif op == 'scroll':
        first_item[0] += int(args[0])
        for offset, row in enumerate(rows):
            row.setText(f'Item {first_item[0] + offset:02d}')
    elif op == 'settext':
        line.setText(args[0] if args else '')
    else:
        raise ValueError(op)
    log(hit='applied', cmd=command, buttons=[(b.text(), b.serial) for b in buttons])


def poll():
    if os.path.exists(CMD):
        lines = [entry.strip() for entry in open(CMD) if entry.strip()]
        os.remove(CMD)
        for entry in lines:
            try:
                apply(entry)
            except Exception as error:  # noqa: BLE001
                log(hit='cmd_error', cmd=entry, error=repr(error))


timer = QtCore.QTimer()
timer.timeout.connect(poll)
timer.start(50)
win.show()
QtCore.QTimer.singleShot(800, lambda: log(hit='ready'))
app.exec_()
