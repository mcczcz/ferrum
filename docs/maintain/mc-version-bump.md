# MC / LeviLamina 版本升级

**验收标准**：用上一版 SDK 编译的模组 DLL 无需重编，在新版 `ferrum-loader.dll` 上正常运行。

## 升级清单

- [ ] 更新 `tooth.json`（server 与 client 两处 `LeviLamina` 依赖）和 `xmake.lua` 中的版本。
- [ ] 刷新依赖并重新配置：
  ```powershell
  xmake repo -u
  xmake f -c -p windows -a x64 -m release -y --target_type=server
  ```
- [ ] 运行 `xmake`，根据编译错误仅修复 `cpp-shim/`；验证 `server` 与 `client` 两种 `--target_type` 均构建成功。
- [ ] 运行 ABI 门禁：
  ```powershell
  cargo test -p ferrum-abi
  cargo xtask layout-lock --check
  cargo xtask cbindgen --check
  ```
- [ ] 用上一 Release 预编译的旧版模组 DLL 在新版 BDS 上验证 load、enable、命令、事件、`ll mod disable/enable`、`ll mod reload` 与关服；确认日志无 `WRONG_THREAD`。

## 上游变更处理

| 上游变化 | 处理方式 | 是否允许修改 `ferrum-abi` |
| :--- | :--- | :--- |
| 头文件迁移或成员改名 | 仅调整 `cpp-shim` | 否 |
| 函数变为 `MCNAPI`（符号不可用） | 改用 LL 高层接口或虚函数；必要时向 `mcapi-requests` 提需求 | 否 |
| 枚举新增值 | 更新 `cpp-shim` 映射；ABI 仅追加常量并升 minor | 只能追加，不改旧值 |
| 游戏特性被移除 | 保留 VTable 槽位，shim 返回 `UNSUPPORTED` | 否 |

## 发布注意

- 仅修改 `cpp-shim` 或 `ferrum-host` 内部时，保持 `AbiVersion` 不变，只发布 `ferrum-loader`。
- 追加了 ABI 字段时，递增 `FERRUM_ABI_MINOR` 并同步发布 `ferrum-abi` 与 `ferrum-sdk`。
- 发布 tag 必须匹配 `tooth.json` 的 `version`（`v<version>`）。
