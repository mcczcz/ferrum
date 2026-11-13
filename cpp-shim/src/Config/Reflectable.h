#pragma once

#include <type_traits>

namespace ferrum {

template <typename T>
struct is_reflectable : std::false_type {};

} // namespace ferrum
