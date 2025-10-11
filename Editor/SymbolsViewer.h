#pragma once
#include <QWidget>
#include <QTreeWidget>
#include <QLineEdit>
#include <QComboBox>
#include <QCheckBox>
#include <QVBoxLayout>
#include <QString>
#include <QStringList>

class SymbolsViewer : public QWidget {
    Q_OBJECT
public:
    SymbolsViewer(QWidget *parent = nullptr);
    void updateSymbols(const QString &text);
    Q_SLOT void printContents();
    void setGroupByType(bool v) { if (m_groupByType) m_groupByType->setChecked(v); }
private slots:
    void applyFilter(const QString &filterText);
    void filterByType(int index);
signals:
    void symbolActivated(int line);
private:
    QLineEdit *m_search{nullptr};
    QComboBox *m_typeFilter{nullptr};
    QTreeWidget *m_tree{nullptr};
    QCheckBox *m_groupByType{nullptr};
    QString m_lastText;
    QStringList m_lastLines;
    int m_generation{0};
    void rebuildTree();
};
