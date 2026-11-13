add_rules("mode.debug", "mode.release")
add_rules("plugin.compile_commands.autoupdate", {outputdir = "."})

add_repositories("levimc-repo https://github.com/LiteLDev/xmake-repo.git")

-- add_requires("levilamina x.x.x") for a specific version
-- add_requires("levilamina develop") to use develop version
-- please note that you should add bdslibrary yourself if using dev version
if is_config("target_type", "server") then
    add_requires("levilamina", {configs = {target_type = "server"}})
else
    add_requires("levilamina", {configs = {target_type = "client"}})
end

add_requires("levibuildscript")

if not has_config("vs_runtime") then
    set_runtimes("MD")
end

option("target_type")
    set_default("server")
    set_showmenu(true)
    set_values("server", "client")
option_end()

-- ferrum-loader.dll = cpp-shim (C++) + ferrum-host (Rust staticlib)
target("ferrum-loader")
    add_rules("@levibuildscript/linkrule")
    add_rules("@levibuildscript/modpacker")
    if is_plat("windows") then
        add_defines("NOMINMAX", "UNICODE", "_HAS_CXX23=1")
        set_exceptions("none") -- To avoid conflicts with /EHa.
        add_cxflags( "/EHa", "/utf-8", "/W4", "/w44265", "/w44289", "/w44296", "/w45263", "/w44738", "/w45204")
        add_cxflags(
            "/EHs",
            "-Wno-microsoft-cast",
            "-Wno-invalid-offsetof",
            "-Wno-c++2b-extensions",
            "-Wno-microsoft-include",
            "-Wno-overloaded-virtual",
            "-Wno-ignored-qualifiers",
            "-Wno-missing-field-initializers",
            "-Wno-potentially-evaluated-expression",
            "-Wno-pragma-system-header-outside-header",
            {tools = {"clang_cl"}}
        )
        set_toolchains("clang-cl")
    end
    add_packages("levilamina")

    set_kind("shared")
    set_languages("c++20")
    set_symbols("debug")
    add_headerfiles("cpp-shim/src/**.h", "cpp-shim/include/**.h")
    add_files("cpp-shim/src/**.cpp")
    add_includedirs("cpp-shim/src", "cpp-shim/include")
    if is_config("target_type", "server") then
        add_defines("LL_PLAT_S")
    else
        add_defines("LL_PLAT_C")
    end

    -- ferrum-host 静态库及其系统依赖（来自 `cargo rustc -- --print native-static-libs`）。
    -- 严禁开启 +crt-static：与 cpp-shim 统一使用 /MD 动态 CRT。
    local cargo_profile = is_mode("debug") and "debug" or "release"
    add_linkdirs(path.join("target", cargo_profile))
    add_links("ferrum_host")
    add_syslinks("kernel32", "ntdll", "userenv", "ws2_32", "dbghelp", "synchronization", "advapi32", "bcrypt")

    before_build(function (target)
        local args = {"build", "-p", "ferrum-host", "--locked"}
        if not is_mode("debug") then
            table.insert(args, "--release")
        end
        os.vrunv("cargo", args)
        -- 生成头文件必须与 Rust 定义一致。
        os.vrunv("cargo", {"xtask", "cbindgen", "--check"})
    end)

    after_build(function (target)
        local output_dir = path.join(os.projectdir(), "bin", target:name())
        os.mkdir(output_dir)
        os.cp(path.join(os.projectdir(), "config.jsonc"), path.join(output_dir, "config.jsonc"))

        local lang_src = path.join(os.projectdir(), "lang")
        local lang_dst = path.join(output_dir, "lang")
        if os.isdir(lang_src) then
            os.mkdir(lang_dst)
            os.cp(path.join(lang_src, "*"), lang_dst)
            print("Copied lang folder to " .. lang_dst)
        end
    end)
