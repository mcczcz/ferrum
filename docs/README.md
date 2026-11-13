# Ferrum 文档中心

本文档库定义了 **Ferrum**（构建在 LeviLamina 之上的 Rust 二级模组加载器与跨版本稳定运行时）的整体架构、工程铁律、下游模组开发标准与长期维护规范。

任何针对 `ferrum-abi`、`ferrum-host`、`cpp-shim` 或 `ferrum-sdk` 的架构演进与新 API 引入，必须严格遵循 `01-specs/` 中的硬性契约。

> **当前实现状态**：已完成 **M0（骨架与生成管线）** 与 **M1（加载器最小闭环，ABI `1.0.0`）**。
> 已可用能力：`FerrumMod` 生命周期（`on_load` / `on_enable` / `on_disable` / `on_unload`）、`ModContext` 元数据与目录、`Logger`、双向版本握手、影子副本重载、panic 防火墙与熔断器、`ferrum-host-mock`。
> 事件、命令、玩家、异步调度、表单与跨模组服务等能力按 [实施路线图](./00-overview/roadmap.md) 在 M2–M4 分阶段落地；对应指南中已标明实现状态。

---

## 文档目录与阅读路线

### 00. 概览与架构 (`00-overview/`)
面向所有参与者，确立项目边界与全局运行机制：
- [项目愿景与非目标声明](./00-overview/vision-and-goals.md)：解决的核心痛点、设计取舍与明确不做的事。
- [三层隔离架构与生命周期映射](./00-overview/architecture-overview.md)：`cpp-shim` / `ferrum-host` / `ferrum-sdk` 的物理隔离边界，以及如何精确对接 LeviLamina 的 `ModManagerRegistry` 与启动时序。
- [实施路线图](./00-overview/roadmap.md)：M0–M5 里程碑划分与各阶段验收标准。

### 01. 核心工程规范 (`01-specs/`)
面向核心维护者与架构审查，属于**不可违背的硬性契约（Hard Contracts）**：
- [ABI 稳定性与 VTable 演进铁律](./01-specs/abi-stability-spec.md)：`#[repr(C)]` 布局、握手协商、尾追加演进、能力发现与废弃策略。
- [跨边界内存与所有权规范](./01-specs/memory-and-ownership.md)：双堆物理隔离、零拷贝借用视图、Writer 回调写入模式与闭包生命周期管理。
- [句柄与游戏对象标识规范](./01-specs/handle-and-id-design.md)：代数槽位池（Generational Arena）、按 ID 每次重新解析的瞬态引用、持久化 ID，以及“裸指针永不出 `cpp-shim`”原则。
- [线程模型、并发与热卸载安全规范](./01-specs/threading-and-safety.md)：`MainThreadToken` 与 HRTB、回调线程约束、`FnMut` 重入守卫、异步桥接（对接 `ServerThreadExecutor`）、停用协议与默认保留镜像的卸载策略。
- [状态码、异常与 Panic 隔离规范](./01-specs/error-handling-spec.md)：双向异常防火墙（C++ `Expected<T>`/`try-catch` 与 Rust `catch_unwind`）、回调毒化、防火墙覆盖边界、TLS 错误上下文与熔断。

### 02. 模组开发者指南 (`02-mod-developer-guide/`)
面向下游 Rust 模组作者：
- [快速开始与项目配置](./02-mod-developer-guide/quick-start.md)：工具链配置、`cdylib` 编译选项、最小可运行模组与目录部署布局。
- [Rust SDK 开发手册（`ferrum-sdk` 参考）](./02-mod-developer-guide/sdk-reference.md)：`#[mod_entry]`、`FerrumMod` 生命周期、`ModContext`、`Logger`、`MainThreadToken`、`Error`/panic 隔离与线程约束完整参考。
- [模组测试与调试指南](./02-mod-developer-guide/testing-and-debugging.md)：使用 `ferrum-host-mock` 编写纯 Rust 单元测试，以及 BDS 影子副本热重载与 PDB 断点联调。
- [清单文件与依赖管理](./02-mod-developer-guide/manifest-and-deps.md)：`manifest.json`（`type: "ferrum"`）规范、依赖拓扑声明与 `lip` 打包结构。
- [事件、命令、表单与异步调度指南（规划）](./02-mod-developer-guide/core-apis-guide.md)：M2–M4 核心领域子系统设计与目标用法。

### 03. 维护者作业手册 (`03-maintainer-playbook/`)
面向 Ferrum 引擎维护者与贡献者：
- [源码联编与调试指南](./03-maintainer-playbook/build-from-source.md)：Cargo + xmake 混合构建管线、cbindgen / 布局锁生成物校验、联合断点调试。
- [MC / LeviLamina 跨版本升级 SOP](./03-maintainer-playbook/mc-version-bump-sop.md)：游戏更新时的零下游破坏适配步骤与回归检查单。
- [新增领域 API 标准流程](./03-maintainer-playbook/adding-new-domain-api.md)：从领域建模到 SDK 安全封装的五步落地清单与审查红线。

---

## 规范等级约定

文档中使用以下关键词标示约束强度：
- **MUST / 必须 / 严禁**：架构级铁律，违反将直接导致未定义行为（UB）、跨 DLL 堆损坏、崩服或跨版本 ABI 破裂。
- **SHOULD / 应当**：推荐的工程最佳实践，偏离需在代码审查中给出明确理由并记录安全证明。
- **MAY / 可以**：可选的优化或扩展方式。
