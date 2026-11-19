#include "ferrum/DomainApi.h"
#include "ll/api/mod/ModManagerRegistry.h"
#include "ll/api/utils/ErrorUtils.h"
#include <windows.h>

#include <string>
#include <utility>
#include <vector>

namespace ferrum {
static std::string moneyString(FfiStr value) {
    return value.len ? std::string{reinterpret_cast<char const*>(value.ptr), value.len} : std::string{};
}


template <class Function, class F>
static FerrumStatus withMoney(char const* symbol, F&& invoke) noexcept {
    try {
        auto mod = ll::mod::ModManagerRegistry::getInstance().getMod("LegacyMoney");
        if (!mod || !mod->isEnabled() || mod->getType() != ll::mod::NativeModManagerName)
            return FerrumStatus_SERVICE_UNAVAILABLE;
        auto native = std::static_pointer_cast<ll::mod::NativeMod>(mod);
        auto handle = static_cast<HMODULE>(native->getHandle());
        auto fn     = reinterpret_cast<Function*>(GetProcAddress(handle, symbol));
        if (!fn) return FerrumStatus_UNSUPPORTED;
        return invoke(fn);
    } catch (...) {
        auto text = ll::error_utils::makeExceptionString(std::current_exception());
        ferrum_host_set_last_error({reinterpret_cast<uint8_t const*>(text.data()), text.size()});
        return FerrumStatus_CPP_EXCEPTION;
    }
}

static FerrumStatus getBalance(FfiStr xuid, int64_t* out) noexcept {
    if (!out) return FerrumStatus_NULL_POINTER;
    return withMoney<long long(std::string)>("LLMoney_Get", [&](auto get) {
        *out = get(moneyString(xuid));
        return FerrumStatus_OK;
    });
}

static FerrumStatus changeBalance(char const* symbol, FfiStr xuid, int64_t amount) noexcept {
    return withMoney<bool(std::string, long long)>(symbol, [&](auto change) {
        return change(moneyString(xuid), amount) ? FerrumStatus_OK : FerrumStatus_FAILED;
    });
}

static FerrumStatus setBalance(FfiStr xuid, int64_t amount) noexcept {
    return changeBalance("LLMoney_Set", xuid, amount);
}

static FerrumStatus addBalance(FfiStr xuid, int64_t amount) noexcept {
    return changeBalance("LLMoney_Add", xuid, amount);
}

static FerrumStatus reduceBalance(FfiStr xuid, int64_t amount) noexcept {
    return changeBalance("LLMoney_Reduce", xuid, amount);
}

static FerrumStatus transfer(FfiStr from, FfiStr to, int64_t amount, FfiStr note) noexcept {
    using Transfer = bool(std::string, std::string, long long, std::string const&);
    return withMoney<Transfer>("LLMoney_Trans", [&](auto trans) {
        return trans(moneyString(from), moneyString(to), amount, moneyString(note)) ? FerrumStatus_OK
                                                                                    : FerrumStatus_FAILED;
    });
}

static FerrumStatus history(FfiStr xuid, int32_t seconds, FfiByteWriter* out) noexcept {
    if (!out) return FerrumStatus_NULL_POINTER;
    return withMoney<std::string(std::string, int)>("LLMoney_GetHist", [&](auto getHist) {
        auto text = getHist(moneyString(xuid), seconds);
        return out->write_fn(out->context, reinterpret_cast<uint8_t const*>(text.data()), text.size());
    });
}

static FerrumStatus clearHistory(int32_t seconds) noexcept {
    return withMoney<void(int)>("LLMoney_ClearHist", [&](auto clear) {
        clear(seconds);
        return FerrumStatus_OK;
    });
}

static FerrumStatus ranking(uint16_t limit, FfiByteWriter* out) noexcept {
    if (!out) return FerrumStatus_NULL_POINTER;
    using Ranking = std::vector<std::pair<std::string, long long>>(unsigned short);
    // 此函数在 LLMoney.h 的 extern "C" 块之外，使用 Windows x64 的实际导出名。
    static constexpr char symbol[] =
        "?LLMoney_Ranking@@YA?AV?$vector@U?$pair@V?$basic_string@DU?$char_traits@D@std@@V?$allocator@D@2@@std@@"
        "_J@std@@V?$allocator@U?$pair@V?$basic_string@DU?$char_traits@D@std@@V?$allocator@D@2@@std@@_J@std@@@2@@std@@G@"
        "Z";
    return withMoney<Ranking>(symbol, [&](auto rank) {
        for (auto const& [xuid, balance] : rank(limit)) {
            uint64_t length = xuid.size();
            auto     write  = [&](void const* data, size_t size) {
                return out->write_fn(out->context, static_cast<uint8_t const*>(data), size);
            };
            if (auto status = write(&balance, sizeof(balance)); status != FerrumStatus_OK) return status;
            if (auto status = write(&length, sizeof(length)); status != FerrumStatus_OK) return status;
            if (auto status = write(xuid.data(), xuid.size()); status != FerrumStatus_OK) return status;
        }
        return FerrumStatus_OK;
    });
}

LlMoneyApiV1 const* llMoneyApi() noexcept {
    static constexpr LlMoneyApiV1 api{
        sizeof(LlMoneyApiV1),
        getBalance,
        setBalance,
        addBalance,
        reduceBalance,
        transfer,
        history,
        clearHistory,
        ranking
    };
    return &api;
}
} // namespace ferrum
