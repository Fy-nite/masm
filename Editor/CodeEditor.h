#pragma once

#include <QPlainTextEdit>
#include <QAbstractItemView>
#include <QWidget>
#include <QCompleter>
#include <QSet>
#include "Diagnostics.h"
#include <QMouseEvent>
#include <QToolTip>

class LineNumberArea;

class CodeEditor : public QPlainTextEdit {
    Q_OBJECT
public:
    explicit CodeEditor(QWidget *parent = nullptr);
    // Diagnostics API: show diagnostics as squiggles and tooltips
    void setDiagnostics(const QVector<Diagnostic> &diags);
    void clearDiagnostics();
    void setCompletionsEnabled(bool v) { m_completionsEnabled = v; if (!v && m_completer) m_completer->popup()->hide(); }
    bool completionsEnabled() const { return m_completionsEnabled; }
    int lineNumberAreaWidth() const;
    void lineNumberAreaPaintEvent(QPaintEvent *event);

protected:
    void resizeEvent(QResizeEvent *event) override;
    void keyPressEvent(QKeyEvent *e) override;
    void paintEvent(QPaintEvent *e) override;
    void mouseMoveEvent(QMouseEvent *event) override;
    void leaveEvent(QEvent *event) override;

private slots:
    void updateLineNumberAreaWidth(int newBlockCount);
    void highlightCurrentLine();
    void updateLineNumberArea(const QRect &, int);
    void insertCompletion(const QString &completion);

private:
    void initCompleter();
    QString textUnderCursor() const;
    void highlightMatchingBrackets();
    void autoIndentCurrentLine();

    QWidget *m_lineNumberArea;
    QCompleter *m_completer{nullptr};
    QSet<QChar> m_bracketsOpen{ '(', '[', '{' };
    QSet<QChar> m_bracketsClose{ ')', ']', '}' };
    bool m_completionsEnabled{true};
    QVector<Diagnostic> m_diagnostics;
    int m_lastTooltipPosition{-1};
};

class LineNumberArea : public QWidget {
public:
    explicit LineNumberArea(CodeEditor *editor) : QWidget(editor), codeEditor(editor) {}
    QSize sizeHint() const override { return QSize(codeEditor->lineNumberAreaWidth(), 0); }
protected:
    void paintEvent(QPaintEvent *event) override { codeEditor->lineNumberAreaPaintEvent(event); }
private:
    CodeEditor *codeEditor;
};
