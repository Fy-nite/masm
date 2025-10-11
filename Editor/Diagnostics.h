#pragma once
#include <QObject>
#include <QString>
#include <QVector>
#include <QProcess>
#include <QDir>

struct Diagnostic {
    QString file;
    int line{-1};
    int column{-1};
    int length{0};
    QString severity; // e.g., Error, Warning
    QString message;
};

class Diagnostics : public QObject {
    Q_OBJECT
public:
    explicit Diagnostics(QObject *parent = nullptr);
    // run the configured masm on the given file and emit diagnostics when ready
    void run(const QString &filePath);
    // analyze raw source text inside the IDE (fast, heuristic checks)
    QVector<Diagnostic> analyzeText(const QString &text, const QString &basePath = QString());
    // convenience: analyze and emit diagnosticsReady for the given text/file
    void analyzeTextAndEmit(const QString &text, const QString &basePath = QString());

signals:
    void diagnosticsReady(const QString &filePath, const QVector<Diagnostic> &diags);

private slots:
    void onProcessFinished(int exitCode, QProcess::ExitStatus status);

private:
    QProcess *m_proc{nullptr};
    QString m_targetFile;
    QVector<Diagnostic> parseOutput(const QString &output);
};
