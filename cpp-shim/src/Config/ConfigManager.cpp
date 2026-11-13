#include "ConfigManager.h"
#include "ConfigSerialization.h"
#include "I18n/I18n.h"
#include "mod/Global.h"
#include <Windows.h>
#include <exception>
#include <filesystem>
#include <fmt/std.h>
#include <fstream>
#include <functional>
#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

namespace ferrum {

namespace {

struct RootObjectRange {
    size_t openBrace;
    size_t closeBrace;
};

std::optional<RootObjectRange> findRootObjectRange(std::string_view text) {
    enum class ParseState { Normal, String, LineComment, BlockComment };

    ParseState state   = ParseState::Normal;
    bool       escaped = false;
    int        depth   = 0;
    size_t     openPos = std::string_view::npos;

    for (size_t i = 0; i < text.size(); ++i) {
        const char ch   = text[i];
        const char next = i + 1 < text.size() ? text[i + 1] : '\0';

        switch (state) {
        case ParseState::Normal:
            if (ch == '"') {
                state   = ParseState::String;
                escaped = false;
                continue;
            }
            if (ch == '/' && next == '/') {
                state = ParseState::LineComment;
                ++i;
                continue;
            }
            if (ch == '/' && next == '*') {
                state = ParseState::BlockComment;
                ++i;
                continue;
            }
            if (ch == '{') {
                if (depth == 0) {
                    openPos = i;
                }
                ++depth;
                continue;
            }
            if (ch == '}') {
                if (depth <= 0) {
                    return std::nullopt;
                }
                --depth;
                if (depth == 0) {
                    if (openPos == std::string_view::npos) {
                        return std::nullopt;
                    }
                    return RootObjectRange{openPos, i};
                }
            }
            break;
        case ParseState::String:
            if (escaped) {
                escaped = false;
                continue;
            }
            if (ch == '\\') {
                escaped = true;
                continue;
            }
            if (ch == '"') {
                state = ParseState::Normal;
            }
            break;
        case ParseState::LineComment:
            if (ch == '\n') {
                state = ParseState::Normal;
            }
            break;
        case ParseState::BlockComment:
            if (ch == '*' && next == '/') {
                state = ParseState::Normal;
                ++i;
            }
            break;
        }
    }
    return std::nullopt;
}

std::string detectLineEnding(std::string_view text) {
    return text.find("\r\n") != std::string_view::npos ? "\r\n" : "\n";
}

std::string detectIndent(std::string_view text, size_t openBracePos, size_t closeBracePos) {
    size_t lineStart = openBracePos + 1;
    while (lineStart < closeBracePos) {
        size_t lineEnd = text.find('\n', lineStart);
        if (lineEnd == std::string_view::npos || lineEnd > closeBracePos) {
            lineEnd = closeBracePos;
        }

        auto   line = text.substr(lineStart, lineEnd - lineStart);
        size_t i    = 0;
        while (i < line.size() && (line[i] == ' ' || line[i] == '\t')) {
            ++i;
        }
        if (i < line.size() && line[i] == '"') {
            return std::string(line.substr(0, i));
        }

        if (lineEnd == closeBracePos) {
            break;
        }
        lineStart = lineEnd + 1;
    }
    return "    ";
}

bool appendMissingTopLevelKeysPreserveComments(
    std::filesystem::path const&                        path,
    std::vector<std::pair<std::string, nlohmann::json>> missingEntries,
    bool                                                hasExistingMembers
) {
    if (missingEntries.empty()) {
        return true;
    }

    std::ifstream in(path, std::ios::binary);
    if (!in.is_open()) {
        return false;
    }
    std::string content((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    in.close();

    auto range = findRootObjectRange(content);
    if (!range.has_value()) {
        return false;
    }

    const std::string lineEnding = detectLineEnding(content);
    const std::string indent     = detectIndent(content, range->openBrace, range->closeBrace);

    std::string patch;
    if (hasExistingMembers) {
        patch += ",";
        patch += lineEnding;
    } else {
        patch += lineEnding;
    }

    for (size_t i = 0; i < missingEntries.size(); ++i) {
        auto& [key, value]  = missingEntries[i];
        patch              += indent;
        patch              += "\"";
        patch              += key;
        patch              += "\": ";
        patch              += value.dump();
        if (i + 1 < missingEntries.size()) {
            patch += ",";
        }
        patch += lineEnding;
    }

    content.insert(range->closeBrace, patch);

    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    if (!out.is_open()) {
        return false;
    }
    out.write(content.data(), static_cast<std::streamsize>(content.size()));
    return out.good();
}

} // namespace

ConfigManager& ConfigManager::getInstance() {
    static ConfigManager instance;
    return instance;
}

ConfigManager::ConfigManager() : mConfig(std::make_unique<Config>()) {}

ConfigManager::~ConfigManager() = default;

bool ConfigManager::load(const std::filesystem::path& path) {
    mConfigPath = path;
    std::ifstream file(path);
    if (!file.is_open()) {
        // 文件不存在，创建默认配置
        logger.info(tr("config.not_found", path));
        if (!save()) {
            return false;
        }
        logger.setLevel(mConfig->logLevel);
        return true;
    }

    std::string content((std::istreambuf_iterator<char>(file)), std::istreambuf_iterator<char>());
    file.close();

    nlohmann::json user_json;
    try {
        // 解析用户配置，允许注释
        user_json = nlohmann::json::parse(content, nullptr, true, true);
        // 填充内存中的配置对象。缺失的键将从结构体定义中获取默认值。
        *mConfig = user_json.get<Config>();
    } catch (const nlohmann::json::exception& e) {
        logger.error(tr("config.parse_error", e.what()));
        return false;
    }

    // 检查缺失的键以警告用户。
    nlohmann::json default_json = Config{}; // 创建一个默认配置对象并转换为json

    // 递归函数查找缺失的键
    std::function<void(const nlohmann::json&, const nlohmann::json&, std::string, std::vector<std::string>&)>
        find_missing_keys =
            [&](const nlohmann::json& d, const nlohmann::json& u, std::string p, std::vector<std::string>& m) {
                for (const auto& item : d.items()) {
                    if (!u.contains(item.key())) {
                        m.push_back(p + item.key());
                    } else if (item.value().is_object() && u.at(item.key()).is_object()) {
                        find_missing_keys(item.value(), u.at(item.key()), p + item.key() + ".", m);
                    }
                }
            };

    std::vector<std::string> missing_keys;
    find_missing_keys(default_json, user_json, "", missing_keys);

    if (!missing_keys.empty()) {
        logger.warn(tr("config.missing_keys"));
        logger.warn(tr("config.missing_keys_hint"));
        for (const auto& key : missing_keys) {
            logger.warn(tr("config.missing_key_item", key));
        }
        logger.warn(tr("config.regenerate_hint"));

        // 仅补齐顶层缺失键，按文本补丁方式写回，保留原注释和已有项顺序。
        std::vector<std::pair<std::string, nlohmann::json>> missing_top_level_entries;
        for (const auto& [key, value] : default_json.items()) {
            if (!user_json.contains(key)) {
                missing_top_level_entries.emplace_back(key, value);
            }
        }
        if (!missing_top_level_entries.empty()) {
            if (appendMissingTopLevelKeysPreserveComments(path, missing_top_level_entries, !user_json.empty())) {
                logger.info(tr("config.updated_missing_keys", missing_top_level_entries.size()));
            } else {
                logger.warn(tr("config.update_missing_keys_failed"));
            }
        }
    }
    logger.setLevel(mConfig->logLevel);

    return true;
}

bool ConfigManager::save() {
    try {
        const std::string content = nlohmann::json(*mConfig).dump(4);
        auto              tempPath = mConfigPath;
        tempPath += ".tmp";

        std::ofstream file;
        file.exceptions(std::ios::failbit | std::ios::badbit);
        file.open(tempPath, std::ios::binary | std::ios::trunc);
        file.write(content.data(), static_cast<std::streamsize>(content.size()));
        file.close();

        // 临时文件写入成功后再替换原配置，支持覆盖已有文件。
        if (!MoveFileExW(tempPath.c_str(), mConfigPath.c_str(), MOVEFILE_REPLACE_EXISTING)) {
            throw std::system_error(static_cast<int>(GetLastError()), std::system_category(), "replace config");
        }
    } catch (const std::exception& e) {
        logger.error(tr("config.save_error", e.what()));
        return false;
    }
    return true;
}


Config& ConfigManager::get() { return *mConfig; }

const Config& ConfigManager::get() const { return *mConfig; }

} // namespace ferrum
