#pragma once
#include <QString>
#include <QRegularExpression>

class Formatter {
public:
    // Very small formatter: normalize spaces, ensure consistent spacing around operands
    static QString format(const QString &text) {
        QStringList lines = text.split('\n');
        QRegularExpression multiWs(R"([\t ]+)");
        for (int i = 0; i < lines.size(); ++i) {
            QString s = lines[i].trimmed();
            if (s.isEmpty()) { lines[i].clear(); continue; }
            // compress multiple spaces/tabs to single space
            s.replace(multiWs, " ");
            // normalize comma spacing
            s.replace(", ", ",");
            s.replace(",", ", ");
            lines[i] = s;
        }
        return lines.join('\n');
    }
};
