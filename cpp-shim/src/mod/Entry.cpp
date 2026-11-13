#include "mod/Entry.h"

#include "Config/ConfigManager.h"
#include "I18n/I18n.h"
#include "ll/api/mod/RegisterHelper.h"
#include <filesystem>

namespace my_mod {
ll::io::Logger& logger = Entry::getInstance().getSelf().getLogger();
Config&         config = ConfigManager::getInstance().get();

Entry& Entry::getInstance() {
    static Entry instance;
    return instance;
}

bool Entry::load() {
    getSelf().getLogger().debug("Loading...");
    auto langPath = getSelf().getLangDir();
    if (!I18n::getInstance().load(langPath, "zh_CN")) {
        logger.warn("语言文件未加载，将使用键名作为回退文本。");
    }

    auto configPath = getSelf().getConfigDir();
    if (!std::filesystem::exists(configPath)) {
        std::filesystem::create_directories(configPath);
    }
    configPath /= "config.jsonc";
    configPath.make_preferred();

    // 首次启动使用随模组发布的推荐配置，保留作者填写的注释。
    const auto defaultConfigPath = getSelf().getModDir() / "config.jsonc";
    if (!std::filesystem::exists(configPath) && std::filesystem::exists(defaultConfigPath)) {
        std::filesystem::copy_file(defaultConfigPath, configPath);
    }

    if (!ConfigManager::getInstance().load(configPath)) {
        getSelf().getLogger().error(tr("config.load_error"));
        return false;
    }
    I18n::getInstance().setLanguage(config.language);

    logger.info(tr("mod.loaded"));
    return true;
}

bool Entry::enable() {
    getSelf().getLogger().debug("Enabling...");

    logger.info(tr("mod.enabled"));
    return true;
}

bool Entry::disable() {
    getSelf().getLogger().debug("Disabling...");
    logger.info(tr("mod.unloaded"));
    return true;
}

bool Entry::unload() {
    getSelf().getLogger().debug("Unloading...");
    return true;
}

LL_REGISTER_MOD(my_mod::Entry, my_mod::Entry::getInstance());
} // namespace my_mod
