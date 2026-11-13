# 清单文件与依赖管理

Ferrum 二级模组完全复用 LeviLamina 原生的 `manifest.json` 模型与 `ModLoadPlanner` 拓扑规划器（参见 LeviLamina `ll/api/mod/Manifest.h`）。

---

## 1. 标准 `manifest.json` 规范

```json
{
    "name": "hello-ferrum",
    "entry": "hello_ferrum.dll",
    "version": "0.1.0",
    "type": "ferrum",
    "author": "YourName",
    "description": "Example Rust mod powered by Ferrum",
    "dependencies": [
        {
            "name": "ferrum-loader",
            "version": "^0.1.0"
        }
    ],
    "optionalDependencies": [],
    "conflicts": [],
    "loadBefore": []
}
```

### 关键校验规则（违反将导致拒绝加载）

1. **`name` 必须与所在文件夹名严格一致**：
   - LeviLamina `ModRegistrar` 强制校验 `manifest.name == dirName`。若文件夹叫 `mods/hello-ferrum/`，则 `name` 必须为 `"hello-ferrum"`。
2. **`type` 固定为 `"ferrum"`**：
   - 标识该模组由 `FerrumModManager` 接管，跳过 `NativeModManager` 的统一内存操作符校验并走 Ferrum ABI 握手流程。
3. **必须显式声明依赖 `ferrum-loader`**：
   - 在 `dependencies` 中加入 `{"name": "ferrum-loader", "version": "^0.1.0"}` 有两重关键作用：
     - 确保 `ModLoadPlanner` 在拓扑排序时始终将 `ferrum-loader` 排在所有 Rust 模组之前加载（保证 `FerrumModManager` 先完成注册）；
     - 当管理员执行 `ll mod unload ferrum-loader --cascade` 时，能够安全地按逆序先卸载所有下游 Rust 模组。
4. **版本约束必须带显式比较前缀**：
   - 遵循 LeviLamina 现代版本范围语法（例如 `"^0.1.0"`、`">=0.1.2 <0.2.0"`、`"=0.1.0"`），避免使用裸版本号（裸版本号在 LeviLamina 中会触发弃用警告）。

---

## 2. 跨模组交互与扩展接口（Mod-to-Mod Interop）

由于不同 Rust 模组各自是独立的 `cdylib`（可能由不同版本 `rustc` 编译，且各自持有独立堆），**两个 Ferrum 模组之间同样严禁直接传递 `repr(Rust)` 对象或裸函数指针**！

当库模组（如经济前置插件 `ferrum-economy`）需要向其他模组暴露 API 时，推荐以下两种标准方式：

1. **宿主服务注册总线（Service Registry via `CoreApiV1`）**：
   - 前置模组定义一个小型 `#[repr(C)]` 服务 VTable（遵守与 `ferrum-abi` 相同的规则：首字段 `struct_size: u32`、只尾追加、只用白名单类型），通过 `ctx.services().register("economy.api.v1", &ECONOMY_VTABLE)` 注册到 `ferrum-host`；
   - 消费方模组在 `manifest.json` 的 `dependencies`（或 `optionalDependencies`）中声明依赖前置模组，通过 `ctx.services().query::<EconomyApiV1>("economy.api.v1")` 获得 **`ServiceHandle`（代数句柄）**，而**不是** `&'static` VTable 引用；
   - 每次调用通过 `handle.acquire()?` 取得短生命周期的 `ServiceGuard`，再经由它调用服务函数。宿主在 `acquire` 时校验句柄代数与提供方状态，并在 guard 存活期间为提供方计入 `active_invocations`；
   - 提供方停用时，宿主先撤销服务（之后 `acquire` 返回 `Err(MOD_DISABLED)`），等待所有 guard 释放，再继续提供方的停用流程。因此消费方永远不会调用到已停用模组的代码；
   - `ServiceGuard` 为 `!Send`，且不应跨 `.await` 或跨 Tick 持有；
   - 依赖关系仍由 LeviLamina 级联处理：卸载提供方时，`ll mod unload <provider> --cascade` 会先停用依赖它的消费方。
2. **自定义事件广播**：
   - 松耦合通知优先使用结构化自定义事件载荷投递，无需强链接前置 crate。

---

## 3. 使用 `lip` (Tooth) 分发打包

发布到社区包管理器 `lip` 时，`tooth.json` 中同样声明对 `ferrum-loader` 包的依赖，并将编译好的 `.dll` 与 `manifest.json` 映射至 `mods/<mod-name>/` 目录即可，终端服主安装体验与普通 LeviLamina 插件完全一致，无需在生产服务器上安装 Rust 环境。
