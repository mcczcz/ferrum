#pragma once

#include "Reflectable.h"
#include "ll/api/io/LogLevel.h"
#include <string>

namespace my_mod {

struct Config {
    int              version                  = 1;
    ll::io::LogLevel logLevel                 = ll::io::LogLevel::Info;
    std::string      language                 = "zh_CN";
};

template <>
struct is_reflectable<Config> : std::true_type {};

} // namespace my_mod
