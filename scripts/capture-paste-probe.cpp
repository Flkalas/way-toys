// A real Wayland clipboard consumer and deterministic capture target for VM tests.
#include <QApplication>
#include <QClipboard>
#include <QCryptographicHash>
#include <QImage>
#include <QKeyEvent>
#include <QPainter>
#include <QWidget>
#include <iostream>

class Probe : public QWidget {
public:
    explicit Probe(QString output) : outputPath(std::move(output)) {
        setWindowTitle("WaytoysCaptureProbe");
        resize(500, 320);
    }
protected:
    void paintEvent(QPaintEvent *) override {
        QPainter p(this);
        const int w=width()/2, h=height()/2;
        p.fillRect(0,0,w,h,QColor("#d32f2f"));
        p.fillRect(w,0,width()-w,h,QColor("#388e3c"));
        p.fillRect(0,h,w,height()-h,QColor("#1976d2"));
        p.fillRect(w,h,width()-w,height()-h,QColor("#fbc02d"));
        if (!pasted.isNull()) {
            p.fillRect(rect(),Qt::white);
            p.drawImage(10,35,pasted);
            p.setPen(Qt::black);
            p.drawText(10,22,QString("Pasted image: %1 x %2").arg(pasted.width()).arg(pasted.height()));
        }
    }
    void keyPressEvent(QKeyEvent *event) override {
        if (event->matches(QKeySequence::Paste)) {
            pasted=QApplication::clipboard()->image();
            if (pasted.isNull()) {std::cout << "{\"paste\":false}" << std::endl;return;}
            const auto rgba=pasted.convertToFormat(QImage::Format_RGBA8888);
            const auto hash=QCryptographicHash::hash(QByteArrayView(reinterpret_cast<const char*>(rgba.constBits()),rgba.sizeInBytes()),QCryptographicHash::Sha256).toHex();
            const bool saved=outputPath.isEmpty() || pasted.save(outputPath,"PNG");
            std::cout << "{\"paste\":true,\"width\":" << pasted.width() << ",\"height\":" << pasted.height() << ",\"rgba_sha256\":\"" << hash.constData() << "\",\"saved\":" << (saved?"true":"false") << "}" << std::endl;
            update();
        } else if (event->key()==Qt::Key_R) {pasted=QImage();update();}
        else {QWidget::keyPressEvent(event);}
    }
private:
    QString outputPath;
    QImage pasted;
};
int main(int argc,char **argv) {
    QApplication app(argc,argv);
    app.setDesktopFileName("org.waytoys.CaptureProbe");
    Probe probe(argc>1?QString::fromLocal8Bit(argv[1]):QString());
    probe.show();
    return app.exec();
}
