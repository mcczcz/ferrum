#include "I18n.h"
#include "mod/Global.h"
#include <filesystem>
#include <fmt/std.h>
#include <fstream>
#include <nlohmann/json.hpp>
#include <vector>

namespace ferrum {

I18n& I18n::getInstance() {
    static I18n instance;
    return instance;
}

bool I18n::load(const std::filesystem::path& langDir, const std::string& defaultLang) {
    mDefaultLang = defaultLang;
    mCurrentLang = defaultLang;

    namespace fs = std::filesystem;
    if (!fs::exists(langDir)) {
        fs::create_directories(langDir);
        return false;
    }

    std::vector<std::string> loadedLanguages;
    for (const auto& entry : fs::directory_iterator(langDir)) {
        if (entry.path().extension() == ".json") {
            std::string lang = fmt::format("{}", entry.path().stem());
            if (loadLanguageFile(entry.path(), lang)) {
                loadedLanguages.push_back(lang);
            }
        }
    }
    for (const auto& lang : loadedLanguages) {
        logger.info(get("i18n.loaded", lang));
    }
    return !loadedLanguages.empty();
}

bool I18n::loadLanguageFile(const std::filesystem::path& path, const std::string& lang) {
    std::ifstream file(path);
    if (!file.is_open()) return false;

    try {
        nlohmann::json json;
        file >> json;

        for (const auto& [key, value] : json.items()) {
            if (value.is_string()) {
                mTranslations[lang][key] = value.get<std::string>();
            }
        }
        return true;
    } catch (const std::exception& e) {
        logger.error(get("i18n.load_error", path, e.what()));
        return false;
    }
}

void I18n::setLanguage(const std::string& lang) {
    if (mTranslations.contains(lang)) {
        mCurrentLang = lang;
    } else {
        logger.warn(get("i18n.lang_not_found", lang, mDefaultLang));
        mCurrentLang = mDefaultLang;
    }
}

const std::string& I18n::getLanguage() const { return mCurrentLang; }

std::string_view I18n::get(std::string_view key) const {

    if (auto langIt = mTranslations.find(mCurrentLang); langIt != mTranslations.end()) {
        if (auto keyIt = langIt->second.find(key); keyIt != langIt->second.end()) {
            return keyIt->second;
        }
    }
    if (mCurrentLang != mDefaultLang) {
        if (auto langIt = mTranslations.find(mDefaultLang); langIt != mTranslations.end()) {
            if (auto keyIt = langIt->second.find(key); keyIt != langIt->second.end()) {
                return keyIt->second;
            }
        }
    }
    return key;
}

} // namespace ferrum
