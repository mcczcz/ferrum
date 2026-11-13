---
name: levilamina-mod
description: 基于本模板开发 LeviLamina (Minecraft Bedrock) 原生 C++ 模组。涉及 cpp-shim/、lang/、config.jsonc、xmake.lua、manifest.json、tooth.json 或 LeviLamina / MC API 时使用；也用于处理 MC 符号链接错误（LNK2019/LNK2001）、MCNAPI、UntypedStorage 成员、空类/空 enum，以及向 mcapi-requests 申请 API。
---

# LeviLamina 模组开发（本模板）

## 第一原则：先查头文件，再写代码

LL 和 MC 的 API 变动频繁，记忆中的签名大概率过时。**任何 `ll::` / MC 类型的调用，写之前都要在头文件里确认**：类名、函数名、参数、const、返回值、所在头文件路径。

头文件位置（与当前构建使用的版本一致）：

1. 从 `compile_commands.json` 取 levilamina 的 include 目录：
   `grep -oE '[A-Za-z]:[^" ]*levilamina[^" ]*include' compile_commands.json | head -1`
   通常形如 `~/AppData/Local/.xmake/packages/l/levilamina/<版本>/<hash>/include/`
   - `ll/api/**` — LeviLamina API（事件、命令、Hook、表单、协程、服务等）
   - `mc/**` — BDS 头文件（Player、Level、Actor、ItemStack、Packet 等）
2. 没有 `compile_commands.json` 时先构建，或运行 `xmake project -k compile_commands`。
3. 本地无包时查上游：https://github.com/LiteLDev/LeviLamina （`src/ll/api/**`）。

查找方式：
- LL API：在 `ll/api/` 下 grep 关键词（如 `PlayerJoinEvent`、`getOrCreateCommand`、`LL_TYPE_INSTANCE_HOOK`）。
- MC 类成员：打开对应头文件，如 `mc/world/actor/player/Player.h`；不知道路径就 grep `class Player :`。
- 虚函数实现名带 `$` 前缀（如 `$tick`），Hook 时以头文件实际名称为准。
- 用法参考：上游 LeviLamina 仓库 `src/ll/core/**` 是 API 的真实调用示例。

查不到就说查不到，不要编造 API。

## 区分客户端与服务端

本模板同一份代码可编译 `server`（BDS）和 `client`（基岩版客户端）两个目标，两者的 LL 包、`mc/` 头文件、可用符号都不同。

- 同一 LL 版本在 xmake 包目录下有多个 hash 子目录，分别对应 server / client 构建。**查头文件前先确认当前 `target_type`**（`xmake f --target_type=...` 或 `compile_commands.json` 中的路径/`LL_PLAT_S`、`LL_PLAT_C` 宏），只查对应那份头文件。
- **头文件存在 ≠ 函数可用**。两端包里文件列表基本相同，但对端独有的类只是空壳：例如 server 包的 `LocalPlayer.h` 只有类型声明、没有任何 `MCAPI` 函数。必须看到函数声明本身才能用。
- `MCNAPI` 标记的函数在当前端不可用（无导出符号），不能调用或 Hook；只能用 `MCAPI` / `MCFOLD` / `MCTAPI` 标记的。
- 客户端包含本地世界的服务端逻辑，所以 `ServerPlayer`、`ServerLevel` 在 client 包里也可能有函数；但 `LocalPlayer`、`ClientInstance`、`mc/client/**` 这类只在 client 包里有实现。同名函数在两端的签名、虚函数表也可能不同，各自查对应头文件。
- 只在一端可用的代码用 `#ifdef LL_PLAT_S` / `#ifdef LL_PLAT_C` 包住，或放到 `src-server/`、`src-client/`（需在 `xmake.lua` 取消对应注释）。
- LL API 也有端差异：例如 `CommandRegistrar::getInstance(bool isClientSide)`、`ll::service::getMinecraft(bool isClientSide)` 需传对端参数，`getClientInstance()` 只在 `LL_PLAT_C` 下存在；`ll/api/event/` 中大量 player 事件基于 `ServerPlayer`，客户端不触发。
- 两端都要支持的功能，改完分别用 `--target_type=server` 和 `--target_type=client` 编译验证。

## 模板约定

- 入口 `cpp-shim/src/mod/Entry.cpp`：`load` 读语言和配置；`enable` 注册命令/事件/Hook；`disable` 反注册；`unload` 清理。
- 全局 `logger`、`config`、`tr()` 通过 `cpp-shim/src/mod/Global.h` 引入；代码放在 `namespace ferrum`。
- 新增 `.cpp` 放 `cpp-shim/src/` 下即可，`xmake.lua` 已通配。平台专属代码用 `LL_PLAT_S` / `LL_PLAT_C` 区分。
- 风格遵循 `.clang-format`、`.clang-tidy`：成员 `mXxx`，类型 PascalCase，函数 camelCase；注释用中文。

### 配置

- 改 `cpp-shim/src/Config/Config.h` 的同时改 `config.jsonc`（同名键 + 中文注释），字段名即 JSON 键名。
- 结构体必须是聚合类型（无构造函数/基类/私有成员）；嵌套结构体要特化 `is_reflectable<T>`。
- 枚举自动按名字序列化；其他自定义类型需在 `ConfigSerialization.h` 写 `adl_serializer`。
- 不兼容改动递增 `version`。

### 多语言

- 新文本同时加到 `lang/zh_CN.json` 和 `lang/en_US.json`，键名 `模块.含义`。
- 日志和玩家可见文本统一 `tr("key", args...)`（fmt `{}` 占位），不要硬编码。

### 线程

- 访问游戏对象只能在服务端线程；耗时操作放线程池，结果切回服务端线程（executor 见 `ll/api/thread/`、`ll/api/coro/`）。

## 构建与发布

```powershell
xmake f -a x64 -m release -p windows --toolchain=clang-cl --target_type=server -y   # 客户端 --target_type=client
xmake -y
```

- 产物在 `bin/<mod-name>/`。改完至少编译通过。
- 初始化模板：`./scripts/init-template.ps1 -ModName <name> -Tooth github.com/<owner>/<repo> [-DryRun]`。
- 依赖其他模组：写入 `manifest.json` 的 `dependencies` 和 `tooth.json`。
- 升级 LeviLamina：同步 `tooth.json` 两个 variant 的版本号。

## 按需工作流

以下文件只在遇到对应情况时再读，不要提前加载：

| 触发条件 | 读取 |
| --- | --- |
| 编译报 `LNK2019` / `LNK2001` / `undefined symbol`，且是 MC 符号 | [references/mcapi-request.md](references/mcapi-request.md) |
| 需要的 MC 类型是空类 / 空 `enum class`，或成员是 `::ll::UntypedStorage<..> mUnkxxxx` | [references/class-request.md](references/class-request.md) |

两者都会对外提交 issue 或生成 issue 正文，**提交前必须得到用户确认**。
