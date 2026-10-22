#include <QCoreApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTextStream>
#include "settings/INIFile.h"

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
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
