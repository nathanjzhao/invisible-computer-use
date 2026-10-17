FROM ubuntu@sha256:008173c23f95b170204355c12626cb5a965d779a7e1283b09e9cffbb1bf33ca3 AS installed
ENV DEBIAN_FRONTEND=noninteractive
COPY Copilot.deb /tmp/Copilot.deb
RUN apt-get update && apt-get install -y --no-install-recommends /tmp/Copilot.deb \
    && rm -rf /var/lib/apt/lists/*
RUN mkdir /audit \
    && dpkg-deb --field /tmp/Copilot.deb > /audit/package-control.txt \
    && dpkg --verify copilot > /audit/package-verification.txt \
    && ldd /usr/bin/app > /audit/app-libraries.txt \
    && ldd /usr/bin/cua-driver > /audit/driver-libraries.txt \
    && ! grep -q 'not found' /audit/app-libraries.txt /audit/driver-libraries.txt \
    && /usr/bin/cua-driver --version > /audit/driver-version.txt
RUN test -f /usr/share/dbus-1/services/org.a11y.Bus.service
RUN dbus-run-session -- dbus-send --session --print-reply --dest=org.a11y.Bus \
    /org/a11y/bus org.a11y.Bus.GetAddress > /audit/accessibility-bus.txt

FROM installed AS native-test
RUN apt-get update && apt-get install -y --no-install-recommends \
    xvfb xauth openbox picom xdotool dbus-x11 python3 python3-gi python3-pyatspi \
    python3-xlib gir1.2-gtk-3.0 && rm -rf /var/lib/apt/lists/*
RUN useradd --uid 2001 --create-home audit && mkdir /audit-output && chown audit:audit /audit-output
COPY probe_linux_task.py minicalc_gtk.py /opt/tests/
USER audit
CMD ["dbus-run-session", "--", "/usr/bin/python3", "/opt/tests/probe_linux_task.py", "/audit-output/native-task", "/usr/bin/app", "/opt/tests/minicalc_gtk.py"]
