# 路线图

| 里程碑 | 状态 |
| :--- | :--- |
| M0 仓库骨架与生成管线 | 完成 |
| M1 加载器最小闭环 | 完成 |
| M2 回调基础设施与首批领域 API | 进行中，缺真实 BDS 验收 |
| M3 调度与异步 | API 已实现，待 BDS 重载验收 |
| M4 表单、跨模组服务、更多领域 | 进行中 |
| M5 真正的热卸载（可选） | 未开始 |

前一个里程碑验收通过前，不扩展下一个里程碑的 API。

## M0 完成

- [x] workspace、`cpp-shim`、cbindgen 与 layout-lock 生成物
- [x] fmt、clippy、test、`--check` 校验

## M1 完成

- [x] ABI 核心类型、模组描述符、`ferrum_mod_entry_v1`
- [x] 影子复制加载、版本握手、句柄池、panic 上报
- [x] `#[mod_entry]`、`catch_unwind` 跳板、`panic = "abort"` 拦截
- [x] 卸载只保留镜像，不 `FreeLibrary`
- [ ] 真实 BDS 验收：`examples/hello` 走完 load、enable、disable、reload、关服；`on_enable` panic 不崩服

## M2 进行中（ABI 1.1）

- [x] 回调基础设施：强类型回调、`ModOwnership`、停用协议、HRTB 令牌、重入守卫、panic 毒化
- [x] 玩家、Actor、Mob 分层接口；基础方块读写
- [x] 命令 Builder（Int、Float、Bool、String、Player）
- [x] 玩家事件：加入、断开、聊天
- [x] 自定义事件：Rust 与 native 互通
- [x] `ferrum-host-mock` 覆盖以上 API
- [ ] 真实 BDS 验收：回调内注销自身、回调内 `ll mod disable <self>`、踢出玩家后继续访问，均不崩服且返回预期错误
- [ ] 发布 `compat_test_mod_v1_0.dll` 作为首个二进制兼容基准

## M3 调度与异步

- [x] `SchedulerApiV1`：`post`、`after_ticks`、`every_ticks`、`cancel`；停用时取消
- [x] SDK：运行时无关的 `on_main_thread(...).await`；停用时等待方收到 `ModDisabled`
- [x] 非 `Send` 闭包只在主线程析构
- [ ] 验收：tokio 示例模组连续 reload 100 次，无崩溃、无悬挂 future

## M4 表单、服务、更多领域

- [x] 表单：Simple、Modal、Custom，及异步封装
- [ ] 命令：`#[derive(CommandArgs)]` 派生宏；更多参数类型（`BlockPos`、`Vec3`、`Actor`、`Enum` 等）
- [ ] 事件：更多 LL 事件（如方块破坏、实体死亡）
- [ ] 跨模组服务：`ServiceHandle` 与 `ServiceGuard`，提供方停用时撤销并等待释放
- [ ] 按需增加领域子表：计分板、容器、实体属性等，流程见 [新增领域 API](./maintain/new-domain-api.md)

## M5 真正的热卸载（可选）

- [ ] `HOT_UNLOAD_SAFE` 路径：排空后确认无线程在镜像内，再 `FreeLibrary`
- [ ] 前提：M1 到 M4 稳定运行，且确有内存回收需求
