#include "ferrum/FerrumModManager.h"
#include "ferrum/DomainApi.h"

#include "mod/Entry.h"

#include "ll/api/io/Logger.h"
#include "ll/api/io/LogLevel.h"
#include "ll/api/mod/ModManagerRegistry.h"
#include "ll/api/thread/ServerThreadExecutor.h"
#include "ll/api/utils/ErrorUtils.h"
#include "ll/api/utils/StringUtils.h"

#include <filesystem>
#include <memory>
#include <string>
#include <string_view>
#include <utility>

namespace ferrum {

// ---------------------------------------------------------------------------
// FfiStr / FfiByteWriter 辅助
// ---------------------------------------------------------------------------

static FfiStr toFfi(std::string_view sv) noexcept {
    return FfiStr{reinterpret_cast<uint8_t const*>(sv.data()), sv.size()};
}

static std::string_view fromFfi(FfiStr s) noexcept {
    // 空视图的 ptr 可能为 nullptr，len == 0 时 string_view 天然合法。
    return s.len == 0 ? std::string_view{} : std::string_view{reinterpret_cast<char const*>(s.ptr), s.len};
}

static FerrumStatus stringWrite(void* context, uint8_t const* bytes, size_t len) noexcept {
    try {
        static_cast<std::string*>(context)->append(reinterpret_cast<char const*>(bytes), len);
        return FerrumStatus_OK;
    } catch (...) {
        return FerrumStatus_OUT_OF_MEMORY;
    }
}

static std::string pathToUtf8(std::filesystem::path const& p) {
    return ll::string_utils::u8str2str(p.u8string());
}

static ll::io::LogLevel toLlLevel(LogLevel level) noexcept {
    switch (level) {
    case LogLevel_FATAL:
        return ll::io::LogLevel::Fatal;
    case LogLevel_ERROR:
        return ll::io::LogLevel::Error;
    case LogLevel_WARN:
        return ll::io::LogLevel::Warn;
    case LogLevel_DEBUG:
        return ll::io::LogLevel::Debug;
    case LogLevel_TRACE:
        return ll::io::LogLevel::Trace;
    default:
        return ll::io::LogLevel::Info;
    }
}

ll::Unexpected hostError(FerrumStatus status) {
    std::string   detail;
    FfiByteWriter writer{&detail, stringWrite};
    (void)ferrum_host_take_last_error(&writer);
    std::string message = "ferrum-host status " + std::to_string(status);
    if (!detail.empty()) {
        message += ": " + detail;
    }
    return ll::makeStringError(std::move(message));
}

// ---------------------------------------------------------------------------
// Cookie：宿主持有的 FerrumMod 强引用
// ---------------------------------------------------------------------------
//
// cookie 是堆上的 std::shared_ptr<FerrumMod>。ferrum-host 拥有它，直到最后一个 ModEntry
// 释放时通过 release_cookie 归还。这样即使模组已从 ModManager 擦除，残留线程写日志也不会悬垂。

static ShimModCookie makeCookie(std::shared_ptr<FerrumMod> const& mod) {
    return new std::shared_ptr<FerrumMod>(mod);
}

static FerrumMod* cookieMod(ShimModCookie cookie) noexcept {
    auto* holder = static_cast<std::shared_ptr<FerrumMod>*>(cookie);
    return holder ? holder->get() : nullptr;
}

// ---------------------------------------------------------------------------
// FerrumShimApi 实现（全部 noexcept）
// ---------------------------------------------------------------------------

static void shimLog(ShimModCookie cookie, LogLevel level, FfiStr message) noexcept {
    try {
        if (auto* mod = cookieMod(cookie)) {
            mod->getLogger().log(toLlLevel(level), std::string{fromFfi(message)});
        }
    } catch (...) {}
}

static FfiBool shimShouldLog(ShimModCookie cookie, LogLevel level) noexcept {
    try {
        if (auto* mod = cookieMod(cookie)) {
            return mod->getLogger().shouldLog(toLlLevel(level)) ? FfiBool_TRUE : FfiBool_FALSE;
        }
    } catch (...) {}
    return FfiBool_FALSE;
}

static void shimLogLoader(LogLevel level, FfiStr message) noexcept {
    try {
        Entry::getInstance().getSelf().getLogger().log(toLlLevel(level), std::string{fromFfi(message)});
    } catch (...) {}
}

static void shimRequestDisable(ShimModCookie cookie) noexcept {
    try {
        if (auto* mod = cookieMod(cookie)) {
            FerrumModManager::requestDisable(*mod);
        }
    } catch (...) {}
}

static void shimReleaseCookie(ShimModCookie cookie) noexcept {
    delete static_cast<std::shared_ptr<FerrumMod>*>(cookie);
}

static FerrumShimApi const shimApi{
    sizeof(FerrumShimApi),
    shimLog,
    shimShouldLog,
    shimLogLoader,
    shimRequestDisable,
    shimReleaseCookie,
};

ll::Expected<> initHost() {
    auto api = shimApi;
    installDomainApi(api);
    if (auto status = ferrum_host_init(&api); status != FerrumStatus_OK) {
        return hostError(status);
    }
    return {};
}

// ---------------------------------------------------------------------------
// FerrumModManager
// ---------------------------------------------------------------------------

FerrumModManager::FerrumModManager() : ModManager(FerrumModType) {}

FerrumModManager::~FerrumModManager() = default;

void FerrumModManager::requestDisable(FerrumMod& mod) {
    std::string name = mod.getName();
    ll::thread::ServerThreadExecutor::getDefault().execute([name = std::move(name)] {
        auto manager = std::static_pointer_cast<FerrumModManager>(
            ll::mod::ModManagerRegistry::getInstance().getManager(FerrumModType)
        );
        if (!manager) {
            return;
        }
        auto target = manager->getMod(name);
        if (!target || !target->isEnabled()) {
            return;
        }
        // 走 ModManager::disable，使 LeviLamina 侧的 Mod 状态与 ferrum-host 保持一致。
        if (auto res = manager->disable(name); !res) {
            res.error().log(target->getLogger());
        }
    });
}

ll::Expected<> FerrumModManager::load(ll::mod::Manifest manifest) {
    auto lock = this->lock();
    auto mod  = std::make_shared<FerrumMod>(std::move(manifest));

    auto const& m          = mod->getManifest();
    std::string version    = m.version ? m.version->to_string() : "0.0.0";
    std::string modDir     = pathToUtf8(mod->getModDir());
    std::string entryPath  = pathToUtf8(mod->getModDir() / ll::string_utils::sv2u8sv(m.entry));
    std::string dataDir    = pathToUtf8(mod->getDataDir());
    std::string configDir  = pathToUtf8(mod->getConfigDir());
    std::string langDir    = pathToUtf8(mod->getLangDir());

    ShimModInfo info{
        sizeof(ShimModInfo),
        makeCookie(mod),
        toFfi(mod->getName()),
        toFfi(version),
        toFfi(entryPath),
        toFfi(modDir),
        toFfi(dataDir),
        toFfi(configDir),
        toFfi(langDir),
    };

    // 生命周期回调把调用转发给 ferrum-host。按值捕获裸指针：Mod 持有回调，捕获 shared_ptr 会成环。
    FerrumMod* self = mod.get();
    mod->onEnable([self](ll::mod::Mod&) {
        ferrum_host_mark_main_thread();
        if (auto status = ferrum_host_enable_mod(self->getContext()); status != FerrumStatus_OK) {
            hostError(status).error().log(self->getLogger());
            return false;
        }
        return true;
    });
    mod->onDisable([self](ll::mod::Mod&) {
        if (auto status = ferrum_host_disable_mod(self->getContext()); status != FerrumStatus_OK) {
            hostError(status).error().log(self->getLogger());
            return false;
        }
        return true;
    });
    mod->onUnload([self](ll::mod::Mod&) {
        if (auto status = ferrum_host_unload_mod(self->getContext()); status != FerrumStatus_OK) {
            hostError(status).error().log(self->getLogger());
            return false;
        }
        return true;
    });

    // cookie 的所有权从这里起转交 ferrum-host（成功或失败都由 release_cookie 归还）。
    uint64_t context = 0;
    if (auto status = ferrum_host_load_mod(&info, &context); status != FerrumStatus_OK) {
        return hostError(status);
    }
    mod->mContext = context;

    // Mod::onLoad 不再调用模组代码（ferrum-host 已执行 on_load），但会通知 LeviLamina 的 onModLoad
    // 监听者，并在服务器已运行时自动串联 onEnable。
    addMod(mod->getName(), mod);
    if (auto res = mod->onLoad(); !res) {
        (void)ferrum_host_unload_mod(context);
        eraseMod(mod->getName());
        return res;
    }
    return {};
}

ll::Expected<> FerrumModManager::unload(std::string_view name) {
    auto lock = this->lock();
    auto mod  = std::static_pointer_cast<FerrumMod>(getMod(name));
    if (!mod) {
        return ll::makeStringError("Mod " + std::string{name} + " not found");
    }
    // Mod::onUnload 先停用，再调用我们注册的 onUnload 回调。
    if (auto res = mod->onUnload(); !res) {
        return res;
    }
    // 镜像保留：不 FreeLibrary。
    eraseMod(name);
    return {};
}

} // namespace ferrum
