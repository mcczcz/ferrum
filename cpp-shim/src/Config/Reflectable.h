#pragma once

#include <type_traits>

namespace my_mod {

template <typename T>
struct is_reflectable : std::false_type {};

} // namespace my_mod
