#include "MicroHighlighter.h"
#include <QColor>
#include <QTextCharFormat>

MicroHighlighter::MicroHighlighter(QTextDocument *parent)
    : QSyntaxHighlighter(parent) {
    // Define formats
    QTextCharFormat instrFmt; instrFmt.setForeground(QColor(0, 255, 255)); instrFmt.setFontWeight(QFont::Bold); // Cyan
    QTextCharFormat regFmt; regFmt.setForeground(QColor(255, 0, 255)); // Magenta
    QTextCharFormat directiveFmt; directiveFmt.setForeground(QColor(0, 255, 0)); directiveFmt.setFontWeight(QFont::DemiBold); // Bright green
    QTextCharFormat labelFmt; labelFmt.setForeground(QColor(255, 165, 0)); labelFmt.setFontWeight(QFont::Bold); // Orange
    QTextCharFormat commentFmt; commentFmt.setForeground(QColor(255, 255, 0)); // Yellow
    QTextCharFormat stringFmt; stringFmt.setForeground(QColor(255, 0, 0)); // Bright red
    QTextCharFormat numberFmt; numberFmt.setForeground(QColor(0, 255, 127)); // Spring green

    // Keywords/instructions (case-insensitive)
    const QStringList instructions = {
        // Basic
        "MOV","MOVZX","MOVSX","ADD","SUB","MUL","DIV","INC","DEC","CMP",
        // Flow
        "JMP","JE","JNE","JZ","JNZ","JS","JNS","JC","JNC","JB","JAE","JNB","JO","JNO","JG","JNLE","JL","JNGE","JGE","JNL","JLE","JNG","CALL","RET",
        // Stack
        "PUSH","POP","ENTER","LEAVE",
        // IO
        "IN","OUT","COUT",
        // Program
        "HLT","EXIT","ARGC","GETARG",
        // Memory/bit ops
        "AND","OR","XOR","NOT","SHL","SHR","SAR","MOVADDR","MOVTO","COPY","FILL","CMP_MEM",
        // FP
        "FMOV","FADD","FSUB","FMUL","FDIV","FCMP","CVTSI2SD","CVTUI2SD","CVTSD2SI","CVTSD2UI","CVTSS2SD","CVTSD2SS",
        // Sys/MNI
        "SYSCALL","MNI",
        // Label directive alias
        "LBL"
    };

    for (const QString &kw : instructions) {
        Rule r; r.pattern = QRegularExpression(QStringLiteral("\\b%1\\b").arg(kw), QRegularExpression::CaseInsensitiveOption); r.format = instrFmt; rules.push_back(r);
    }

    // Registers
    const QString regPattern = R"((?i)\b(RAX|RBX|RCX|RDX|RSI|RDI|RBP|RSP|RIP|R1[0-5]?|R0|R2|R3|R4|R5|R6|R7|R8|R9|FPR1[0-5]?|FPR[0-9])\b)";
    Rule regRule; regRule.pattern = QRegularExpression(regPattern); regRule.format = regFmt; rules.push_back(regRule);

    // Directives (#include, data defs, STATE, RES*, DB/DW/DD/DQ/DF/DDbl)
    const QStringList directives = { "#include", "STATE", "DB", "DW", "DD", "DQ", "DF", "DDbl", "RESB", "RESW", "RESD", "RESQ", "RESF", "RESDbl" };
    for (const QString &d : directives) {
        Rule r; r.pattern = QRegularExpression(QStringLiteral("(?i)\\b%1\\b").arg(QRegularExpression::escape(d))); r.format = directiveFmt; rules.push_back(r);
    }

    // Numbers (hex 0x..., decimal, negative)
    Rule numRule; numRule.pattern = QRegularExpression(R"((?i)\b(0x[0-9A-F]+|-?\d+)\b)"); numRule.format = numberFmt; rules.push_back(numRule);

    // Strings and comments
    stringPattern = QRegularExpression(R"("([^"\\]|\\.)*")");
    commentPattern = QRegularExpression(R"(;.*$)");

    // Label definitions: label: at start or after whitespace
    labelDefPattern = QRegularExpression(R"(^\s*([A-Za-z_][A-Za-z0-9_\.]*)\:)");
    // LBL directive: LBL name
    lblDirectivePattern = QRegularExpression(R"((?i)\bLBL\s+([A-Za-z_][A-Za-z0-9_\.]*)\b)");

    // Include directive pattern (whole line helpful)
    includeDirectivePattern = QRegularExpression(R"(^\s*#include\s*[<"].*[>"])");
}

void MicroHighlighter::highlightBlock(const QString &text) {
    // Strings
    auto it = stringPattern.globalMatch(text);
    while (it.hasNext()) {
        auto m = it.next();
        QTextCharFormat fmt; fmt.setForeground(QColor(255, 0, 0)); // Bright red
        setFormat(m.capturedStart(), m.capturedLength(), fmt);
    }

    // Comments
    auto c = commentPattern.match(text);
    if (c.hasMatch()) {
        QTextCharFormat fmt; fmt.setForeground(QColor(255, 255, 0)); // Yellow
        setFormat(c.capturedStart(), text.length() - c.capturedStart(), fmt);
        // Early return? No, still allow label coloring before comment
    }

    // Rules (instructions, registers, directives, numbers)
    for (const Rule &r : rules) {
        auto it2 = r.pattern.globalMatch(text);
        while (it2.hasNext()) {
            auto m = it2.next();
            setFormat(m.capturedStart(), m.capturedLength(), r.format);
        }
    }

    // Label definitions
    auto ld = labelDefPattern.match(text);
    if (ld.hasMatch()) {
        QTextCharFormat fmt; fmt.setForeground(QColor(255, 165, 0)); fmt.setFontWeight(QFont::Bold); // Orange
        setFormat(ld.capturedStart(1), ld.capturedLength(1), fmt);
    }

    // LBL directive label name
    auto ldl = lblDirectivePattern.match(text);
    if (ldl.hasMatch()) {
        QTextCharFormat fmt; fmt.setForeground(QColor(255, 165, 0)); fmt.setFontWeight(QFont::Bold); // Orange
        setFormat(ldl.capturedStart(1), ldl.capturedLength(1), fmt);
    }

    // Include directive line tint
    auto inc = includeDirectivePattern.match(text);
    if (inc.hasMatch()) {
        QTextCharFormat fmt; fmt.setForeground(QColor(0, 255, 0)); // Bright green
        setFormat(inc.capturedStart(), inc.capturedLength(), fmt);
    }
}
