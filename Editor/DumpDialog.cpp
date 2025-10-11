#include "DumpDialog.h"
#include <QVBoxLayout>
#include <QTextEdit>
#include <QPushButton>
#include <QFileDialog>
#include <QFile>
#include <QIODevice>

DumpDialog::DumpDialog(const QString &text, QWidget *parent) : QDialog(parent) {
    setWindowTitle("Symbols Dump");
    QVBoxLayout *l = new QVBoxLayout(this);
    m_edit = new QTextEdit(this);
    m_edit->setReadOnly(true);
    m_edit->setPlainText(text);
    l->addWidget(m_edit);
    QHBoxLayout *h = new QHBoxLayout();
    QPushButton *exportBtn = new QPushButton("Export…", this);
    QPushButton *close = new QPushButton("Close", this);
    h->addStretch(1);
    h->addWidget(exportBtn);
    h->addWidget(close);
    l->addLayout(h);
    connect(close, &QPushButton::clicked, this, &DumpDialog::accept);
    connect(exportBtn, &QPushButton::clicked, this, &DumpDialog::exportToFile);
    setLayout(l);
    resize(600, 400);
}

DumpDialog::~DumpDialog() {}

void DumpDialog::exportToFile() {
    QString file = QFileDialog::getSaveFileName(this, "Export Dump", "symbols_dump.txt", "Text Files (*.txt);;All Files (*.*)");
    if (file.isEmpty()) return;
    QFile f(file);
    if (f.open(QIODevice::WriteOnly | QIODevice::Text)) {
        f.write(m_edit->toPlainText().toUtf8());
        f.close();
    }
}
