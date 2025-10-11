#include "CodeEditor.h"
#include <QPainter>
#include <QTextBlock>
#include <QAbstractItemView>
#include <QScrollBar>
#include <QKeyEvent>

CodeEditor::CodeEditor(QWidget *parent)
    : QPlainTextEdit(parent) {
    m_lineNumberArea = new LineNumberArea(this);

    connect(this, &CodeEditor::blockCountChanged, this, &CodeEditor::updateLineNumberAreaWidth);
    connect(this, &CodeEditor::updateRequest, this, &CodeEditor::updateLineNumberArea);
    connect(this, &CodeEditor::cursorPositionChanged, this, &CodeEditor::highlightCurrentLine);
    connect(this, &CodeEditor::cursorPositionChanged, this, &CodeEditor::highlightMatchingBrackets);

    updateLineNumberAreaWidth(0);
    highlightCurrentLine();
    initCompleter();
}

int CodeEditor::lineNumberAreaWidth() const {
    int digits = 1;
    int max = qMax(1, blockCount());
    while (max >= 10) {
        max /= 10;
        ++digits;
    }
    int space = 3 + fontMetrics().horizontalAdvance(QLatin1Char('9')) * digits;
    return space;
}

void CodeEditor::updateLineNumberAreaWidth(int) {
    if (m_lineNumberArea)
        setViewportMargins(lineNumberAreaWidth(), 0, 0, 0);
}

void CodeEditor::updateLineNumberArea(const QRect &rect, int dy) {
    if (m_lineNumberArea) {
        if (dy)
            m_lineNumberArea->scroll(0, dy);
        else
            m_lineNumberArea->update(0, rect.y(), m_lineNumberArea->width(), rect.height());

        if (rect.contains(viewport()->rect()))
            updateLineNumberAreaWidth(0);
    }
}

void CodeEditor::resizeEvent(QResizeEvent *e) {
    QPlainTextEdit::resizeEvent(e);
    QRect cr = contentsRect();
    if (m_lineNumberArea)
        m_lineNumberArea->setGeometry(QRect(cr.left(), cr.top(), lineNumberAreaWidth(), cr.height()));
}

void CodeEditor::lineNumberAreaPaintEvent(QPaintEvent *event) {
    if (!m_lineNumberArea) return;
    QPainter painter(m_lineNumberArea);
    painter.fillRect(event->rect(), QColor(245, 245, 245));

    QTextBlock block = firstVisibleBlock();
    int blockNumber = block.blockNumber();
    int top = qRound(blockBoundingGeometry(block).translated(contentOffset()).top());
    int bottom = top + qRound(blockBoundingRect(block).height());

    while (block.isValid() && top <= event->rect().bottom()) {
        if (block.isVisible() && bottom >= event->rect().top()) {
            QString number = QString::number(blockNumber + 1);
            painter.setPen(Qt::gray);
            painter.drawText(0, top, m_lineNumberArea->width() - 5, fontMetrics().height(), Qt::AlignRight, number);
        }

        block = block.next();
        top = bottom;
        bottom = top + qRound(blockBoundingRect(block).height());
        ++blockNumber;
    }
}

void CodeEditor::highlightCurrentLine() {
    if (isReadOnly()) return;
    QTextEdit::ExtraSelection selection;
    selection.format.setBackground(QColor(232, 242, 254));
    selection.format.setProperty(QTextFormat::FullWidthSelection, true);
    selection.cursor = textCursor();
    selection.cursor.clearSelection();
    setExtraSelections({ selection });
}

void CodeEditor::paintEvent(QPaintEvent *e) {
    QPlainTextEdit::paintEvent(e);
}

void CodeEditor::initCompleter() {
    if (m_completer) return;
    QStringList words = {
        // Instructions and directives (subset for completeness)
        "MOV","MOVZX","MOVSX","ADD","SUB","MUL","DIV","INC","DEC","CMP",
        "JMP","JE","JNE","JZ","JNZ","JS","JNS","JC","JNC","JB","JAE","JNB","JO","JNO","JG","JNLE","JL","JNGE","JGE","JNL","JLE","JNG",
        "CALL","RET","PUSH","POP","ENTER","LEAVE","IN","OUT","COUT","HLT","EXIT","ARGC","GETARG",
        "AND","OR","XOR","NOT","SHL","SHR","SAR","MOVADDR","MOVTO","COPY","FILL","CMP_MEM","SYSCALL","MNI","LBL",
        // Directives
        "#include","STATE","DB","DW","DD","DQ","DF","DDbl","RESB","RESW","RESD","RESQ","RESF","RESDbl",
        // Registers
        "RAX","RBX","RCX","RDX","RSI","RDI","RBP","RSP","RIP","R0","R1","R2","R3","R4","R5","R6","R7","R8","R9","R10","R11","R12","R13","R14","R15",
        "FPR0","FPR1","FPR2","FPR3","FPR4","FPR5","FPR6","FPR7","FPR8","FPR9","FPR10","FPR11","FPR12","FPR13","FPR14","FPR15"
    };

    words.sort(Qt::CaseInsensitive);
    m_completer = new QCompleter(words, this);
    m_completer->setCaseSensitivity(Qt::CaseInsensitive);
    m_completer->setWrapAround(false);
    m_completer->setFilterMode(Qt::MatchContains);
    m_completer->setWidget(this);
    connect(m_completer, SIGNAL(activated(QString)), this, SLOT(insertCompletion(QString)));
}

QString CodeEditor::textUnderCursor() const {
    QTextCursor tc = textCursor();
    tc.select(QTextCursor::WordUnderCursor);
    return tc.selectedText();
}

void CodeEditor::insertCompletion(const QString &completion) {
    QTextCursor tc = textCursor();
    tc.select(QTextCursor::WordUnderCursor);
    tc.removeSelectedText();
    tc.insertText(completion);
    setTextCursor(tc);
}

void CodeEditor::keyPressEvent(QKeyEvent *e) {
    if (m_completer && m_completer->popup()->isVisible()) {
        switch (e->key()) {
        case Qt::Key_Enter:
        case Qt::Key_Return:
        case Qt::Key_Escape:
        case Qt::Key_Tab:
        case Qt::Key_Backtab:
            e->ignore();
            return; // let the completer handle it
        default:
            break;
        }
    }

    QPlainTextEdit::keyPressEvent(e);

    const bool ctrlOrShift = e->modifiers() & (Qt::ControlModifier | Qt::ShiftModifier);
    if (!m_completer || (ctrlOrShift && e->text().isEmpty())) return;

    static QString eow = QStringLiteral("~!@#$%^&*()+{}|:\\\\\"<>?,./;'[]-= \t\n");
    bool isShortcut = (e->modifiers() & Qt::ControlModifier) && e->key() == Qt::Key_Space; // Ctrl+Space
    if (!isShortcut) {
        QString completionPrefix = textUnderCursor();
        if (completionPrefix.length() < 1 || eow.contains(e->text().right(1))) {
            m_completer->popup()->hide();
            return;
        }
        m_completer->setCompletionPrefix(completionPrefix);
    }

    if (isVisible() && hasFocus()) {
        QRect cr = cursorRect();
        if (cr.isValid()) {
            cr.setWidth(m_completer->popup()->sizeHintForColumn(0)
                + m_completer->popup()->verticalScrollBar()->sizeHint().width());
            m_completer->complete(cr);
        }
    }
}

void CodeEditor::highlightMatchingBrackets() {
    QList<QTextEdit::ExtraSelection> extras;

    auto highlightAt = [&](int pos){
        QTextEdit::ExtraSelection sel;
        sel.format.setBackground(QColor(210, 230, 255));
        QTextCursor c = textCursor();
        c.setPosition(pos);
        c.movePosition(QTextCursor::NextCharacter, QTextCursor::KeepAnchor);
        sel.cursor = c;
        extras.append(sel);
    };

    QTextCursor c = textCursor();
    if (!c.atBlockEnd()) {
        c.movePosition(QTextCursor::NextCharacter, QTextCursor::KeepAnchor);
        QString ch = c.selectedText();
        if (ch.size() == 1) {
            QChar qc = ch[0];
            const int SEARCH_LIMIT = 1000; // Prevent infinite loop/hang
            if (m_bracketsOpen.contains(qc)) {
                // find forward match
                int depth = 1;
                int pos = c.position();
                int searched = 0;
                while (pos < document()->characterCount() && searched < SEARCH_LIMIT) {
                    QChar cur = document()->characterAt(pos);
                    if (cur == qc) depth++;
                    if ((qc == '(' && cur == ')') || (qc == '[' && cur == ']') || (qc == '{' && cur == '}')) {
                        depth--;
                        if (depth == 0) { highlightAt(pos); break; }
                    }
                    ++pos;
                    ++searched;
                }
                highlightAt(c.position() - 1);
            } else if (m_bracketsClose.contains(qc)) {
                // find backward match
                QChar open = '(';
                if (qc == ')') open = '('; else if (qc == ']') open = '['; else if (qc == '}') open = '{';
                int depth = 1;
                int pos = c.position() - 2;
                int searched = 0;
                while (pos >= 0 && searched < SEARCH_LIMIT) {
                    QChar cur = document()->characterAt(pos);
                    if (cur == qc) depth++;
                    if (cur == open) {
                        depth--;
                        if (depth == 0) { highlightAt(pos); break; }
                    }
                    --pos;
                    ++searched;
                }
                highlightAt(c.position() - 1);
            }
        }
    }

    setExtraSelections(extras);
}
