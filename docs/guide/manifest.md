# manifest.json

Ferrum 模组沿用 LeviLamina 的 `manifest.json` 和依赖规划，`type` 取 `"ferrum"`。

```json
{
    "name": "hello-ferrum",
    "entry": "hello_ferrum.dll",
    "version": "0.1.0",
    "type": "ferrum",
    "author": "YourName",
    "description": "Example Rust mod",
    "dependencies": [{ "name": "ferrum-loader", "version": "^0.1.0" }],
    "optionalDependencies": [],
    "conflicts": [],
    "loadBefore": []
}
```

## 字段

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `name` | 是 | 必须等于模组目录名 |
| `entry` | 是 | 模组 dll 文件名，相对模组目录 |
| `version` | 否 | 缺省时 `ctx.version()` 返回 `"0.0.0"` |
| `type` | 是 | 固定 `"ferrum"` |
| `dependencies` | 是 | 应含 `ferrum-loader` |
| `author` `description` `optionalDependencies` `conflicts` `loadBefore` | 否 | 与 LeviLamina 原生含义相同 |

## 规则

1. `name` 与目录名不一致，LL 拒绝加载。目录 `mods/hello-ferrum/` 对应 `"name": "hello-ferrum"`。
2. `type` 必须是 `"ferrum"`，模组才会交给 Ferrum 的模组管理器并执行 ABI 握手。
3. `dependencies` 应包含 `ferrum-loader`。这样 LL 会先加载它，`ll mod unload ferrum-loader --cascade` 也会先卸载依赖它的模组。
4. 版本约束写显式前缀，如 `"^0.1.0"`、`">=0.1.2 <0.2.0"`、`"=0.1.0"`。裸版本号在 LL 中触发弃用警告。

## 打包

用 `lip` 发布时，`tooth.json` 依赖 `ferrum-loader` 包，把 dll 和 `manifest.json` 映射到 `mods/<name>/`。服主不需要安装 Rust。

## 模组间交互

跨模组服务调用尚未实现，规划中，见 [../roadmap.md](../roadmap.md)。当前可用 [events.md](./events.md) 中的自定义事件做松耦合通知。
