#include <QApplication>
#include <QMainWindow>
#include <QPlainTextEdit>
#include <QMenuBar>
#include <QDockWidget>
#include <QFileDialog>
#include <QProcess>
#include <QMessageBox>
#include <QAction>
#include <QStatusBar>
#include <QFileInfo>
#include "CodeEditor.h"
#include "MicroHighlighter.h"
#include "SymbolsViewer.h"
#include "Indexer.h"
#include "AboutDialog.h"
#include "SettingsDialog.h"
#include <QThread>
#include <QTimer>
#include <QInputDialog>
#include <QListWidget>
#include <QDialog>
#include <QVBoxLayout>
#include <thread>

class MasmEditor : public QMainWindow {
    CodeEditor *editor;
    MicroHighlighter *highlighter{nullptr};
    QString currentFilePath;
    SymbolsViewer *symbolsViewer;
    QMap<QString,int> indexDefs;
    QMultiMap<QString,int> indexUses;
    QTimer *indexDebounce{nullptr};
public:
    MasmEditor() {
        editor = new CodeEditor(this);
        setCentralWidget(editor);
        highlighter = new MicroHighlighter(editor->document());

        // Make editor and window larger
        editor->setMinimumSize(900, 600);
        resize(1100, 700);

        // Add dockable, swappable symbols viewer
        symbolsViewer = new SymbolsViewer(this);
        QDockWidget *dock = new QDockWidget("Symbols", this);
        dock->setWidget(symbolsViewer);
        dock->setAllowedAreas(Qt::LeftDockWidgetArea | Qt::RightDockWidgetArea | Qt::TopDockWidgetArea | Qt::BottomDockWidgetArea);
        addDockWidget(Qt::LeftDockWidgetArea, dock);

        // Update symbols when text changes
        connect(editor, &QPlainTextEdit::textChanged, this, [this]() {
            symbolsViewer->updateSymbols(editor->toPlainText());
            // Debounced indexer update
            if (!indexDebounce) {
                indexDebounce = new QTimer(this);
                indexDebounce->setSingleShot(true);
                indexDebounce->setInterval(400);
                connect(indexDebounce, &QTimer::timeout, this, [this]() {
                    QString text = editor->toPlainText();
                    // run parsing on a background thread
                    std::thread([this, text]() {
                        IndexResult res = Indexer::parseText(text);
                        // post result back to UI thread
                        QMetaObject::invokeMethod(this, [this, res]() {
                            indexDefs = res.definitions;
                            indexUses = res.usages;
                        }, Qt::QueuedConnection);
                    }).detach();
                });
            }
            indexDebounce->start();
        });

        // Prime index immediately
        indexDebounce = new QTimer(this);
        indexDebounce->setSingleShot(true);
        indexDebounce->setInterval(400);
        indexDebounce->start();

        // Move cursor to symbol line on double-click
        connect(symbolsViewer, &SymbolsViewer::symbolActivated, this, [this](int line) {
            QTextCursor cursor(editor->document()->findBlockByLineNumber(line));
            editor->setTextCursor(cursor);
            editor->setFocus();
        });

        // Go-to-definition (F12)
        QAction *gotoDef = new QAction("Go to Definition", this);
        gotoDef->setShortcut(QKeySequence(Qt::Key_F12));
        addAction(gotoDef);
        connect(gotoDef, &QAction::triggered, this, [this]() {
            QTextCursor c = editor->textCursor();
            c.select(QTextCursor::WordUnderCursor);
            QString word = c.selectedText();
            if (indexDefs.contains(word)) {
                int line = indexDefs.value(word);
                QTextCursor cursor(editor->document()->findBlockByLineNumber(line));
                editor->setTextCursor(cursor);
                editor->setFocus();
            } else {
                QMessageBox::information(this, "Go to Definition", QString("No definition found for '%1'").arg(word));
            }
        });

        // Find usages
        QAction *findUsages = new QAction("Find Usages", this);
        findUsages->setShortcut(QKeySequence(Qt::ALT | Qt::Key_F7));
        addAction(findUsages);
        connect(findUsages, &QAction::triggered, this, [this]() {
            QTextCursor c = editor->textCursor();
            c.select(QTextCursor::WordUnderCursor);
            QString word = c.selectedText();
            QList<int> lines = indexUses.values(word);
            QDialog dlg(this);
            dlg.setWindowTitle(QString("Usages of '%1'").arg(word));
            QVBoxLayout *lay = new QVBoxLayout(&dlg);
            QListWidget *list = new QListWidget(&dlg);
            for (int ln : lines) {
                QString txt = editor->document()->findBlockByLineNumber(ln).text();
                list->addItem(QString("%1: %2").arg(ln+1).arg(txt));
            }
            lay->addWidget(list);
            connect(list, &QListWidget::itemDoubleClicked, &dlg, [&dlg, this, list](QListWidgetItem *item){
                int row = list->row(item);
                // parse line number from item text
                QString t = item->text();
                int colon = t.indexOf(':');
                if (colon > 0) {
                    int ln = t.left(colon).toInt() - 1;
                    QTextCursor cursor(editor->document()->findBlockByLineNumber(ln));
                    editor->setTextCursor(cursor);
                    editor->setFocus();
                    dlg.accept();
                }
            });
            dlg.exec();
        });

        QMenu *fileMenu = menuBar()->addMenu("File");
        QAction *openAct = fileMenu->addAction("Open");
    QAction *saveAct = fileMenu->addAction("Save");
    QAction *saveAsAct = fileMenu->addAction("Save As...");
    QAction *compileAct = fileMenu->addAction("Build");
    QAction *runAct = fileMenu->addAction("Build && Run");
    QAction *settingsAct = fileMenu->addAction("Settings...");

    QMenu *helpMenu = menuBar()->addMenu("Help");
    QAction *aboutAct = helpMenu->addAction("About");

        connect(openAct, &QAction::triggered, this, [this]() {
            QString file = QFileDialog::getOpenFileName(this, "Open MASM File", "", "MASM Files (*.masm *.masi);;All Files (*.*)");
            if (!file.isEmpty()) {
                QFile f(file);
                if (f.open(QIODevice::ReadOnly | QIODevice::Text))
                    editor->setPlainText(f.readAll());
                currentFilePath = file;
                statusBar()->showMessage(QString("Opened %1").arg(QFileInfo(file).fileName()), 3000);
            }
        });

        connect(saveAct, &QAction::triggered, this, [this]() {
            if (currentFilePath.isEmpty()) {
                QString file = QFileDialog::getSaveFileName(this, "Save MASM File", "", "MASM Files (*.masm *.masi)");
                if (file.isEmpty()) return;
                currentFilePath = file;
            }
            QFile f(currentFilePath);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text))
                f.write(editor->toPlainText().toUtf8());
            statusBar()->showMessage("Saved", 2000);
        });

        connect(saveAsAct, &QAction::triggered, this, [this]() {
            QString file = QFileDialog::getSaveFileName(this, "Save MASM File As", "", "MASM Files (*.masm *.masi)");
            if (file.isEmpty()) return;
            currentFilePath = file;
            QFile f(currentFilePath);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text))
                f.write(editor->toPlainText().toUtf8());
            statusBar()->showMessage("Saved As", 2000);
        });

        connect(settingsAct, &QAction::triggered, this, [this]() {
            SettingsDialog dlg(this);
            // load current masm path if needed (stub)
            if (dlg.exec() == QDialog::Accepted) {
                // TODO: persist settings via QSettings
            }
        });

        connect(aboutAct, &QAction::triggered, this, [this]() {
            AboutDialog dlg(this);
            dlg.exec();
        });

        connect(compileAct, &QAction::triggered, this, [this]() {
            // Save to temp file and run compiler
            QString tempFile = currentFilePath;
            if (tempFile.isEmpty()) tempFile = QDir::temp().filePath("temp.masm");
            QFile f(tempFile);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text))
                f.write(editor->toPlainText().toUtf8());
            f.close();

            QProcess proc;
            proc.start("masm", QStringList() << tempFile); 
            proc.waitForFinished();
            QString output = proc.readAllStandardOutput() + proc.readAllStandardError();
            QMessageBox::information(this, "Compiler Output", output);
        });

        connect(runAct, &QAction::triggered, this, [this]() {
            QString tempFile = currentFilePath;
            if (tempFile.isEmpty()) tempFile = QDir::temp().filePath("temp.masm");
            QFile f(tempFile);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text)) f.write(editor->toPlainText().toUtf8());
            f.close();

            QProcess build;
            build.start("masm", QStringList() << tempFile);
            build.waitForFinished();
            QString buildOut = build.readAllStandardOutput() + build.readAllStandardError();

            QProcess run;
            run.start("masm", QStringList() << tempFile);
            run.waitForFinished();
            QString runOut = run.readAllStandardOutput() + run.readAllStandardError();
            QMessageBox::information(this, "Build & Run Output", buildOut + "\n---\n" + runOut);
        });
    }
};

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);
    MasmEditor w;
    w.show();
    return app.exec();
}