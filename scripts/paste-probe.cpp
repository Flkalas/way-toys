// Test-only Wayland producer/consumer; all clipboard contents are fixed fixtures.
#include <QApplication>
#include <QClipboard>
#include <QCryptographicHash>
#include <QFile>
#include <QImage>
#include <QJsonDocument>
#include <QJsonObject>
#include <QKeyEvent>
#include <QLabel>
#include <QMimeData>
#include <QTextEdit>
#include <QVBoxLayout>
#include <QWidget>

class Probe : public QWidget {
public:
    QTextEdit* editor;
    QString output;
    explicit Probe(QString path) : output(std::move(path)) {
        setWindowTitle("WaytoysPasteProbe");
        auto* layout=new QVBoxLayout(this);
        layout->addWidget(new QLabel(QString::fromUtf8("Ctrl+1 텍스트 복사 · Ctrl+2 이미지 복사 · Ctrl+3 민감 표시 · Ctrl+V 검사")));
        editor=new QTextEdit;
        editor->setPlainText(QString::fromUtf8("Waytoys clipboard 한글 확인\n두 번째 줄 · 27"));
        layout->addWidget(editor);
        resize(600,280);
    }
    bool eventFilter(QObject*,QEvent* event) override {
        if(event->type()!=QEvent::KeyPress) return false;
        auto* key=static_cast<QKeyEvent*>(event);
        if(!(key->modifiers()&Qt::ControlModifier)) return false;
        auto* clipboard=QApplication::clipboard();
        if(key->key()==Qt::Key_1){editor->selectAll();editor->copy();return true;}
        if(key->key()==Qt::Key_2){
            QImage image(96,64,QImage::Format_RGBA8888);
            for(int y=0;y<64;y++) for(int x=0;x<96;x++) image.setPixelColor(x,y,QColor(x<48?211:56,y<32?47:142,60));
            clipboard->setImage(image);return true;
        }
        if(key->key()==Qt::Key_3){auto* mime=new QMimeData;mime->setText("DO_NOT_SAVE_TEST_SECRET_27");mime->setData("x-kde-passwordManagerHint","secret");clipboard->setMimeData(mime);return true;}
        if(key->key()==Qt::Key_V){
            QJsonObject result;
            if(clipboard->mimeData()->hasImage()){
                auto image=clipboard->image().convertToFormat(QImage::Format_RGBA8888);
                QByteArray pixels;for(int y=0;y<image.height();y++)pixels.append(reinterpret_cast<const char*>(image.constScanLine(y)),image.width()*4);
                result={{"kind","image"},{"width",image.width()},{"height",image.height()},{"sha256",QString(QCryptographicHash::hash(pixels,QCryptographicHash::Sha256).toHex())}};
            }else{auto value=clipboard->text().toUtf8();result={{"kind","text"},{"bytes",value.size()},{"sha256",QString(QCryptographicHash::hash(value,QCryptographicHash::Sha256).toHex())}};}
            QFile file(output);if(file.open(QIODevice::Append))file.write(QJsonDocument(result).toJson(QJsonDocument::Compact)+'\n');return true;
        }
        return false;
    }
};
int main(int argc,char** argv){QApplication app(argc,argv);QApplication::setDesktopFileName("org.waytoys.PasteProbe");if(argc!=2)return 2;Probe probe(QString::fromLocal8Bit(argv[1]));app.installEventFilter(&probe);probe.show();return app.exec();}
