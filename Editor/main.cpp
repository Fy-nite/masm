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
#include "Formatter.h"
#include "Diagnostics.h"
#include <QSettings>
#include <QFileInfo>
#include <QFile>
#include <QDir>
#include <QIODevice>
#include <QListWidgetItem>
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
    Diagnostics *diagnostics{nullptr};
    QListWidget *m_problemsList{nullptr};
    void emitDiagnosticsToUI(const QVector<Diagnostic> &diags, const QString &sourceFile = QString()) {
        if (!m_problemsList) return;
            m_problemsList->clear();
        for (const Diagnostic &d : diags) {
            QString label = QString("%1:%2: %3: %4").arg(QFileInfo(d.file).fileName()).arg(d.line+1).arg(d.severity).arg(d.message);
            QListWidgetItem *it = new QListWidgetItem(label, m_problemsList);
            it->setData(Qt::UserRole, d.line);
            it->setData(Qt::UserRole+1, d.file);
        }
        auto ce = dynamic_cast<CodeEditor*>(editor);
        if (ce) {
            if (!sourceFile.isEmpty() && !currentFilePath.isEmpty() && QFileInfo(sourceFile).absoluteFilePath() == QFileInfo(currentFilePath).absoluteFilePath()) {
                ce->setDiagnostics(diags);
            } else {
                bool shown = false;
                for (const Diagnostic &d : diags) {
                    if (!currentFilePath.isEmpty() && QFileInfo(d.file).absoluteFilePath() == QFileInfo(currentFilePath).absoluteFilePath()) {
                        ce->setDiagnostics(diags);
                        shown = true;
                        break;
                    }
                }
                if (!shown) ce->clearDiagnostics();
            }
        }
    }
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
            // Debounced indexer update and diagnostics
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

                    // run quick in-IDE diagnostics (fast heuristics)
                    auto ce = dynamic_cast<CodeEditor*>(editor);
                    if (diagnostics) {
                        // analyze in background to avoid UI hitch
                        std::thread([this, text]() {
                            QVector<Diagnostic> diags = diagnostics->analyzeText(text, currentFilePath);
                            // call member on UI thread via a queued lambda
                            QMetaObject::invokeMethod(this, [this, diags]() {
                                this->emitDiagnosticsToUI(diags);
                            }, Qt::QueuedConnection);
                        }).detach();
                    }
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
            if (line < 0) return;
            QTextCursor cursor(editor->document()->findBlockByLineNumber(line));
            editor->setTextCursor(cursor);
            editor->setFocus();
        });

        connect(symbolsViewer, &SymbolsViewer::includeRequested, this, [this](const QString &filename){
            QString path = filename;
            // if relative, resolve against current file directory
            if (!QFileInfo(path).isAbsolute() && !currentFilePath.isEmpty()) {
                path = QFileInfo(currentFilePath).absolutePath() + QDir::separator() + filename;
            }
            if (!QFile::exists(path)) {
                // also try workspace-relative
                if (QFile::exists(filename)) path = filename;
            }
            if (QFile::exists(path)) {
                QFile f(path);
                if (f.open(QIODevice::ReadOnly | QIODevice::Text)) {
                    editor->setPlainText(f.readAll());
                    currentFilePath = path;
                    statusBar()->showMessage(QString("Opened %1").arg(QFileInfo(path).fileName()), 3000);
                }
            } else {
                QMessageBox::warning(this, "Open Include", QString("Could not find included file: %1").arg(filename));
            }
        });

    // Problems pane and diagnostics
    diagnostics = new Diagnostics(this);
    QDockWidget *problemsDock = new QDockWidget("Problems", this);
    m_problemsList = new QListWidget(this);
    problemsDock->setWidget(m_problemsList);
    addDockWidget(Qt::BottomDockWidgetArea, problemsDock);

        // helper: update Problems list and editor
        auto emitDiagnosticsToUI = [this](const QVector<Diagnostic> &diags, const QString &sourceFile = QString()) {
            if (!m_problemsList) return;
            m_problemsList->clear();
            for (const Diagnostic &d : diags) {
                QString label = QString("%1:%2: %3: %4").arg(QFileInfo(d.file).fileName()).arg(d.line+1).arg(d.severity).arg(d.message);
                QListWidgetItem *it = new QListWidgetItem(label, m_problemsList);
                it->setData(Qt::UserRole, d.line);
                it->setData(Qt::UserRole+1, d.file);
            }
            auto ce = dynamic_cast<CodeEditor*>(editor);
            if (ce) {
                // if sourceFile provided and matches currentFilePath, show; otherwise clear
                if (!sourceFile.isEmpty() && !currentFilePath.isEmpty() && QFileInfo(sourceFile).absoluteFilePath() == QFileInfo(currentFilePath).absoluteFilePath()) {
                    ce->setDiagnostics(diags);
                } else {
                    // if any diag file matches current file, show; otherwise clear
                    bool shown = false;
                    for (const Diagnostic &d : diags) {
                        if (!currentFilePath.isEmpty() && QFileInfo(d.file).absoluteFilePath() == QFileInfo(currentFilePath).absoluteFilePath()) {
                            ce->setDiagnostics(diags);
                            shown = true;
                            break;
                        }
                    }
                    if (!shown) ce->clearDiagnostics();
                }
            }
        };

        connect(diagnostics, &Diagnostics::diagnosticsReady, this, [this, emitDiagnosticsToUI](const QString &file, const QVector<Diagnostic> &diags){
            emitDiagnosticsToUI(diags, file);
        });

        connect(m_problemsList, &QListWidget::itemDoubleClicked, this, [this](QListWidgetItem *item){
            int line = item->data(Qt::UserRole).toInt();
            QString file = item->data(Qt::UserRole+1).toString();
            if (!QFileInfo(file).exists()) return;
            QFile f(file);
            if (f.open(QIODevice::ReadOnly | QIODevice::Text)) {
                editor->setPlainText(f.readAll());
                currentFilePath = file;
                if (line >= 0) {
                    QTextCursor cursor(editor->document()->findBlockByLineNumber(line));
                    editor->setTextCursor(cursor);
                }
            }
        });

        // Go-to-definition (F12)
        QAction *gotoDef = new QAction("Go to Definition", this);
        gotoDef->setShortcut(QKeySequence(Qt::Key_F12));
        addAction(gotoDef);
        connect(gotoDef, &QAction::triggered, this, [this]() {
            QTextCursor c = editor->textCursor();
            c.select(QTextCursor::WordUnderCursor);
            QString word = c.selectedText().toLower();
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
            QString word = c.selectedText().toLower();
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
    QAction *formatAct = fileMenu->addAction("Format Document");
    QAction *settingsAct = fileMenu->addAction("Settings...");
    settingsAct->setShortcut(QKeySequence("Ctrl+,"));

    QMenu *helpMenu = menuBar()->addMenu("Help");
    QAction *aboutAct = helpMenu->addAction("About");
    QAction *dumpAct = helpMenu->addAction("Dump Symbols...");
    dumpAct->setShortcut(QKeySequence("Ctrl+Shift+P"));

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
            if (diagnostics) {
                diagnostics->analyzeTextAndEmit(editor->toPlainText(), currentFilePath);
                diagnostics->run(currentFilePath);
            }
        });

        connect(saveAsAct, &QAction::triggered, this, [this]() {
            QString file = QFileDialog::getSaveFileName(this, "Save MASM File As", "", "MASM Files (*.masm *.masi)");
            if (file.isEmpty()) return;
            currentFilePath = file;
            QFile f(currentFilePath);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text))
                f.write(editor->toPlainText().toUtf8());
            statusBar()->showMessage("Saved As", 2000);
            if (diagnostics) {
                diagnostics->analyzeTextAndEmit(editor->toPlainText(), currentFilePath);
                diagnostics->run(currentFilePath);
            }
        });

        connect(settingsAct, &QAction::triggered, this, [this]() {
            SettingsDialog dlg(this);
            dlg.loadSettings();
            if (dlg.exec() == QDialog::Accepted) {
                dlg.saveSettings();
                // apply completions setting if editor supports it
                bool comps = dlg.completionsEnabled();
                // CodeEditor may provide setCompletionsEnabled; call if available
                // we'll attempt a dynamic cast and call; if not present, ignore
                auto ce = dynamic_cast<CodeEditor*>(editor);
                if (ce) {
                    // safe to call if implemented
                    ce->setCompletionsEnabled(comps);
                }
            }
        });

        // Apply persisted settings on startup
        {
            QSettings s("masm", "MasmEditor");
            QString masmPath = s.value("masm/path", "").toString();
            bool comps = s.value("editor/completions", true).toBool();
            auto ce = dynamic_cast<CodeEditor*>(editor);
            if (ce) ce->setCompletionsEnabled(comps);
        }

        connect(formatAct, &QAction::triggered, this, [this]() {
            QString before = editor->toPlainText();
            QString after = Formatter::format(before);
            if (after != before) {
                editor->setPlainText(after);
            }
        });

        connect(aboutAct, &QAction::triggered, this, [this]() {
            AboutDialog dlg(this);
            dlg.exec();
        });

        connect(dumpAct, &QAction::triggered, this, [this]() {
            symbolsViewer->printContents();
        });

        connect(compileAct, &QAction::triggered, this, [this]() {
            // Save to temp file and run compiler
            QString tempFile = currentFilePath;
            if (tempFile.isEmpty()) tempFile = QDir::temp().filePath("temp.masm");
            QFile f(tempFile);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text))
                f.write(editor->toPlainText().toUtf8());
            f.close();

            // resolve masm executable from settings or tools folder
            QSettings s("masm", "MasmEditor");
            QString masmExe = s.value("masm/path", "").toString();
            if (masmExe.isEmpty()) {
                // look for tools/masm next to the editor exe
                QString exeDir = QCoreApplication::applicationDirPath();
                QString candidate = exeDir + QDir::separator() + "tools" + QDir::separator() + "masm";
                if (QFile::exists(candidate) || QFile::exists(candidate + ".exe")) masmExe = candidate;
            }
            if (masmExe.isEmpty()) masmExe = QStringLiteral("masm");

            // compute stdlib dir next to masm if present
            QString stdlibDir;
            if (!masmExe.isEmpty()) {
                QFileInfo fi(masmExe);
                stdlibDir = fi.absolutePath() + QDir::separator() + "stdlib";
            }

            QProcess proc;
            proc.start(masmExe, QStringList() << tempFile);
            proc.waitForFinished();
            QString output = proc.readAllStandardOutput() + proc.readAllStandardError();
            QMessageBox::information(this, "Compiler Output", output);
            if (diagnostics) diagnostics->analyzeTextAndEmit(editor->toPlainText(), tempFile);
            diagnostics->run(tempFile);
        });

        connect(runAct, &QAction::triggered, this, [this]() {
            QString tempFile = currentFilePath;
            if (tempFile.isEmpty()) tempFile = QDir::temp().filePath("temp.masm");
            QFile f(tempFile);
            if (f.open(QIODevice::WriteOnly | QIODevice::Text)) f.write(editor->toPlainText().toUtf8());
            f.close();

            QSettings s2("masm", "MasmEditor");
            QString masmExe2 = s2.value("masm/path", "").toString();
            if (masmExe2.isEmpty()) {
                QString exeDir = QCoreApplication::applicationDirPath();
                QString candidate = exeDir + QDir::separator() + "tools" + QDir::separator() + "masm";
                if (QFile::exists(candidate) || QFile::exists(candidate + ".exe")) masmExe2 = candidate;
            }
            if (masmExe2.isEmpty()) masmExe2 = QStringLiteral("masm");

            QProcess build;
            build.start(masmExe2, QStringList() << tempFile);
            build.waitForFinished();
            QString buildOut = build.readAllStandardOutput() + build.readAllStandardError();

            QProcess run;
            run.start(masmExe2, QStringList() << tempFile);
            run.waitForFinished();
            QString runOut = run.readAllStandardOutput() + run.readAllStandardError();
            QMessageBox::information(this, "Build & Run Output", buildOut + "\n---\n" + runOut);
            if (diagnostics) diagnostics->analyzeTextAndEmit(editor->toPlainText(), tempFile);
            diagnostics->run(tempFile);
        });
    }
};

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);
    MasmEditor w;
    w.show();
    return app.exec();
}