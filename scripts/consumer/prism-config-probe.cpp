#include <QCoreApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTextStream>
#include <QSysInfo>
#include "settings/INIFile.h"

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    if (app.arguments().at(1) == "--environment") {
        QJsonObject environment{
            {"qt_compiled", QT_VERSION_STR},
            {"qt_runtime", qVersion()},
            {"target_architecture", QSysInfo::buildCpuArchitecture()},
            {"runtime_architecture", QSysInfo::currentCpuArchitecture()},
            {"compiler", __VERSION__},
            {"cplusplus", static_cast<double>(__cplusplus)},
        };
        QTextStream(stdout) << QJsonDocument(environment).toJson(QJsonDocument::Compact) << '\n';
        return 0;
    }
    INIFile ini;
    bool loaded = ini.loadFile(app.arguments().at(1));
    QJsonObject result{
        {"loaded", loaded},
        {"version", ini.get("ConfigVersion", {}).toString()},
        {"override", ini.get("OverrideCommands", false).toBool()},
        {"prepare", ini.get("PreLaunchCommand", {}).toString()},
        {"wrapper", ini.get("WrapperCommand", {}).toString()},
    };
    QTextStream(stdout) << QJsonDocument(result).toJson(QJsonDocument::Compact) << '\n';
}
