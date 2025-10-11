#include "Indexer.h"
#include <QRegularExpression>

IndexResult Indexer::parseText(const QString &text) {
    IndexResult result;
    QRegularExpression labelDef(R"(^\s*([A-Za-z_][A-Za-z0-9_\.]*)\:)");
    QRegularExpression lblDef(R"((?i)\bLBL\s+([A-Za-z_][A-Za-z0-9_\.]*)\b)");

    QStringList lines = text.split('\n');
    for (int i = 0; i < lines.size(); ++i) {
        const QString &line = lines[i];
        auto ld = labelDef.match(line);
        if (ld.hasMatch()) result.definitions.insert(ld.captured(1), i);
        auto ldl = lblDef.match(line);
        if (ldl.hasMatch()) result.definitions.insert(ldl.captured(1), i);

        // Find usages (simple word-based)
        QRegularExpression word(R"([A-Za-z_][A-Za-z0-9_\.]*)");
        auto it = word.globalMatch(line);
        while (it.hasNext()) {
            auto m = it.next();
            QString w = m.captured(0);
            result.usages.insert(w, i);
        }
    }

    return result;
}
