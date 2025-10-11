#pragma once
#include <QDialog>
class QTextEdit;

class DumpDialog : public QDialog {
    Q_OBJECT
public:
    explicit DumpDialog(const QString &text, QWidget *parent = nullptr);
    ~DumpDialog();
private slots:
    void exportToFile();
private:
    QTextEdit *m_edit{nullptr};
};
