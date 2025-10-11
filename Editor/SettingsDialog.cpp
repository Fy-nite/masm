#include "SettingsDialog.h"
#include <QVBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QComboBox>
#include <QCheckBox>
#include <QSettings>

SettingsDialog::SettingsDialog(QWidget *parent) : QDialog(parent) {
    setWindowTitle("Settings");
    QVBoxLayout *l = new QVBoxLayout(this);
    l->addWidget(new QLabel("MASM executable path:", this));
    m_pathEdit = new QLineEdit(this);
    l->addWidget(m_pathEdit);

    l->addWidget(new QLabel("Theme:", this));
    m_theme = new QComboBox(this);
    m_theme->addItem("System");
    m_theme->addItem("Light");
    m_theme->addItem("Dark");
    l->addWidget(m_theme);

    m_completions = new QCheckBox("Enable completions", this);
    l->addWidget(m_completions);

    QHBoxLayout *h = new QHBoxLayout();
    QPushButton *ok = new QPushButton("OK", this);
    QPushButton *cancel = new QPushButton("Cancel", this);
    h->addStretch(1);
    h->addWidget(cancel);
    h->addWidget(ok);
    l->addLayout(h);

    connect(ok, &QPushButton::clicked, this, [this]() { saveSettings(); accept(); });
    connect(cancel, &QPushButton::clicked, this, [this]() { loadSettings(); reject(); });

    setLayout(l);
    loadSettings();
}

SettingsDialog::~SettingsDialog() {}

QString SettingsDialog::masmPath() const { return m_pathEdit->text(); }
void SettingsDialog::setMasmPath(const QString &p) { m_pathEdit->setText(p); }

QString SettingsDialog::theme() const { return m_theme->currentText(); }
void SettingsDialog::setTheme(const QString &t) {
    int idx = m_theme->findText(t);
    if (idx >= 0) m_theme->setCurrentIndex(idx);
}

bool SettingsDialog::completionsEnabled() const { return m_completions->isChecked(); }
void SettingsDialog::setCompletionsEnabled(bool v) { m_completions->setChecked(v); }

void SettingsDialog::loadSettings() {
    QSettings s("masm", "MasmEditor");
    m_pathEdit->setText(s.value("masm/path", "").toString());
    setTheme(s.value("ui/theme", "System").toString());
    setCompletionsEnabled(s.value("editor/completions", true).toBool());
}

void SettingsDialog::saveSettings() {
    QSettings s("masm", "MasmEditor");
    s.setValue("masm/path", masmPath());
    s.setValue("ui/theme", theme());
    s.setValue("editor/completions", completionsEnabled());
}
