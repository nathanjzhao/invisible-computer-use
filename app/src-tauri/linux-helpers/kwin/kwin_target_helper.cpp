// SPDX-License-Identifier: MIT
// Cua-owned KWin effect. The Rust driver verifies that this D-Bus name is
// owned by the running kwin_wayland process before it trusts any response.
// Identity protocol v1 remains compatible with History. Capture protocol v2
// adds window-owned offscreen pixels. History input v1 adds scoped observation only.

#include <kwin/effect/effect.h>
#include <kwin/window.h>
#include <kwin/effect/effecthandler.h>
#include <kwin/core/rendertarget.h>
#include <kwin/core/renderviewport.h>
#include <kwin/opengl/gltexture.h>
#include <kwin/opengl/glframebuffer.h>
#if __has_include(<kwin/opengl/eglcontext.h>)
#include <kwin/opengl/eglcontext.h>
#else
#include <kwin/opengl/openglcontext.h>
#endif
#include <QBuffer>
#include <QCoreApplication>
#include <QDBusContext>
#include <QDBusMessage>
#include <QPointer>
#include <QTimer>
#include <optional>
#include <kwin/workspace.h>
#include <kwin/input.h>
#include <kwin/input_event.h>
#include <kwin/input_event_spy.h>
#include <kwin/pointer_input.h>
#include <kwin/wayland_server.h>
#include <kwin/wayland/seat.h>
#include <QDBusServiceWatcher>
#include <KConfigGroup>
#include <KSharedConfig>
#include <time.h>

#include <QDBusConnection>
#include <QHash>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSet>
#include <QtMath>
#include <QUuid>

namespace
{
constexpr auto kService = "org.cua.KWinTarget";
constexpr auto kPath = "/org/cua/KWinTarget";
constexpr quint32 kProtocolVersion = 1;

class TargetApi : public QObject, protected QDBusContext, public KWin::InputEventSpy
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.cua.KWinTarget")

public:
    TargetApi() {
        m_historyTimer.setInterval(50);
        connect(&m_historyTimer, &QTimer::timeout, this, [this] {
            if (!historyValid()) stopHistory();
        });
        m_historyOwner.setConnection(QDBusConnection::sessionBus());
        m_historyOwner.setWatchMode(QDBusServiceWatcher::WatchForUnregistration);
        connect(&m_historyOwner, &QDBusServiceWatcher::serviceUnregistered, this, [this] { stopHistory(); });
    }
    ~TargetApi() override { stopHistory(); }
    bool pending() const { return m_pending.has_value(); }
    void renderPending()
    {
        if (!m_pending) return;
        const auto request = *m_pending;
        m_pending.reset();
        const auto fail = [&request](const QString &reason) {
            request.bus.send(request.message.createErrorReply("org.cua.KWinTarget.CaptureFailed", reason));
        };
        auto window = request.window.data();
        if (!window || window->isDeleted() || window->isMinimized() ||
            window->pid() != request.pid || window->internalId() != request.uuid ||
            frame(window) != request.frame) {
            fail("window_capture_stale_target"); return;
        }
        if (!KWin::effects->isOpenGLCompositing() || !KWin::effects->makeOpenGLContextCurrent()) {
            fail("window_capture_unsupported_renderer"); return;
        }
        const QSize size(request.frame.width(), request.frame.height());
        auto texture = KWin::GLTexture::allocate(GL_RGBA8, size);
        if (!texture) { fail("window_capture_no_buffer"); return; }
        KWin::GLFramebuffer target(texture.get());
        if (!target.valid()) { fail("window_capture_no_framebuffer"); return; }
        KWin::RenderTarget renderTarget(&target);
#if CUA_KWIN_CAPTURE_REGION
        KWin::RenderViewport viewport(window->frameGeometry(), 1, renderTarget, QPoint());
#else
        KWin::RenderViewport viewport(window->frameGeometry(), 1, renderTarget);
#endif
        KWin::GLFramebuffer::pushFramebuffer(&target);
        glClearColor(0, 0, 0, 0);
        glClear(GL_COLOR_BUFFER_BIT);
        KWin::WindowPaintData paint;
        // Only this EffectWindow is drawn, onto a newly cleared private target.
        // No output readback, cursor, other windows, or notification layers.
        KWin::effects->drawWindow(renderTarget, viewport, window->effectWindow(),
            KWin::Effect::PAINT_WINDOW_TRANSFORMED | KWin::Effect::PAINT_WINDOW_TRANSLUCENT,
#if CUA_KWIN_CAPTURE_REGION
            KWin::Region::infinite(),
#else
            KWin::infiniteRegion(),
#endif
            paint);
        QImage image(size, QImage::Format_RGBA8888_Premultiplied);
        KWin::effects->openglContext()->glReadnPixels(0, 0, size.width(), size.height(),
            GL_RGBA, GL_UNSIGNED_BYTE, image.sizeInBytes(), image.bits());
        KWin::GLFramebuffer::popFramebuffer();
        image = image.mirrored(false, true);
        if (window->isDeleted() || window->pid() != request.pid ||
            frame(window) != request.frame || window->internalId() != request.uuid) {
            fail("window_capture_stale_target"); return;
        }
        QByteArray png;
        QBuffer output(&png);
        output.open(QIODevice::WriteOnly);
        if (!image.save(&output, "PNG")) { fail("window_capture_encode_failed"); return; }
        const QJsonObject result{{"version", 2}, {"id", qint64(request.token)},
            {"pid", qint64(request.pid)}, {"x", request.frame.x()}, {"y", request.frame.y()},
            {"w", size.width()}, {"h", size.height()}, {"png", QString::fromLatin1(png.toBase64())}};
        request.bus.send(request.message.createReply(QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact))));
    }

public slots:
    Q_SCRIPTABLE quint32 GetHistorySetupVersion() { return 1; }
    Q_SCRIPTABLE bool RememberHistoryApproval() {
        if (!calledFromDBus()) return false;
        // Use the running desktop's actual config location. The host can have
        // an isolated XDG_CONFIG_HOME and must not persist into that profile.
        auto config = KSharedConfig::openConfig(QStringLiteral("kwinrc"));
        KConfigGroup plugins(config, QStringLiteral("Plugins"));
        plugins.writeEntry(QStringLiteral("cua_kwin_target_helperEnabled"), true);
        return config->sync();
    }
    Q_SCRIPTABLE quint32 GetHistoryInputVersion() { return 1; }
    Q_SCRIPTABLE bool BeginHistoryInput(qulonglong token, quint32 pid) {
        if (!calledFromDBus()) return false;
        const auto caller = message().service();
        if (historyValid() && caller != m_historyCaller) return false;
        stopHistory();
        auto window = KWin::Workspace::self()->activeWindow();
        if (!window || m_tokens.value(window->internalId()) != token || !token || window->pid() != pid) return false;
        m_historyWindow = window; m_historyId = window->internalId(); m_historyPid = pid;
        m_historyFrame = frame(window); m_historyClient = client(window);
        m_historyCaller = caller; m_historyUntil = monotonicUs() + 1000000;
        if (!historyValid()) { stopHistory(); return false; }
        KWin::input()->installInputEventSpy(this); m_historyWatching = true;
        m_historyOwner.addWatchedService(caller); m_historyTimer.start();
        return true;
    }
    Q_SCRIPTABLE bool CheckHistoryInput() {
        return calledFromDBus() && message().service() == m_historyCaller && historyValid();
    }
    Q_SCRIPTABLE QString DrainHistoryInput() {
        if (!calledFromDBus() || message().service() != m_historyCaller || !historyValid())
            return QStringLiteral("{\"valid\":false,\"lost\":true,\"events\":[]}");
        const QJsonObject result{{"valid", true}, {"lost", m_historyLost}, {"events", m_historyEvents}};
        m_historyEvents = {}; m_historyLost = false; m_historyUntil = monotonicUs() + 1000000;
        return QString::fromUtf8(QJsonDocument(result).toJson(QJsonDocument::Compact));
    }
    Q_SCRIPTABLE void EndHistoryInput() {
        if (calledFromDBus() && message().service() == m_historyCaller) stopHistory();
    }
    Q_SCRIPTABLE quint32 GetCaptureVersion() { return 2; }

    Q_SCRIPTABLE QString CaptureWindow(qulonglong token, quint32 pid)
    {
        if (!calledFromDBus()) return {};
        if (m_pending) { sendErrorReply("org.cua.KWinTarget.CaptureFailed", "window_capture_busy"); return {}; }
        for (auto window : KWin::Workspace::self()->windows()) {
            if (!window || !window->isClient() || window->isDeleted() || window->isMinimized() ||
                window->pid() != pid || m_tokens.value(window->internalId()) != token || token == 0)
                continue;
            const auto rect = frame(window);
            if (rect.width() <= 0 || rect.height() <= 0 || qint64(rect.width()) * rect.height() > 32 * 1024 * 1024)
                break;
            setDelayedReply(true);
            m_pending = Request{window, window->internalId(), token, pid, rect, message(), connection()};
            KWin::effects->addRepaintFull();
            const auto sequence = ++m_requestSequence;
            QTimer::singleShot(2500, this, [this, sequence] {
                if (m_pending && m_requestSequence == sequence) {
                    auto request = *m_pending; m_pending.reset();
                    request.bus.send(request.message.createErrorReply("org.cua.KWinTarget.CaptureFailed", "window_capture_timeout"));
                }
            });
            return {};
        }
        sendErrorReply("org.cua.KWinTarget.CaptureFailed", "window_capture_stale_target");
        return {};
    }

    Q_SCRIPTABLE quint32 GetVersion()
    {
        return kProtocolVersion;
    }

    Q_SCRIPTABLE QString GetWindows()
    {
        const auto liveWindows = KWin::Workspace::self()->windows();
        QSet<QUuid> liveIds;
        for (KWin::Window *window : liveWindows) {
            if (window && window->isClient() && !window->isDeleted()) {
                liveIds.insert(window->internalId());
            }
        }
        for (auto it = m_tokens.begin(); it != m_tokens.end();) {
            if (!liveIds.contains(it.key())) {
                it = m_tokens.erase(it);
            } else {
                ++it;
            }
        }

        QJsonArray windows;
        for (KWin::Window *window : liveWindows) {
            if (!window || !window->isClient() || window->isDeleted()) {
                continue;
            }
            const auto frame = window->frameGeometry();
            QJsonObject record;
            record["token"] = static_cast<qint64>(tokenFor(window->internalId()));
            record["pid"] = window->pid();
            record["title"] = window->captionNormal();
            record["app_id"] = window->resourceClass();
            record["x"] = qRound(frame.x());
            record["y"] = qRound(frame.y());
            record["w"] = qRound(frame.width());
            record["h"] = qRound(frame.height());
            record["active"] = window->isActive();
            record["minimized"] = window->isMinimized();
            record["stacking"] = window->stackingOrder();
            windows.push_back(record);
        }
        return QString::fromUtf8(QJsonDocument(windows).toJson(QJsonDocument::Compact));
    }

private:
    static quint64 monotonicUs() {
        timespec ts{}; if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
        return quint64(ts.tv_sec) * 1000000 + ts.tv_nsec / 1000;
    }
    QRect m_historyFrame;
    QRect m_historyClient;
    bool historyValid() const {
        auto w = m_historyWindow.data();
        return !m_historyCaller.isEmpty() && monotonicUs() < m_historyUntil && w &&
            !w->isDeleted() && !w->isMinimized() && w->isClient() && w->isActive() &&
            w->inherits("KWin::WaylandWindow") && w->surface() &&
            w->pid() == m_historyPid && w->internalId() == m_historyId && frame(w) == m_historyFrame && client(w) == m_historyClient &&
            KWin::Workspace::self()->activeWindow() == w &&
            !KWin::effects->isScreenLocked() && !KWin::effects->activeFullScreenEffect() && !KWin::effects->hasKeyboardGrab() &&
            !KWin::effects->isMouseInterception() && !KWin::input()->isSelectingWindow();
    }
    void stopHistory() {
        if (m_historyWatching) KWin::input()->uninstallInputEventSpy(this);
        m_historyWatching = false; m_historyTimer.stop();
        m_historyOwner.setWatchedServices({}); m_historyCaller.clear();
        m_historyWindow.clear(); m_historyEvents = {}; m_historyLost = false;
    }
    void appendHistory(QJsonObject item, std::chrono::microseconds timestamp) {
        if (m_historyEvents.size() >= 32) { m_historyEvents = {}; m_historyLost = true; return; }
        const auto rect = m_historyClient;
        item["bounds"] = QJsonArray{rect.x(), rect.y(), rect.width(), rect.height()};
        // Preserve the native event clock, including delay before this callback.
        item["at"] = qint64(timestamp.count()); m_historyEvents.push_back(item);
    }
    void pointerButton(KWin::PointerButtonEvent *event) override {
        if (!historyValid() || KWin::input()->pointer()->focus() != m_historyWindow ||
            event->state != KWin::PointerButtonState::Pressed ||
            (event->button != Qt::LeftButton && event->button != Qt::RightButton) ||
            !QRectF(m_historyClient).contains(event->position)) return;
        appendHistory({{"kind", "click"}, {"right", event->button == Qt::RightButton},
            {"x", event->position.x()}, {"y", event->position.y()}}, event->timestamp);
    }
    void keyboardKey(KWin::KeyboardKeyEvent *event) override {
        if (!historyValid() || event->state != KWin::KeyboardKeyState::Pressed ||
            !m_historyWindow->surface() || KWin::waylandServer()->seat()->focusedKeyboardSurface() != m_historyWindow->surface()) return;
        QString key;
        switch (event->key) {
        case Qt::Key_Return: case Qt::Key_Enter: key = "return"; break;
        case Qt::Key_Tab: case Qt::Key_Backtab: key = "tab"; break;
        case Qt::Key_Escape: key = "escape"; break;
        default: return;
        }
        const auto mods = event->modifiers;
        // Never access event->text or serialize a printable key/scancode.
        appendHistory({{"kind", "key"}, {"key", key}, {"ctrl", bool(mods & Qt::ControlModifier)},
            {"alt", bool(mods & Qt::AltModifier)}, {"shift", bool(mods & Qt::ShiftModifier)},
            {"win", bool(mods & Qt::MetaModifier)}}, event->timestamp);
    }
    QPointer<KWin::Window> m_historyWindow;
    QUuid m_historyId;
    quint32 m_historyPid = 0;
    QString m_historyCaller;
    quint64 m_historyUntil = 0;
    bool m_historyWatching = false;
    bool m_historyLost = false;
    QJsonArray m_historyEvents;
    QTimer m_historyTimer;
    QDBusServiceWatcher m_historyOwner;

    static QRect frame(KWin::Window *w) {
        const auto r = w->frameGeometry();
        return QRect(qRound(r.x()), qRound(r.y()), qRound(r.width()), qRound(r.height()));
    }
    static QRect client(KWin::Window *w) {
        const auto r = w->clientGeometry();
        return QRect(qRound(r.x()), qRound(r.y()), qRound(r.width()), qRound(r.height()));
    }
    struct Request {
        QPointer<KWin::Window> window;
        QUuid uuid;
        quint64 token;
        quint32 pid;
        QRect frame;
        QDBusMessage message;
        QDBusConnection bus;
    };
    std::optional<Request> m_pending;
    quint64 m_requestSequence = 0;
    quint64 tokenFor(const QUuid &id)
    {
        if (const auto token = m_tokens.constFind(id); token != m_tokens.cend()) {
            return *token;
        }
        // Keep the counter on KWin itself so effect unload/reload cannot reuse
        // an old token while the compositor's unique D-Bus owner stays alive.
        auto app = QCoreApplication::instance();
        const quint64 token = app->property("cuaWindowNextToken").toULongLong() + 1;
        if (token >= 0xffffffffULL) return 0;
        app->setProperty("cuaWindowNextToken", QVariant::fromValue(token));
        m_tokens.insert(id, token);
        return token;
    }

    QHash<QUuid, quint64> m_tokens;
};

class KwinTargetHelperEffect : public KWin::Effect
{
public:
    KwinTargetHelperEffect()
    {
        auto bus = QDBusConnection::sessionBus();
        if (!bus.registerService(QString::fromLatin1(kService))) {
            return;
        }
        m_exported = bus.registerObject(
            QString::fromLatin1(kPath),
            &m_api,
            QDBusConnection::ExportScriptableSlots);
        if (!m_exported) {
            bus.unregisterService(QString::fromLatin1(kService));
        }
    }

    bool isActive() const override { return m_api.pending(); }
    void postPaintScreen() override
    {
        KWin::effects->postPaintScreen();
        m_api.renderPending();
    }

    ~KwinTargetHelperEffect() override
    {
        if (!m_exported) {
            return;
        }
        auto bus = QDBusConnection::sessionBus();
        bus.unregisterObject(QString::fromLatin1(kPath));
        bus.unregisterService(QString::fromLatin1(kService));
    }

private:
    TargetApi m_api;
    bool m_exported = false;
};
}

KWIN_EFFECT_FACTORY(KwinTargetHelperEffect, "kwin_target_helper.json")

#include "kwin_target_helper.moc"
