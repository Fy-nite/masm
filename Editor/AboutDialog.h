#pragma once
#include <QDialog>
#include <QLabel>
#include <QVBoxLayout>

class AboutDialog : public QDialog {
public:
    AboutDialog(QWidget *parent = nullptr) : QDialog(parent) {
        setWindowTitle("About MasmEditor");
        QVBoxLayout *l = new QVBoxLayout(this);
        QLabel *t = new QLabel("MasmEditor\nVersion: dev\nSimple MicroASM editor.", this);
        t->setAlignment(Qt::AlignCenter);
        l->addWidget(t);
        setLayout(l);
    }
};
