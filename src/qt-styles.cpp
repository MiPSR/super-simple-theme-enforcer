#include <QCoreApplication>
#include <QStyleFactory>
#include <QTextStream>

// Read plugin metadata without opening a window or changing the session.
int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    QTextStream out(stdout);
    auto keys = QStyleFactory::keys();
    keys.sort(Qt::CaseInsensitive);
    for (const auto &key : keys) {
        // qt6ct's proxy is not a concrete widget style.
        if (key.compare(QStringLiteral("qt6ct-style"), Qt::CaseInsensitive) != 0)
            out << key << '\n';
    }
}
