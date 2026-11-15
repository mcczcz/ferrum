#pragma once
#include "ferrum_abi.h"

// 使用 GetProcAddress 查询此入口，无需链接 ferrum-loader.lib。
// 函数表及生成的 C ABI 类型不依赖第三方模组的 C++ 对象布局。
extern "C" NativeEventApiV1 const* ferrum_native_event_api_v1() noexcept;
using FerrumGetNativeEventApiV1 = NativeEventApiV1 const* (*)() noexcept;
