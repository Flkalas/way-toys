// VM-only size-hint fixture, built against Qt6 Widgets.
#include <QApplication>
#include <QWidget>
#include <QLabel>
int main(int argc, char **argv)
{
    QApplication app(argc, argv);
    QApplication::setDesktopFileName("org.example.ZoneConstraintProbe");
    QWidget window;
    window.setWindowTitle("WaytoysProbe");
    window.setMinimumSize(600, 500);
    window.resize(620, 510);
    QLabel text("Minimum size: 600 x 500. A smaller zone must be refused.", &window);
    text.move(20, 20);
    window.show();
    return app.exec();
}
