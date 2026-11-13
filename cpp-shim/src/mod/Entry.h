#pragma once

#include "ll/api/mod/NativeMod.h"

#include <memory>

namespace ferrum {

class FerrumModManager;

// ferrum-loader 自身（type = "native"）的入口。
class Entry {

public:
    static Entry& getInstance();

    Entry() : mSelf(*ll::mod::NativeMod::current()) {}

    [[nodiscard]] ll::mod::NativeMod& getSelf() const { return mSelf; }

    bool load();

    bool enable();

    bool disable();

    // 不提供 unload：下游 Rust 模组的镜像与回调依赖本 DLL 常驻，ferrum-loader 不支持被卸载。

private:
    ll::mod::NativeMod&               mSelf;
    std::shared_ptr<FerrumModManager> mManager;
};

} // namespace ferrum
