// SPDX-License-Identifier: GPL-2.0-or-later
// Version-bound adapter. Geometry/selection policy stays in the Rust host.
#include <plugin.h>
#include <input.h>
#include <input_event.h>
#include <window.h>
#include <workspace.h>
#include <outline.h>
#include <wayland_server.h>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusContext>
#include <QDBusServiceWatcher>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPointer>
#include <QTimer>
#include <QUuid>
#include <cmath>

using namespace KWin;
static QString json(const QJsonObject &v) { return QString::fromUtf8(QJsonDocument(v).toJson(QJsonDocument::Compact)); }
static QJsonArray rect(const QRectF &r) { return {r.x(), r.y(), r.width(), r.height()}; }

class Bridge : public Plugin, public InputEventFilter, protected QDBusContext
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.waytoys.ZonesBridge1")
public:
    Bridge() : InputEventFilter(InputFilterOrder::Effects), watcher(this)
    {
        auto bus = QDBusConnection::sessionBus();
        bus.registerObject(QStringLiteral("/Waytoys/Zones"), this, QDBusConnection::ExportAllSlots | QDBusConnection::ExportAllSignals);
        watcher.setConnection(bus);
        watcher.setWatchMode(QDBusServiceWatcher::WatchForUnregistration);
        connect(&watcher, &QDBusServiceWatcher::serviceUnregistered, this, [this] { detach(); });
        connect(waylandServer(), &WaylandServer::lockStateChanged, this, [this] {
            if (waylandServer()->isScreenLocked()) detach();
        });
        connect(workspace(), &Workspace::windowAdded, this, &Bridge::track);
        for (auto *w : workspace()->windows()) track(w);
        connect(workspace(), &Workspace::outputsChanged, this, [this] {
            cancel("outputs-changed"); emit Event(json({{"kind", "outputs"}, {"epoch", epoch}}));
        });
        timeout.setSingleShot(true);
        timeout.setInterval(150);
        connect(&timeout, &QTimer::timeout, this, [this] { cancel("target-timeout"); });
        lease.setInterval(1000);
        connect(&lease, &QTimer::timeout, this, [this] {
            if (++missed >= 4 || (waylandServer() && waylandServer()->isScreenLocked())) detach();
        });
        lease.start();
        qInfo("Waytoys bridge loaded (protocol 1)");
    }
    ~Bridge() override { detach(); QDBusConnection::sessionBus().unregisterObject(QStringLiteral("/Waytoys/Zones")); }

    bool pointerMotion(PointerMotionEvent *e) override
    {
        if (!eligible()) return false;
        if (!e->modifiers.testFlag(Qt::ShiftModifier)) {
            if (owned) update(e->position, e->modifiers);
            return false;
        }
        update(e->position, e->modifiers);
        return true;
    }
    bool keyboardKey(KeyboardKeyEvent *e) override
    {
        if (!eligible() || (!owned && !e->modifiers.testFlag(Qt::ShiftModifier))) return false;
        if (e->state == KeyboardKeyState::Pressed && e->key == Qt::Key_Escape) {
            auto *w = window.data();
            cancel("escape");
            if (w) w->cancelInteractiveMoveResize();
            return true;
        }
        if (e->state == KeyboardKeyState::Pressed && (e->key == Qt::Key_Return || e->key == Qt::Key_Enter)) {
            finish(input()->globalPointer(), e->modifiers); return true;
        }
        if (e->state == KeyboardKeyState::Pressed || e->state == KeyboardKeyState::Repeated)
            window->keyPressEvent(uint(e->key) | uint(e->modifiers & ~Qt::ShiftModifier));
        if (window && window->isInteractiveMove()) update(input()->globalPointer(), input()->keyboardModifiers());
        return true;
    }
    bool pointerButton(PointerButtonEvent *e) override
    {
        if (!eligible() || !owned) return false;
        if (e->state == PointerButtonState::Released && e->buttons == Qt::NoButton) {
            finish(e->position, e->modifiers); return true;
        }
        return false;
    }

public Q_SLOTS:
    QString Snapshot() const
    {
        QJsonArray outputs;
        for (auto *o : workspace()->outputs()) outputs.append(QJsonObject{{"name", o->name()}, {"geometry", rect(o->geometry())}});
        return json({{"protocol", 1}, {"epoch", epoch}, {"outputs", outputs}, {"attached", enabled}});
    }
    bool Configure(const QString &value)
    {
        const auto service = QDBusConnection::sessionBus().interface()->serviceOwner(QStringLiteral("org.waytoys.Runtime1"));
        if (!calledFromDBus() || !service.isValid() || message().service() != service.value()) return false;
        const auto doc = QJsonDocument::fromJson(value.toUtf8());
        if (!doc.isObject() || doc.object().value("protocol").toInt() != 1) return false;
        const auto config = doc.object();
        if (!config.value("enabled").toBool()) { detach(); return true; }
        QStringList nextOutputs;
        for (const auto &v : config.value("outputs").toArray()) nextOutputs.append(v.toString());
        QStringList nextExcluded;
        for (const auto &v : config.value("excluded").toArray()) nextExcluded.append(v.toString());
        if (nextOutputs.isEmpty()) { detach(); return false; }
        outputs = nextOutputs; excluded = nextExcluded;
        transparent = config.value("transparent").toBool();
        owner = message().service(); watcher.setWatchedServices({owner}); missed = 0;
        if (!enabled) { input()->installInputEventFilter(this); enabled = true; }
        if (window && !allowed(window)) release("excluded");
        return true;
    }
    bool Target(const QString &value)
    {
        if (!calledFromDBus() || message().service() != owner || !enabled || !window) return false;
        const auto obj = QJsonDocument::fromJson(value.toUtf8()).object();
        if (obj.value("epoch").toString() != epoch || obj.value("id").toInteger() != id || obj.value("sequence").toInteger() != sequence) return false;
        const auto a = obj.value("rect").toArray();
        QRectF target;
        if (a.size() == 4) {
            target = QRectF(a[0].toDouble(), a[1].toDouble(), a[2].toDouble(), a[3].toDouble());
            if (!std::isfinite(target.x()) || !std::isfinite(target.y()) || !std::isfinite(target.width()) || !std::isfinite(target.height()) || target.width() <= 0 || target.height() <= 0) return false;
            auto *o = workspace()->outputAt(lastPosition);
            if (!o || !QRectF(o->geometry()).contains(target)) return false;
        }
        if (!pending) return true;
        timeout.stop();
        if (!target.isEmpty() && !window->isInteractiveMove() && !window->isInteractiveResize() && allowed(window)) {
            const QSizeF size = window->constrainFrameSize(target.size());
            if (size.width() <= target.width() + .01 && size.height() <= target.height() + .01) {
                target.setSize(size);
                window->moveResize(target);
                qInfo("Waytoys snap id=%lld x=%.1f y=%.1f w=%.1f h=%.1f", id, target.x(), target.y(), target.width(), target.height());
            } else qInfo("Waytoys snap skipped: window size constraint");
        }
        cancel("complete");
        return true;
    }

Q_SIGNALS:
    void Event(const QString &event);

private:
    bool allowed(Window *w) const
    {
        return w && w->isNormalWindow() && w->isMovable() && w->isResizable() && !w->isFullScreen()
            && !w->isMinimized() && !excluded.contains(w->resourceClass()) && !excluded.contains(w->desktopFileName())
            && !w->resourceClass().startsWith(QStringLiteral("waytoys"));
    }
    bool eligible()
    {
        auto *w = workspace()->moveResizeWindow();
        auto *o = workspace()->outputAt(input()->globalPointer());
        const bool result = enabled && !(waylandServer() && waylandServer()->isScreenLocked()) && w && w == window && w->isInteractiveMove()
            && allowed(w) && o && outputs.contains(o->name());
        if (!result && owned) release("ineligible");
        return result;
    }
    void track(Window *w)
    {
        connect(w, &Window::interactiveMoveResizeStarted, this, [this, w] {
            cancel("new-drag");
            if (!enabled || !w->isInteractiveMove() || !input()->qtButtonStates().testFlag(Qt::LeftButton) || !allowed(w)) return;
            window = w; ++id; sequence = 0;
            originalOpacity = w->opacity();
            if (eligible() && input()->keyboardModifiers().testFlag(Qt::ShiftModifier)) update(input()->globalPointer(), input()->keyboardModifiers());
        });
        connect(w, &Window::interactiveMoveResizeFinished, this, [this, w] { if (window == w && !finishing) cancel("native-finish"); });
        connect(w, &QObject::destroyed, this, [this, w] { if (window.data() == w || window.isNull()) cancel("window-closed"); });
    }
    void event(const QString &kind, const QPointF &pos, Qt::KeyboardModifiers mods)
    {
        auto *o = workspace()->outputAt(pos);
        emit Event(json({{"kind", kind}, {"epoch", epoch}, {"id", id}, {"sequence", ++sequence}, {"position", QJsonArray{pos.x(), pos.y()}},
                         {"output", o ? o->name() : QString()}, {"shift", mods.testFlag(Qt::ShiftModifier)}, {"ctrl", mods.testFlag(Qt::ControlModifier)}}));
    }
    void update(const QPointF &pos, Qt::KeyboardModifiers mods)
    {
        lastPosition = pos;
        const bool active = mods.testFlag(Qt::ShiftModifier);
        if (active && !owned) { owned = true; originalOpacity = window->opacity(); }
        if (active) {
            window->updateInteractiveMoveResize(pos, mods & ~Qt::ShiftModifier);
            workspace()->outline()->hide();
            window->setOpacity(transparent ? originalOpacity * .5 : originalOpacity);
        } else if (owned) {
            window->setOpacity(originalOpacity); owned = false;
            window->updateInteractiveMoveResize(pos, mods);
        }
        event("update", pos, mods);
    }
    void finish(const QPointF &pos, Qt::KeyboardModifiers mods)
    {
        if (!window) return;
        update(pos, mods);
        if (!owned) { window->endInteractiveMoveResize(); return; }
        auto *w = window.data();
        const QRectF freeGeometry = w->moveResizeGeometry();
        w->setOpacity(originalOpacity);
        finishing = true;
        w->endInteractiveMoveResize();
        // Shift was removed from KWin's move state. Undo edge-only tiling too,
        // before returning to the event loop; never touch the user's tile tree.
        w->setQuickTileModeAtCurrentPosition(QuickTileFlag::None);
        w->setMaximize(false, false);
        w->moveResize(freeGeometry);
        finishing = false; owned = false; pending = true;
        timeout.start(); event("drop", pos, mods);
    }
    void cancel(const QString &reason)
    {
        timeout.stop();
        if (window && owned) window->setOpacity(originalOpacity);
        if (window || pending || owned) emit Event(json({{"kind", "cancel"}, {"epoch", epoch}, {"id", id}, {"sequence", ++sequence}, {"reason", reason}}));
        window.clear(); owned = false; pending = false;
    }
    void detach()
    {
        release("detached");
        if (enabled && input()) input()->uninstallInputEventFilter(this);
        enabled = false; owner.clear(); watcher.setWatchedServices({});
    }
    void release(const QString &reason)
    {
        auto *w = window.data();
        cancel(reason);
        if (w && w->isInteractiveMove()) w->updateInteractiveMoveResize(input()->globalPointer(), input()->keyboardModifiers());
    }
    QPointer<Window> window;
    QDBusServiceWatcher watcher;
    QTimer timeout, lease;
    QString owner;
    const QString epoch = QUuid::createUuid().toString(QUuid::WithoutBraces);
    QStringList outputs, excluded;
    QPointF lastPosition;
    qint64 id = 0, sequence = 0;
    qreal originalOpacity = 1;
    int missed = 0;
    bool enabled = false, transparent = false, owned = false, pending = false, finishing = false;
};

class Factory : public PluginFactory
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID PluginFactory_iid FILE "metadata.json")
    Q_INTERFACES(KWin::PluginFactory)
public:
    std::unique_ptr<Plugin> create() const override { return std::make_unique<Bridge>(); }
};
#include "bridge.moc"
