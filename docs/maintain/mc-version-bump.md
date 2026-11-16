# MC / LeviLamina 版本升级

验收线：用旧版 SDK 编译的模组 DLL，不重编译，在新版 `ferrum-loader.dll` 上照常运行。

## 清单

- [ ] 改依赖版本。`tooth.json` 里 `github.com/LiteLDev/LeviLamina`（server 与 client 两处），以及 `xmake.lua` 的 `add_requires("levilamina", ...)`。
- [ ] 刷新包缓存：
  ```powershell
  xmake repo -u
  xmake f -c -p windows -a x64 -m release -y --target_type=server
  ```
- [ ] 运行 `xmake`，按编译错误修 `cpp-shim/`。常规改动只在这个目录。
- [ ] 跑 `ferrum-abi` 门禁：
  ```powershell
  cargo test -p ferrum-abi
  cargo xtask layout-lock --check
  cargo xtask cbindgen --check
  ```
- [ ] 旧二进制实测：取上一个 Release 里预编译的基准模组，不要用当前分支重编。放进新版 BDS 的 `mods/`，走一遍 load、enable、命令、事件、`ll mod disable` / `enable`、`ll mod reload`、关服。
- [ ] 日志里没有 `WRONG_THREAD`。有说明上游事件线程变了，要修 shim 的主线程处理。
- [ ] `server` 与 `client` 两种 `--target_type` 都能构建（CI 已覆盖）。

## 上游变更怎么处理

| 变更 | 做法 | 能动 `ferrum-abi` 吗 |
| :--- | :--- | :--- |
| 头文件迁移、成员改名 | 改 `cpp-shim` 的 `#include` 和成员访问 | 不能 |
| 函数变为 `MCNAPI`（不可用） | 换 LeviLamina 高层封装或公开虚函数；确需符号就向上游 `mcapi-requests` 提需求 | 不能 |
| 枚举新增值 | `cpp-shim` 映射表补分支；ABI 里只追加常量并递增 minor | 只能追加，不改旧值 |
| 游戏特性被移除 | 保留槽位，shim 返回 `UNSUPPORTED` | 不能删槽位或改签名 |

## 发布

- 只改了 `cpp-shim` 或 `ferrum-host` 内部：`AbiVersion` 不变，只发新的 `ferrum-loader`，不用发新 SDK。
- 追加了 ABI 字段：递增 `FERRUM_ABI_MINOR`，同时发布 `ferrum-abi` 和 `ferrum-sdk`。
- 发布 tag 必须等于 `tooth.json` 的 `version`（`v<version>`），否则 release 工作流的校验会失败。
