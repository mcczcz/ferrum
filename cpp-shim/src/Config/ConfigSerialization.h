#pragma once

// clang-cl defines _MSC_VER, which makes Boost.PFR 2.1 select the MSVC field-name parser.
// This override targets Boost.PFR 2.1's internal name_of_field_impl signature.
// Recheck the signature prefix/suffix when upgrading Boost.PFR or clang-cl.
#if defined(__clang__) && defined(_MSC_VER)
#define BOOST_PFR_FUNCTION_SIGNATURE __PRETTY_FUNCTION__
#define BOOST_PFR_CORE_NAME_PARSING                                                                                    \
    (sizeof("auto boost::pfr::detail::name_of_field_impl() [MsvcWorkaround = ") - 1, sizeof("}]") - 1, backward("."))
#endif

#include "Reflectable.h"
#include "ll/api/io/LogLevel.h"
#include <magic_enum/magic_enum.hpp>
#include <nlohmann/json.hpp>

#include <boost/pfr.hpp>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility> 


namespace nlohmann {

template <>
struct adl_serializer<ll::io::LogLevel> {
    static void to_json(json& j, ll::io::LogLevel value) { j = std::string(magic_enum::enum_name(value)); }

    static void from_json(const json& j, ll::io::LogLevel& value) {
        const auto level = j.is_number_integer() ? magic_enum::enum_cast<ll::io::LogLevel>(j.get<int>())
                                                 : magic_enum::enum_cast<ll::io::LogLevel>(j.get<std::string>());
        if (!level) {
            throw json::type_error::create(302, "invalid log level: " + j.dump(), &j);
        }
        value = *level;
    }
};

template <typename T>
    requires(ferrum::is_reflectable<T>::value && std::is_aggregate_v<T>)
struct adl_serializer<T> {
    /**
     * @brief 将聚合类型 T 序列化为 JSON 对象。
     */
    static void to_json(json& j, const T& value) {
        j                          = json::object();
        constexpr auto field_count = boost::pfr::tuple_size_v<T>;
        [&]<size_t... I>(std::index_sequence<I...>) {
            ((j[boost::pfr::get_name<I, T>()] = boost::pfr::get<I>(value)), ...);
        }(std::make_index_sequence<field_count>{});
    }

    /**
     * @brief 从 JSON 对象反序列化为聚合类型 T。
     */
    static void from_json(const json& j, T& value) {
        const T        default_value{};
        constexpr auto field_count = boost::pfr::tuple_size_v<T>;

        [&]<size_t... I>(std::index_sequence<I...>) {
            (([&] {
                 constexpr std::string_view name = boost::pfr::get_name<I, T>();

                 if (j.contains(name)) {
                     j.at(name).get_to(boost::pfr::get<I>(value));
                 } else {
                     boost::pfr::get<I>(value) = boost::pfr::get<I>(default_value);
                 }
             }()),
             ...);
        }(std::make_index_sequence<field_count>{});
    }
};

} // namespace nlohmann
