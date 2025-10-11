#pragma once
#include <QObject>
#include <QString>
#include <QMap>
#include <QMultiMap>

struct IndexResult {
    QMap<QString,int> definitions;
    QMultiMap<QString,int> usages;
};

namespace Indexer {
    IndexResult parseText(const QString &text);
}
