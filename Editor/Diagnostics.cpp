#include "Diagnostics.h"
#include <QProcess>
#include <QSettings>
#include <QFileInfo>
#include <QDebug>
#include <QCoreApplication>
#include <QDir>
#include <QRegularExpression>

Diagnostics::Diagnostics(QObject *parent) : QObject(parent) {}

void Diagnostics::run(const QString &filePath) {
    if (m_proc) {
        m_proc->kill();
        m_proc->deleteLater();
    }
    m_targetFile = filePath;

    QSettings s("masm", "MasmEditor");
    QString masmExe = s.value("masm/path", "").toString();
    if (masmExe.isEmpty()) {
        QString exeDir = QCoreApplication::applicationDirPath();
        QString candidate = exeDir + QDir::separator() + "tools" + QDir::separator() + "masm";
        if (QFile::exists(candidate) || QFile::exists(candidate + ".exe")) masmExe = candidate;
    }
    if (masmExe.isEmpty()) masmExe = QStringLiteral("masm");

    m_proc = new QProcess(this);
    connect(m_proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this, &Diagnostics::onProcessFinished);
    m_proc->start(masmExe, QStringList() << filePath);
}

void Diagnostics::onProcessFinished(int /*exitCode*/, QProcess::ExitStatus /*status*/) {
    if (!m_proc) return;
    QString out = m_proc->readAllStandardOutput() + m_proc->readAllStandardError();
    QVector<Diagnostic> diags = parseOutput(out);
    emit diagnosticsReady(m_targetFile, diags);
    m_proc->deleteLater();
    m_proc = nullptr;
}

QVector<Diagnostic> Diagnostics::parseOutput(const QString &output) {
    QVector<Diagnostic> res;
    // Very small parser: look for lines like "file:line:col: Error: message" or "file(line): Error message"
    QRegularExpression r1(R"(([\w\./\\:-]+)[:(](\d+)[\):]?(?::(\d+))?\s*[:]?\s*(Error|Warning)[:]?\s*(.*))", QRegularExpression::CaseInsensitiveOption);
    auto it = r1.globalMatch(output);
    while (it.hasNext()) {
        auto m = it.next();
        Diagnostic d;
        d.file = m.captured(1);
        d.line = m.captured(2).toInt() - 1;
        if (!m.captured(3).isEmpty()) d.column = m.captured(3).toInt() - 1;
        d.severity = m.captured(4);
        d.message = m.captured(5).trimmed();
        res.append(d);
    }
    // If nothing matched, optionally include the whole output as an informational diagnostic
    if (res.isEmpty() && !output.trimmed().isEmpty()) {
        Diagnostic d;
        d.file = m_targetFile;
        d.line = -1;
        d.severity = "Info";
        d.message = output.trimmed();
        res.append(d);
    }
    return res;
}

// Lightweight in-IDE analyzer. Does not invoke external masm; instead applies
// a few heuristic checks on the source text to provide fast feedback.
QVector<Diagnostic> Diagnostics::analyzeText(const QString &text, const QString &basePath) {
    QVector<Diagnostic> res;
    QStringList lines = text.split('\n');

    // collect labels and detect duplicates
    QHash<QString, int> labelFirstLine;
    QHash<QString, int> labelCount;
    QRegularExpression labelRx(R"(^\s*([A-Za-z_][A-Za-z0-9_.]*)\s*:\s*$)", QRegularExpression::CaseInsensitiveOption);
    QRegularExpression labelWithRestRx(R"(^\s*([A-Za-z_][A-Za-z0-9_.]*)\s*:\s*(.*)$)", QRegularExpression::CaseInsensitiveOption);
    QRegularExpression instrRx(R"(^\s*([A-Za-z_][A-Za-z0-9_.]*)\b)", QRegularExpression::CaseInsensitiveOption);
    QSet<QString> knownInstr;
    // small set of known instructions/directives for heuristics
    knownInstr.unite(QSet<QString>({"mov","add","sub","mul","div","jmp","je","jne","call","ret","push","pop","db","dw","dd","include","state","lbl","macro","endm","out"}));

    // build a canonical register name set based on masm register_map.rs
    QSet<QString> knownRegs;
    QStringList regs = {"rax","rbx","rcx","rdx","rsi","rdi","rbp","rsp","rip"};
    for (int i = 0; i <= 15; ++i) regs << QString("r%1").arg(i);
    for (int i = 0; i <= 15; ++i) regs << QString("fpr%1").arg(i);
    regs << QStringList({"zf","sf","of"});
    for (const QString &r : regs) knownRegs.insert(r.toLower());

    for (int i = 0; i < lines.size(); ++i) {
        const QString &ln = lines.at(i);
        // If a label is defined on the same line as an instruction, record the label
        // so the name isn't treated as an unknown instruction, and process the rest
        // of the line for instruction checks.
        auto lw = labelWithRestRx.match(ln);
        if (lw.hasMatch()) {
            QString name = lw.captured(1).toLower();
            labelCount[name] = labelCount.value(name, 0) + 1;
            if (!labelFirstLine.contains(name)) labelFirstLine[name] = i;
        }
        auto m = labelRx.match(ln);
        if (m.hasMatch()) {
            QString name = m.captured(1).toLower();
            labelCount[name] = labelCount.value(name, 0) + 1;
            if (!labelFirstLine.contains(name)) labelFirstLine[name] = i;
        }
    }

    // duplicates
    for (auto it = labelCount.constBegin(); it != labelCount.constEnd(); ++it) {
        if (it.value() > 1) {
            Diagnostic d;
            d.file = basePath;
            d.line = labelFirstLine.value(it.key(), 0);
            d.severity = "Warning";
            d.message = QString("Duplicate label '%1' defined (%2 times)").arg(it.key()).arg(it.value());
            res.append(d);
        }
    }

    // collect label usages and unknown instructions
    QSet<QString> definedLabels;
    for (const QString &k : labelFirstLine.keys()) definedLabels.insert(k);
    for (int i = 0; i < lines.size(); ++i) {
        const QString &ln = lines.at(i);
        // include check
        if (ln.trimmed().startsWith("#include", Qt::CaseInsensitive)) {
            QRegularExpression incRx(R"(#include\s+["<]([^">]+)[">])", QRegularExpression::CaseInsensitiveOption);
            auto mm = incRx.match(ln);
            if (mm.hasMatch()) {
                QString path = mm.captured(1);
                QString resolved = path;
                if (!QFileInfo(resolved).isAbsolute() && !basePath.isEmpty()) {
                    resolved = QFileInfo(basePath).absolutePath() + QDir::separator() + path;
                }
                if (!QFile::exists(resolved)) {
                    Diagnostic d;
                    d.file = basePath;
                    d.line = i;
                    d.severity = "Warning";
                    d.message = QString("Included file not found: %1").arg(path);
                    res.append(d);
                }
            }
        }

        // simple instruction unknown check
        // if label: instr ... then use the remainder after the colon
        QString content = ln;
        auto mlabel = labelWithRestRx.match(ln);
        if (mlabel.hasMatch()) content = mlabel.captured(2);
        auto mi = instrRx.match(content);
        if (mi.hasMatch()) {
            QString tok = mi.captured(1).toLower();
            // if it's a label (line ends with :), skip
            if (!content.trimmed().endsWith(":")) {
                if (!knownInstr.contains(tok) && !definedLabels.contains(tok) && !tok.startsWith(";") && !tok.startsWith("//")) {
                    // heuristics: if token contains non-alpha maybe it's a label usage or operand
                    if (tok.indexOf(QRegularExpression("[^a-z0-9_.]")) == -1) {
                        Diagnostic d;
                        d.file = basePath;
                        d.line = i;
                        d.severity = "Warning";
                        d.message = QString("Unknown instruction or symbol: %1").arg(mi.captured(1));
                        res.append(d);
                    }
                }
            }
        }

        // operand parsing helpers: support comma-separated or space-separated operands,
        // ignoring commas/spaces inside brackets or quotes.
        auto splitOperands = [](const QString &s) {
            QString text = s.trimmed();
            QStringList parts;
            if (text.isEmpty()) return parts;
            // prefer comma splitting if present
            if (text.contains(',')) {
                int start = 0;
                bool inBracket = false;
                bool inQuote = false;
                for (int i = 0; i < text.size(); ++i) {
                    QChar c = text[i];
                    if (c == '"') inQuote = !inQuote;
                    if (!inQuote) {
                        if (c == '[') inBracket = true;
                        if (c == ']') inBracket = false;
                        if (c == ',' && !inBracket) {
                            parts << text.mid(start, i - start).trimmed();
                            start = i + 1;
                        }
                    }
                }
                parts << text.mid(start).trimmed();
                return parts;
            }
            // otherwise split on whitespace, but keep bracketed expressions and quoted strings intact
            QString cur;
            bool inBracket = false;
            bool inQuote = false;
            for (int i = 0; i < text.size(); ++i) {
                QChar c = text[i];
                if (c == '"') { inQuote = !inQuote; cur.append(c); continue; }
                if (!inQuote) {
                    if (c == '[') { inBracket = true; cur.append(c); continue; }
                    if (c == ']') { inBracket = false; cur.append(c); continue; }
                    if (c.isSpace() && !inBracket) {
                        if (!cur.trimmed().isEmpty()) { parts << cur.trimmed(); cur.clear(); }
                        continue;
                    }
                }
                cur.append(c);
            }
            if (!cur.trimmed().isEmpty()) parts << cur.trimmed();
            return parts;
        };

        // operand-count and register-name checks for common instructions
        QRegularExpression instrLineRx(R"(^\s*([A-Za-z_][A-Za-z0-9_.]*)\s+(.*))", QRegularExpression::CaseInsensitiveOption);
        for (int i = 0; i < lines.size(); ++i) {
            auto m = instrLineRx.match(lines.at(i));
            if (!m.hasMatch()) continue;
            QString op = m.captured(1).toLower();
            QString rest = m.captured(2).trimmed();

            // only check a few ops
            if (op == "mov" || op == "add" || op == "sub") {
                QStringList ops = splitOperands(rest);
                if (ops.size() != 2) {
                    Diagnostic d; d.file = basePath; d.line = i; d.severity = "Warning";
                    d.message = QString("%1 expects 2 operands, got %2").arg(op).arg(ops.size()); res.append(d); continue;
                }
                // check destination semantics: destination must not be an immediate literal
                QString dest = ops.first().trimmed().toLower();
                // strip brackets
                if (dest.startsWith("[") && dest.endsWith("]")) {
                    // memory destination is allowed
                } else {
                    // immediate patterns: numeric decimal, hex (0x), or quoted string
                    bool isImmediate = false;
                    if (dest.startsWith('"') || dest.startsWith('\'')) isImmediate = true;
                    if (QRegularExpression(R"(^0x[0-9a-f]+$)", QRegularExpression::CaseInsensitiveOption).match(dest).hasMatch()) isImmediate = true;
                    if (QRegularExpression(R"(^[0-9]+$)").match(dest).hasMatch()) isImmediate = true;
                    if (isImmediate) {
                        Diagnostic d; d.file = basePath; d.line = i; d.severity = "Error";
                        d.message = QString("Destination operand for '%1' cannot be an immediate literal: %2").arg(op).arg(ops.first());
                        res.append(d);
                    }
                }
                // check register names if operand looks like a register
                for (const QString &o : ops) {
                    QString low = o.toLower();
                    // strip memory brackets
                    QString stripped = low;
                    if (stripped.startsWith("[") && stripped.endsWith("]")) stripped = stripped.mid(1, stripped.size()-2).trimmed();
                    // if looks like plain register (no spaces, alpha+digits), validate
                    if (QRegularExpression(R"(^[a-z0-9]+$)").match(stripped).hasMatch()) {
                        if (!knownRegs.contains(stripped)) {
                            Diagnostic d; d.file = basePath; d.line = i; d.severity = "Warning";
                            d.message = QString("Unknown register: %1").arg(o);
                            res.append(d);
                        }
                    }
                }
            } else if (op == "push" || op == "pop") {
                QStringList ops = splitOperands(rest);
                if (ops.size() != 1) {
                    Diagnostic d; d.file = basePath; d.line = i; d.severity = "Warning";
                    d.message = QString("%1 expects 1 operand, got %2").arg(op).arg(ops.size()); res.append(d); continue;
                }
                QString o = ops.first().toLower();
                if (QRegularExpression(R"(^[a-z0-9]+$)").match(o).hasMatch() && !knownRegs.contains(o)) {
                    Diagnostic d; d.file = basePath; d.line = i; d.severity = "Warning";
                    d.message = QString("Unknown register: %1").arg(o);
                    res.append(d);
                }
            } else if (op == "call" || op == "jmp" || op == "je" || op == "jne") {
                QStringList ops = splitOperands(rest);
                if (ops.size() < 1) continue;
                QString target = ops.first().toLower();
                // if target is register, ensure register valid; otherwise if label, will be checked by undefined-label check
                if (QRegularExpression(R"(^[a-z0-9]+$)").match(target).hasMatch() && knownRegs.contains(target)) {
                    // ok
                } else if (QRegularExpression(R"(^[a-z_][a-z0-9_.]*$)", QRegularExpression::CaseInsensitiveOption).match(target).hasMatch()) {
                    // label-like; undefined labels already emitted earlier
                } else if (target.startsWith("[") && target.endsWith("]")) {
                    // memory-target: inside may contain register
                    QString inside = target.mid(1, target.size()-2).trimmed().toLower();
                    if (QRegularExpression(R"(^[a-z0-9]+$)").match(inside).hasMatch() && !knownRegs.contains(inside)) {
                        Diagnostic d; d.file = basePath; d.line = i; d.severity = "Warning";
                        d.message = QString("Unknown register in memory operand: %1").arg(inside);
                        res.append(d);
                    }
                }
            }
        }
    }

    // undefined labels: look for uses like 'jmp label' where label not defined
    QRegularExpression jumpRx(R"((?:\b|\s)(jmp|je|jne|jg|jl|call)\s+([A-Za-z_][A-Za-z0-9_.]*)\b)", QRegularExpression::CaseInsensitiveOption);
    for (int i = 0; i < lines.size(); ++i) {
        auto m = jumpRx.globalMatch(lines.at(i));
        while (m.hasNext()) {
            auto mm = m.next();
            QString label = mm.captured(2).toLower();
            if (!definedLabels.contains(label)) {
                Diagnostic d;
                d.file = basePath;
                d.line = i;
                d.severity = "Error";
                d.message = QString("Undefined label: %1").arg(label);
                res.append(d);
            }
        }
    }

    return res;
}

void Diagnostics::analyzeTextAndEmit(const QString &text, const QString &basePath) {
    QVector<Diagnostic> diags = analyzeText(text, basePath);
    emit diagnosticsReady(basePath.isEmpty() ? QString() : basePath, diags);
}
