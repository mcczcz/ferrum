# MC / LeviLamina 跨版本升级标准作业程序 (SOP)

当 Minecraft Bedrock 发布新版本且上游 LeviLamina 完成适配发布后，Ferrum 维护者按本 SOP 执行版本跟进。**核心验收红线：使用旧版 SDK 编译的 `examples/` 二进制 `.dll` 文件在未重编译的前提下，必须在新版 `ferrum-loader.dll` 上原样通过全部集成测试！**

---

## 第一步：更新依赖锁定

1. 在 `xmake.lua` 与 `tooth.json` 中更新 `levilamina` 依赖版本约束（例如从 `1.2.0` 升级至新目标版本）。
2. 清理并刷新 xmake 包缓存：
   ```powershell
   xmake repo -u
   xmake f -c -p windows -a x64 -m release -y --target_type=server
   ```

---

## 第二步：修复 `cpp-shim/` 编译差异（唯一允许常规修改的目录）

运行 `xmake` 收集 `clang-cl` 对 `cpp-shim/` 的编译诊断。常见上游变更分类及处置规范如下：

| 上游变更类型 | 标准处置方案 | 是否允许改动 `ferrum-abi`？ |
| :--- | :--- | :--- |
| **`src/mc` 头文件目录迁移或类成员重命名** | 直接在 `cpp-shim/*.cpp` 内更新 `#include` 路径与成员访问方式 | **严禁改动** |
| **MC 某函数被标记为 `MCNAPI`（不可用）** | 查阅 LeviLamina 替代高层封装或通过现有公开虚函数/组件系统重写；若确需底层符号，向上游 `mcapi-requests` 提单 | **严禁改动** |
| **MC 枚举新增了枚举值（如新维度、新伤害源）** | 在 `cpp-shim` 映射表中补充新分支；若模组使用旧版 SDK，未知值映射为 `Other(raw_id)` 或保留位，在 `ferrum-abi` 追加新常量并提升 `minor` | 仅可尾追加常量，**严禁改旧值** |
| **某项旧游戏特性被 Mojang 彻底移除** | 保持 VTable 原函数槽位不变，`cpp-shim` 改为返回 `FerrumStatus::UNSUPPORTED` 并在首次调用时输出限频弃用日志 | **严禁删除槽位或改签名** |

---

## 第三步：ABI 零破坏回归门禁（Binary Compatibility Gate）

在合并升级 PR 之前，必须按顺序通过以下三项检查：

1. **布局锁与生成物检查**：
   ```powershell
   cargo test -p ferrum-abi
   cargo xtask layout-lock --check
   cargo xtask cbindgen --check
   ```
   确认 `layout_lock.rs` 中没有任何已有字段的 `size_of`（按值结构体）或 `offset_of!` 被修改，且 C 头文件与 Rust 定义一致。
2. **跨版本旧二进制免重编实测（最关键！）**：
   - 从上一次 GitHub Release 下载预编译好的基准测试模组 `compat_test_mod_v1_0.dll`（**绝对不要用当前分支重新编译它**）；
   - 将其放入新版 BDS + 新版 `ferrum-loader.dll` 的 `mods/` 目录；
   - 启动 BDS 自动执行冒烟套件：验证 `on_load` -> `on_enable` -> 命令执行 -> 事件收发 -> Tick 调度 -> `ll mod disable` / `enable` -> `ll mod reload` -> 正常关服全链路零报错、零崩溃。
   - 同时验证每个事件回调确实在主线程触发（宿主若记录了 `WRONG_THREAD` 错误，说明上游事件线程归属发生变化，需修正事件的 `MainThreadEvent` / `AnyThreadEvent` 标注）。
3. **双端构建检查（如支持 Client 目标）**：
   - 分别在 `--target_type=server`（及适用的 `client`）配置下完整编译零警告通过。

---

## 第四步：版本发布策略

- 若本次升级**仅修改了 `cpp-shim` 或 `ferrum-host` 内部实现**，未新增任何 ABI 接口：
  - `AbiVersion` 保持不变（如 `1.2.0`）；
  - 发布新的 `ferrum-loader` 补丁/适配包（版本号或构建元数据关联目标 LeviLamina 版本），无需发布新版 `ferrum-sdk` crate！
- 若顺带合入了新 ABI 尾追加字段：
  - 递增 `AbiVersion.minor`，同步发布新版 `ferrum-abi` 与 `ferrum-sdk`。
