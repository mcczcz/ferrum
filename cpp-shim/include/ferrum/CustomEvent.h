#pragma once
#include "ferrum_abi.h"
#include "ll/api/event/Cancellable.h"
#include <string>
#include <string_view>
#include <vector>

namespace ferrum {
// 只在同步 publish 内借用，禁止保存事件对象或 scope 跨越回调。
class FerrumCustomEvent final : public ll::event::Cancellable<ll::event::Event> {
    using Base = ll::event::Cancellable<ll::event::Event>;
    CustomEventViewApiV1 const* mApi;
    EventScopeHandle            mScope;
    using Base::setCancelled;
    static FfiStr ffi(std::string_view text) { return {reinterpret_cast<uint8_t const*>(text.data()), text.size()}; }
    static FerrumStatus writeString(void* env, uint8_t const* data, size_t len) noexcept {
        try {
            if (len) static_cast<std::string*>(env)->append(reinterpret_cast<char const*>(data), len);
            return FerrumStatus_OK;
        } catch (...) {
            return FerrumStatus_CPP_EXCEPTION;
        }
    }
    static FerrumStatus writeBytes(void* env, uint8_t const* data, size_t len) noexcept {
        try {
            auto& out = *static_cast<std::vector<uint8_t>*>(env);
            if (len) out.insert(out.end(), data, data + len);
            return FerrumStatus_OK;
        } catch (...) {
            return FerrumStatus_CPP_EXCEPTION;
        }
    }

public:
    static constexpr ll::event::EventIdView CustomEventId{"ferrum.custom_event.v1"};
    explicit FerrumCustomEvent(CustomEventViewApiV1 const* api, EventScopeHandle scope) : mApi(api), mScope(scope) {}
    FerrumCustomEvent(FerrumCustomEvent const&)            = delete;
    FerrumCustomEvent& operator=(FerrumCustomEvent const&) = delete;
    FerrumStatus       name(std::string& out) const {
        out.clear();
        FfiByteWriter writer{&out, writeString};
        return mApi->name(mScope, &writer);
    }
    FerrumStatus getI64(std::string_view key, int64_t& out) const { return mApi->get_i64(mScope, ffi(key), &out); }
    FerrumStatus getU64(std::string_view key, uint64_t& out) const { return mApi->get_u64(mScope, ffi(key), &out); }
    FerrumStatus getF64(std::string_view key, double& out) const { return mApi->get_f64(mScope, ffi(key), &out); }
    FerrumStatus getBool(std::string_view key, FfiBool& out) const { return mApi->get_bool(mScope, ffi(key), &out); }
    FerrumStatus getUuid(std::string_view key, PlayerUuid& out) const { return mApi->get_uuid(mScope, ffi(key), &out); }
    FerrumStatus getString(std::string_view key, std::string& out) const {
        out.clear();
        FfiByteWriter writer{&out, writeString};
        return mApi->get_string(mScope, ffi(key), &writer);
    }
    FerrumStatus getBytes(std::string_view key, std::vector<uint8_t>& out) const {
        out.clear();
        FfiByteWriter writer{&out, writeBytes};
        return mApi->get_bytes(mScope, ffi(key), &writer);
    }
    FerrumStatus cancel() {
        auto status = mApi->cancel(mScope);
        if (status == FerrumStatus_OK) Base::cancel();
        return status;
    }
    bool active() const { return mApi->is_active(mScope) == FerrumStatus_OK; }
    void syncCancelled() {
        FfiBool cancelled = FfiBool_FALSE;
        if (Base::isCancelled()) (void)mApi->cancel(mScope);
        if (mApi->is_cancelled(mScope, &cancelled) == FerrumStatus_OK && cancelled) Base::cancel();
    }
    EventScopeHandle scope() const { return mScope; }
};
} // namespace ferrum
