#include "SymbolsViewer.h"
#include <QRegularExpression>
#include <QPushButton>
#include <QDateTime>
#include <QCheckBox>
#include <QTextStream>
#include "DumpDialog.h"

// Internal representation for symbols
struct SymbolEntry { QString name; QString type; int line; };

SymbolsViewer::SymbolsViewer(QWidget *parent)
    : QWidget(parent) {
    m_search = new QLineEdit(this);
    m_search->setPlaceholderText("Search symbols...");
    m_typeFilter = new QComboBox(this);
    m_typeFilter->addItems({"All", "Label", "Data", "State", "LBL", "Macro", "Include"});
    m_tree = new QTreeWidget(this);
    m_tree->setHeaderLabels({"Symbol", "Type"});
    m_tree->setRootIsDecorated(false);

    QVBoxLayout *layout = new QVBoxLayout(this);
    layout->setContentsMargins(4,4,4,4);
    layout->addWidget(m_search);
    layout->addWidget(m_typeFilter);
    m_groupByType = new QCheckBox("Group by type", this);
    m_groupByType->setChecked(true);
    layout->addWidget(m_groupByType);
    layout->addWidget(m_tree);
    // Add a small print button for debugging
    auto *printBtn = new QPushButton("Print", this);
    layout->addWidget(printBtn);
    connect(printBtn, &QPushButton::clicked, this, &SymbolsViewer::printContents);
    setLayout(layout);

    connect(m_search, &QLineEdit::textChanged, this, &SymbolsViewer::applyFilter);
    connect(m_typeFilter, qOverload<int>(&QComboBox::currentIndexChanged), this, &SymbolsViewer::filterByType);
    connect(m_groupByType, &QCheckBox::toggled, this, &SymbolsViewer::rebuildTree);

    connect(m_tree, &QTreeWidget::itemDoubleClicked, this, [this](QTreeWidgetItem *item, int) {
        QString type = item->text(1);
        // If this is an Include entry, try to open the referenced file in the editor
        if (type == "Include") {
            QString fname = item->text(0);
            emit includeRequested(fname);
            return;
        }
        bool ok = false;
        int line = item->data(0, Qt::UserRole).toInt(&ok);
        if (ok) emit symbolActivated(line);
    });
}

void SymbolsViewer::printContents() {
    QString dump;
    QTextStream ss(&dump);
    ss << "--- SymbolsViewer verbose dump ---\n";
    ss << " timestamp:" << QDateTime::currentDateTime().toString(Qt::ISODate) << " generation:" << m_generation << "\n";
    ss << " m_tree:" << (quintptr)m_tree << " topLevelCount:" << m_tree->topLevelItemCount() << "\n";
    for (int i = 0; i < m_tree->topLevelItemCount(); ++i) {
        auto *group = m_tree->topLevelItem(i);
        QString gname = group->text(0);
        ss << " Group idx=" << i << " ptr=" << (quintptr)group << " name=" << gname << " childCount=" << group->childCount() << "\n";
        for (int j = 0; j < group->childCount(); ++j) {
            auto *item = group->child(j);
            QString name = item->text(0);
            QString type = item->text(1);
            int line = item->data(0, Qt::UserRole).toInt();
            ss << "   childIdx=" << j << " itemPtr=" << (quintptr)item << " name=" << name << " type=" << type << " line=" << line << "\n";
            if (!m_lastLines.isEmpty()) {
                if (line >= 0 && line < m_lastLines.size()) ss << "      source> " << m_lastLines[line] << "\n";
                else ss << "      source> (no source line for index)\n";
            }
        }
    }
    ss << "--- end ---\n";
    // show modal dialog with dump
    DumpDialog dlg(dump, this);
    dlg.exec();
}

void SymbolsViewer::applyFilter(const QString &filterText) {
    m_lastText = filterText;
    rebuildTree();
}

void SymbolsViewer::filterByType(int) {
    rebuildTree();
}

void SymbolsViewer::rebuildTree() {
    // m_lastText and m_typeFilter determine what to show; updateSymbols already stores last parsed text in m_lastText
    updateSymbols(m_lastText);
}

void SymbolsViewer::updateSymbols(const QString &text) {
    m_lastText = text;
    m_tree->clear();

    // store last lines for verbose printing and bump generation
    m_lastLines = text.split('\n');
    ++m_generation;

    QRegularExpression labelDef(R"(^\s*([A-Za-z_][A-Za-z0-9_\.]*):)");
    QRegularExpression dataDef(R"(^\s*([A-Za-z_][A-Za-z0-9_\.]*)\s+(DB|DW|DD|DQ|DF|DDbl|RESB|RESW|RESD|RESQ|RESF|RESDbl))", QRegularExpression::CaseInsensitiveOption);
    QRegularExpression stateDef(R"(^\s*STATE\s+([A-Za-z_][A-Za-z0-9_\.]*)\b)", QRegularExpression::CaseInsensitiveOption);
    QRegularExpression lblDef(R"((?i)\bLBL\s+([A-Za-z_][A-Za-z0-9_\.]*)\b)");
    QRegularExpression macroDef(R"((?i)^\s*MACRO\s+([A-Za-z_][A-Za-z0-9_\.]*)\b)");
    QRegularExpression includeDef(R"((?i)^\s*INCLUDE\s+\"?([^\"\s]+)\"?)");

    QStringList lines = text.split('\n');

    // Collect all raw matches per name, then pick the best entry per name.
    struct Found { QString type; int line; QString display; };
    struct BestInfo { QString type; int line; QString display; };
    QMap<QString, QVector<Found>> allMatches; // key = lowercase name -> list of matches
    auto priority = [](const QString &t){
        if (t == "LBL") return 4;
        if (t == "Label") return 3;
        if (t == "Data") return 2;
        if (t == "Macro") return 2;
        if (t == "Include") return 1;
        if (t == "State") return 0;
        return 0;
    };

    for (int i = 0; i < lines.size(); ++i) {
        const QString &line = lines[i];
        QSet<QString> seenThisLine;
        auto labelMatch = labelDef.match(line);
        if (labelMatch.hasMatch()) {
            QString name = labelMatch.captured(1).trimmed();
            if (seenThisLine.contains(name)) { continue; }
            seenThisLine.insert(name);
            if (name.length() < 3) { continue; }
            QString key = name.toLower();
            QString t = "Label";
            allMatches[key].append({t, i, name});
        }
        auto dataMatch = dataDef.match(line);
        if (dataMatch.hasMatch()) {
            QString name = dataMatch.captured(1).trimmed();
            if (seenThisLine.contains(name)) { continue; }
            seenThisLine.insert(name);
            if (name.length() < 3) { continue; }
            QString key = name.toLower();
            QString t = "Data";
            allMatches[key].append({t, i, name});
        }
        auto stateMatch = stateDef.match(line);
        if (stateMatch.hasMatch()) {
            QString name = stateMatch.captured(1).trimmed();
            if (seenThisLine.contains(name)) { continue; }
            seenThisLine.insert(name);
            if (name.length() < 3) { continue; }
            QString key = name.toLower();
            QString t = "State";
            allMatches[key].append({t, i, name});
        }
        auto lblMatch = lblDef.match(line);
        if (lblMatch.hasMatch()) {
            QString name = lblMatch.captured(1).trimmed();
            if (seenThisLine.contains(name)) { continue; }
            seenThisLine.insert(name);
            if (name.length() < 3) { continue; }
            QString key = name.toLower();
            QString t = "LBL";
            allMatches[key].append({t, i, name});
        }
        auto macroMatch = macroDef.match(line);
        if (macroMatch.hasMatch()) {
            QString name = macroMatch.captured(1).trimmed();
            if (seenThisLine.contains(name)) { continue; }
            seenThisLine.insert(name);
            if (name.length() < 1) { continue; }
            QString key = name.toLower();
            QString t = "Macro";
            allMatches[key].append({t, i, name});
        }
        auto includeMatch = includeDef.match(line);
        if (includeMatch.hasMatch()) {
            QString inc = includeMatch.captured(1).trimmed();
            // use the filename as display/name
            QString display = inc;
            QString key = display.toLower();
            // includes may repeat; allow them but avoid too-short names
            if (display.length() < 1) continue;
            QString t = "Include";
            allMatches[key].append({t, i, display});
        }
    }
    QString filter = m_search->text();
    QString type = m_typeFilter->currentText();

    // Select best entry per name, with nearby-LBL override.
    QMap<QString, BestInfo> bestByName;
    const int NEARBY_LINES = 3;
    for (auto it = allMatches.constBegin(); it != allMatches.constEnd(); ++it) {
        const QString key = it.key();
        const QVector<Found> list = it.value();
        // find candidate with highest priority; if tie pick earliest line
        Found best = list[0];
        for (const Found &f : list) {
            if (priority(f.type) > priority(best.type)) best = f;
            else if (priority(f.type) == priority(best.type) && f.line < best.line) best = f;
        }
        // if best is not LBL but there's an LBL within NEARBY_LINES, prefer the nearest LBL
        if (best.type != "LBL") {
            int chosenLblIdx = -1;
            int bestLblDist = INT_MAX;
            for (int k = 0; k < list.size(); ++k) {
                const Found &f = list[k];
                if (f.type == "LBL") {
                    int d = qAbs(f.line - best.line);
                    if (d <= NEARBY_LINES && d < bestLblDist) { bestLblDist = d; chosenLblIdx = k; }
                }
            }
            if (chosenLblIdx != -1) best = list[chosenLblIdx];
        }
        bestByName.insert(key, {best.type, best.line, best.display});
    }

    // Aggressive suppression: if an LBL exists for a name, prefer it globally and remove other types
    for (auto it = bestByName.begin(); it != bestByName.end(); ++it) {
        const QString &key = it.key();
        // look into allMatches for any LBL
        const QVector<Found> &lst = allMatches.value(key);
        bool hasLbl = false;
        int lblLine = INT_MAX;
        for (const Found &f : lst) if (f.type == "LBL") { hasLbl = true; lblLine = qMin(lblLine, f.line); }
        if (hasLbl) {
            // replace with earliest LBL
            for (const Found &f : lst) if (f.type == "LBL" && f.line == lblLine) {
                it.value() = {QString("LBL"), f.line, f.display};
                break;
            }
        }
    }

    // Deterministic grouping and sorted insertion.
    QMap<QString, QVector<BestInfo>> buckets;
    for (auto it = bestByName.constBegin(); it != bestByName.constEnd(); ++it) {
        QString name = it.value().display;
        QString t = it.value().type;
        int line = it.value().line;
        if (!filter.isEmpty() && !name.contains(filter, Qt::CaseInsensitive)) continue;
        if (type != "All" && t != type) continue;
        buckets[t].append({t, line, name});
    }

    // If grouping is disabled produce a flat list (sorted by line)
    if (!m_groupByType->isChecked()) {
        QVector<BestInfo> all;
        for (auto it = buckets.constBegin(); it != buckets.constEnd(); ++it) all += it.value();
        std::sort(all.begin(), all.end(), [](const BestInfo &a, const BestInfo &b){
            if (a.line != b.line) return a.line < b.line;
            return a.display.toLower() < b.display.toLower();
        });
        for (const BestInfo &bi : all) {
            auto *item = new QTreeWidgetItem(m_tree, QStringList{bi.display, bi.type});
            item->setData(0, Qt::UserRole, bi.line);
        }
        // no groups; just show flat list
        return;
    }

    // Define the desired group order
    QStringList groupOrder = {"LBL", "Label", "Data", "Macro", "Include", "State"};
    for (const QString &gname : groupOrder) {
        if (!buckets.contains(gname)) continue;
        auto vec = buckets[gname];
        // sort by line ascending, then name
        std::sort(vec.begin(), vec.end(), [](const BestInfo &a, const BestInfo &b){
            if (a.line != b.line) return a.line < b.line;
            return a.display.toLower() < b.display.toLower();
        });
        auto *groupItem = new QTreeWidgetItem(m_tree, QStringList{gname});
        groupItem->setFirstColumnSpanned(true);
        for (const BestInfo &bi : vec) {
            auto *item = new QTreeWidgetItem(groupItem, QStringList{bi.display, bi.type});
            item->setData(0, Qt::UserRole, bi.line);
        }
        groupItem->setExpanded(true);
    }
}
