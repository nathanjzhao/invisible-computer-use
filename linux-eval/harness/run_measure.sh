export DISPLAY=:99 NO_AT_BRIDGE=0 GTK_A11Y=1
pkill Xvfb 2>/dev/null || true; sleep 1
Xvfb :99 -screen 0 1440x900x24 >/tmp/xvfb.log 2>&1 & sleep 2
openbox >/tmp/ob.log 2>&1 & sleep 1
eval "$(dbus-launch --sh-syntax)"
picom --backend xrender >/tmp/picom.log 2>&1 & sleep 1
/usr/libexec/at-spi-bus-launcher --launch-immediately >/tmp/atspi.log 2>&1 & sleep 1
DRV=~/re/cua-src/libs/cua-driver/rust/target/debug/cua-driver
$DRV stop 2>/dev/null || true; ($DRV serve >/tmp/drv.log 2>&1 &); sleep 4
python3 ~/re/measure.py
echo DONE_MEASURE
