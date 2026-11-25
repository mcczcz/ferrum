# Ferrum 文档

本文档按受众分为两层：外部**模组开发者**只需阅读第一部分；**项目自身维护者**阅读第二部分。

---

## 模组开发者文档

面向使用 `ferrum-sdk` 编写 Rust 模组（以及通过公开头文件与 Rust 模组互通的 C++ Native 模组）的开发者。

| 文档 | 内容 |
| :--- | :--- |
| [快速开始](./guide/quick-start.md) | 环境要求、`Cargo.toml`、最小模组、部署与热重载 |
| [清单文件](./guide/manifest.md) | `manifest.json` 字段、依赖约束与打包规则 |
| [SDK 手册](./guide/sdk.md) | 生命周期、日志、主线程令牌、错误与 panic、世界/实体/方块/NBT、命令、表单、经济、调度与异步 |
| [事件](./guide/events.md) | LL 官方事件（主线程与同步事件）；Rust 与 C++ 原生模组互通的自定义事件 |
| [测试与调试](./guide/testing.md) | `MockHost` 单元测试与 BDS 联调 |

---

## 项目设计与维护文档

面向 Ferrum Loader 本身的贡献者与维护者。修改 `ferrum-abi`、FFI 边界或生命周期逻辑前，必须先阅读对应设计文档。

### 架构与契约设计

| 文档 | 内容 |
| :--- | :--- |
| [架构](./design/architecture.md) | 目标与非目标、分层边界、LeviLamina 接入与生命周期映射 |
| [ABI 契约](./design/abi.md) | 跨边界类型规则、握手与容量协商、VTable 尾追加演进、接口清单与布局锁 |
| [内存与所有权](./design/memory.md) | 双堆隔离、借用视图、`FfiByteWriter`、回调 `env / invoke_fn / drop_fn` |
| [句柄与标识](./design/handles.md) | 零游戏指针约束、瞬态引用、值标识、按需重解析与代数句柄池 |
| [线程与卸载](./design/threading.md) | 主线程检查、HRTB 令牌、锁与重入守卫、停用协议、RAII 计数守卫与影子加载 |
| [错误、Panic 与熔断](./design/errors.md) | 双向异常防火墙、回调毒化、`FerrumStatus`、`LastErrorSlot` 与熔断器 |

### 工程维护

| 文档 | 内容 |
| :--- | :--- |
| [构建](./maintain/build.md) | 工具链要求、Rust 检查、`xmake` 构建与跨语言调试 |
| [新增领域 API](./maintain/new-domain-api.md) | 从 `ferrum-abi` 到 SDK、Mock 与文档的完整改动清单 |
| [MC 版本升级](./maintain/mc-version-bump.md) | 升级 LeviLamina / BDS 时的验证清单与兼容性处理 |
| [路线图](./roadmap.md) | 各里程碑当前状态与待验收项 |
