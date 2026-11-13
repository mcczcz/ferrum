#pragma once

#include "ferrum/ferrum_abi.h"

#include "ll/api/Expected.h"
#include "ll/api/mod/Manifest.h"
#include "ll/api/mod/Mod.h"
#include "ll/api/mod/ModManager.h"

#include <cstdint>
#include <memory>
#include <string_view>

namespace ferrum {

inline constexpr std::string_view FerrumModType = "ferrum";

// 每个 type = "ferrum" 的 Rust 模组在 LeviLamina 中对应的 Mod 实例。
class FerrumMod : public ll::mod::Mod {
    friend class FerrumModManager;

public:
    explicit FerrumMod(ll::mod::Manifest manifest) : Mod(std::move(manifest)) {}

    // ferrum-host 中的 ModContextHandle（0 表示尚未加载成功）。
    [[nodiscard]] uint64_t getContext() const noexcept { return mContext; }

private:
    uint64_t mContext = 0;
};

// 注册到 ModManagerRegistry 的 "ferrum" 类型管理器，把生命周期转发给 ferrum-host。
class FerrumModManager final : public ll::mod::ModManager {
public:
    FerrumModManager();
    ~FerrumModManager() override;

    // 熔断等路径请求在主线程停用某模组（任意线程调用）。
    static void requestDisable(FerrumMod& mod);

protected:
    ll::Expected<> load(ll::mod::Manifest manifest) override;
    ll::Expected<> unload(std::string_view name) override;
};

// 安装 shim 函数表并初始化 ferrum-host。
ll::Expected<> initHost();

// 取回 ferrum-host 当前线程的 last-error 并组装为 LeviLamina 错误。
ll::Unexpected hostError(FerrumStatus status);

} // namespace ferrum
