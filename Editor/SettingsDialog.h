#pragma once
#include <QDialog>

class QLineEdit;
class QComboBox;
class QCheckBox;

class SettingsDialog : public QDialog {
    Q_OBJECT
public:
    explicit SettingsDialog(QWidget *parent = nullptr);
    ~SettingsDialog();

    QString masmPath() const;
    void setMasmPath(const QString &p);

    QString theme() const;
    void setTheme(const QString &t);

    bool completionsEnabled() const;
    void setCompletionsEnabled(bool v);

    // load/save helpers
    void loadSettings();
    void saveSettings();

private:
    QLineEdit *m_pathEdit{nullptr};
    QComboBox *m_theme{nullptr};
    QCheckBox *m_completions{nullptr};
};
