# manifest.json

Ferrum 模组沿用 LeviLamina 的 `manifest.json` 格式，只需将 `type` 设为 `"ferrum"`。

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

## 字段说明

| 字段 | 必填 | 说明 |
| :--- | :--- | :--- |
| `name` | 是 | 必须与模组目录名完全一致 |
| `entry` | 是 | 模组 DLL 文件名，相对模组目录 |
| `version` | 否 | 语义化版本；省略时 `ctx.version()` 返回 `"0.0.0"` |
| `type` | 是 | 固定为 `"ferrum"` |
| `dependencies` | 是 | 必须包含 `ferrum-loader` |
| `author` `description` `optionalDependencies` `conflicts` `loadBefore` | 否 | 与 LeviLamina 原生含义一致 |

## 约束

1. `name` 与 `mods/<name>/` 目录名不一致时，LeviLamina 会拒绝加载。
2. `type` 必须为 `"ferrum"`。
3. `dependencies` 应显式声明 `ferrum-loader` 与显式版本前缀（如 `"^0.1.0"`），以保证加载顺序与 `--cascade` 级联卸载正确。

## 分发

使用 `lip` 打包时，在 `tooth.json` 中声明对 `ferrum-loader` 的依赖，将 DLL 与 `manifest.json` 部署到 `mods/<name>/` 即可。服主无需安装 Rust 工具链。
