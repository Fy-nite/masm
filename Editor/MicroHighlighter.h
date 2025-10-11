#pragma once

#include <QSyntaxHighlighter>
#include <QTextCharFormat>
#include <QRegularExpression>

class MicroHighlighter : public QSyntaxHighlighter {
    Q_OBJECT
public:
    explicit MicroHighlighter(QTextDocument *parent = nullptr);

protected:
    void highlightBlock(const QString &text) override;

private:
    struct Rule {
        QRegularExpression pattern;
        QTextCharFormat format;
    };

    QVector<Rule> rules;
    QRegularExpression labelDefPattern;          // e.g. label:
    QRegularExpression lblDirectivePattern;      // e.g. LBL name
    QRegularExpression includeDirectivePattern;  // #include ...
    QRegularExpression stringPattern;
    QRegularExpression commentPattern;
};
