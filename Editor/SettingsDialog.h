#pragma once
#include <QDialog>
#include <QVBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>

class SettingsDialog : public QDialog {
public:
    SettingsDialog(QWidget *parent = nullptr) : QDialog(parent) {
        setWindowTitle("Settings");
        QVBoxLayout *l = new QVBoxLayout(this);
        l->addWidget(new QLabel("MASM executable path:", this));
        pathEdit = new QLineEdit(this);
        l->addWidget(pathEdit);
        QPushButton *ok = new QPushButton("OK", this);
        connect(ok, &QPushButton::clicked, this, &SettingsDialog::accept);
        l->addWidget(ok);
        setLayout(l);
    }
    QString masmPath() const { return pathEdit->text(); }
    void setMasmPath(const QString &p) { pathEdit->setText(p); }
private:
    QLineEdit *pathEdit{nullptr};
};
