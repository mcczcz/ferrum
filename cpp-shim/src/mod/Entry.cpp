#include "mod/Entry.h"

#include "Config/ConfigManager.h"
#include "I18n/I18n.h"
#include "ferrum/FerrumModManager.h"
#include "ferrum/ferrum_abi.h"

#include "ll/api/mod/ModManagerRegistry.h"
#include "ll/api/mod/RegisterHelper.h"

#include <filesystem>
#include <memory>

namespace ferrum {
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

    if (auto res = initHost(); !res) {
        logger.error(tr("loader.host_init_failed"));
        res.error().log(logger);
        return false;
    }

    // 必须在 load 阶段注册：依赖本模组的 type = "ferrum" 模组紧随其后加载。
    mManager = std::make_shared<FerrumModManager>();
    if (!ll::mod::ModManagerRegistry::getInstance().addManager(mManager)) {
        logger.error(tr("loader.manager_register_failed"));
        return false;
    }

    auto abi = ferrum_host_abi_version();
    logger.info(tr("loader.loaded", abi.major, abi.minor, abi.patch));
    return true;
}

bool Entry::enable() {
    getSelf().getLogger().debug("Enabling...");
    // enable 在服务端线程执行，此处记录主线程以供 MainThreadToken 校验。
    ferrum_host_mark_main_thread();
    logger.info(tr("mod.enabled"));
    return true;
}

bool Entry::disable() {
    getSelf().getLogger().debug("Disabling...");
    logger.info(tr("mod.disabled"));
    return true;
}

LL_REGISTER_MOD(ferrum::Entry, ferrum::Entry::getInstance());
} // namespace ferrum
