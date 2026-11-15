#pragma once
#include "ferrum/ferrum_abi.h"

namespace ferrum {
FerrumStatus installNativeEvents() noexcept;
void         closeNativeEvents() noexcept;
void         installCustomEventBridge(FerrumShimApi& api);
} // namespace ferrum
