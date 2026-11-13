#pragma once

#include "Config.h"
#include <filesystem>
#include <memory>


namespace my_mod {

class ConfigManager {
public:
    static ConfigManager& getInstance();
    bool                  load(const std::filesystem::path& path);
    bool                  save();
    Config&               get();
    const Config&         get() const;

private:
    ConfigManager();
    ~ConfigManager();

    ConfigManager(const ConfigManager&)            = delete;
    ConfigManager& operator=(const ConfigManager&) = delete;
    ConfigManager(ConfigManager&&)                 = delete;
    ConfigManager& operator=(ConfigManager&&)      = delete;

    std::unique_ptr<Config> mConfig;
    std::filesystem::path   mConfigPath;
};

} // namespace my_mod
