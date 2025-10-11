#include <QApplication>
#include <Qsci/qsciscintilla.h>
#include <Qsci/qscilexercpp.h>
#include <QWidget>
#include <QVBoxLayout>

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);

    QWidget window;
    QVBoxLayout layout(&window);

    QsciScintilla *editor = new QsciScintilla();
    QsciLexerCPP *lexer = new QsciLexerCPP();
    editor->setLexer(lexer);
    editor->setUtf8(true);

    layout.addWidget(editor);
    window.setWindowTitle("Simple C++ Code Editor");
    window.resize(800, 600);
    window.show();

    return app.exec();
}