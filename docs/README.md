# Ferrum 文档

Ferrum 是 LeviLamina 上的 Rust 二级模组加载器：服主装 `ferrum-loader`，模组作者只用 Cargo 写 `type = "ferrum"` 的模组。

当前进度见 [路线图](./roadmap.md)。

## 写模组

| 文档 | 内容 |
| :--- | :--- |
| [快速开始](./guide/quick-start.md) | 环境、`Cargo.toml`、最小模组、部署 |
| [SDK 手册](./guide/sdk.md) | 生命周期、上下文、日志、主线程令牌、错误、玩家/实体/方块、命令 |
| [事件](./guide/events.md) | 玩家事件；与原生模组互通的自定义事件 |
| [测试与调试](./guide/testing.md) | `ferrum-host-mock` 单元测试；BDS 联调 |
| [清单文件](./guide/manifest.md) | `manifest.json` 字段与校验规则 |

## 设计（维护者必读）

改 ABI、FFI 或生命周期代码前先读对应文档。

| 文档 | 内容 |
| :--- | :--- |
| [架构](./design/architecture.md) | 目标、分层、与 LeviLamina 的对接、生命周期 |
| [ABI](./design/abi.md) | 可跨边界的类型、握手、VTable 演进规则 |
| [内存](./design/memory.md) | 双堆隔离、借用视图、Writer 输出、回调所有权 |
| [句柄](./design/handles.md) | 代数句柄、游戏对象 ID、按需重新解析 |
| [线程](./design/threading.md) | 主线程令牌、锁规则、重入、停用与卸载协议 |
| [错误](./design/errors.md) | 状态码、last-error、panic 防火墙、熔断 |

## 维护

| 文档 | 内容 |
| :--- | :--- |
| [构建](./maintain/build.md) | 构建命令、生成物校验、调试 |
| [新增领域 API](./maintain/new-domain-api.md) | 从 ABI 到 SDK 的改动清单 |
| [MC 版本升级](./maintain/mc-version-bump.md) | 升级 LeviLamina / BDS 的步骤 |
