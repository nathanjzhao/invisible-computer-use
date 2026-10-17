# Linux drive for the cross-platform lineage patch AND the icu product drive (slice 12):
# Xvfb + Openbox + AT-SPI + a GTK fixture + a real GTK app (gnome-calculator).
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    xvfb openbox dbus-x11 at-spi2-core picom python3 python3-gi gir1.2-gtk-3.0 xdotool x11-xserver-utils xfce4-screensaver i3lock gnome-calculator ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /drive
COPY fixture.py drive.py icu-drive.py /drive/
ENV DISPLAY=:99 NO_AT_BRIDGE=0 GTK_A11Y=1
ENTRYPOINT ["python3", "/drive/drive.py"]
