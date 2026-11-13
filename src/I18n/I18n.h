#pragma once

#include <filesystem>
#include <fmt/format.h>
#include <functional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <utility>


namespace my_mod {

class I18n {
public:
    static I18n& getInstance();

    bool load(const std::filesystem::path& langDir, const std::string& defaultLang = "zh_CN");

    void setLanguage(const std::string& lang);

    const std::string& getLanguage() const;

    std::string_view get(std::string_view key) const;

    template <typename... Args>
    std::string get(std::string_view key, Args&&... args) const {
        const auto text = get(key);
        try {
            return fmt::format(fmt::runtime(text), std::forward<Args>(args)...);
        } catch (const fmt::format_error&) {
            return std::string(text);
        }
    }

private:
    I18n()                       = default;
    ~I18n()                      = default;
    I18n(const I18n&)            = delete;
    I18n& operator=(const I18n&) = delete;

    bool loadLanguageFile(const std::filesystem::path& path, const std::string& lang);

    struct StringHash {
        using is_transparent = void;

        std::size_t operator()(std::string_view value) const noexcept { return std::hash<std::string_view>{}(value); }
    };

    using TranslationMap = std::unordered_map<std::string, std::string, StringHash, std::equal_to<>>;

    std::string                                     mCurrentLang;
    std::string                                     mDefaultLang;
    std::unordered_map<std::string, TranslationMap> mTranslations;
};

// 便捷函数
inline std::string_view tr(std::string_view key) { return I18n::getInstance().get(key); }

template <typename... Args>
inline std::string tr(std::string_view key, Args&&... args) {
    return I18n::getInstance().get(key, std::forward<Args>(args)...);
}

} // namespace my_mod
