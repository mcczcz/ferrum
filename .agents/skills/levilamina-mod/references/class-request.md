# 工作流：类申请

需要用的 MC 类型在头文件里不可用时，先检查属于哪种情况：

```powershell
python scripts/mcapi-request.py class-info <完整类名> [...] [--platform server|client]
```

类名必须带完整命名空间 / 外层类（如 `Realms::Player` 和 `Player` 是不同类型）。脚本按当前端查最新 LeviLamina 头文件，分为：

- **成员已全部翻译**：无需申请，回头查头文件找对应成员。
- **找不到定义**：拼写或命名空间错误，或只有前置声明；先 grep 头文件确认真实名称。
- **未翻译成员**（含 `::ll::UntypedStorage<A, S> mUnkxxxx`）→ 申请成员变量翻译，见下文 A。
- **空壳**（空 `class` / `struct` / `enum class`，无函数、无成员、无枚举值）→ 先确认它确实有内容，再申请开放，见下文 B。

## A. 成员变量翻译（提交到 mcapi-requests）

1. 在 Justification 中写明需要哪些成员、用来做什么，以及为什么不能用现有函数代替（例如必须读取某个状态，而该状态没有 getter）。
2. 与符号申请合并在同一流程中处理：`--class` 可以和 `--symbol` 同时使用，也可以单独使用。提交前的仓库/README 检查、`status` 查看 bot 结果、`edit` 修改被关闭的 issue，都和 [mcapi-request.md](mcapi-request.md) 第 3、4 步相同。

```powershell
python scripts/mcapi-request.py submit --dry-run --class <完整类名> --justification "..."
python scripts/mcapi-request.py submit --class <完整类名> --justification "..."
```

CI 只接受**确实含有 `UntypedStorage` 成员**的类。成员已全部翻译的类会被判定为 `all member variables of this class are already available` 并关闭；空壳类和 enum 也会被拒绝，不要放进 Classes 字段。

## B. 空壳类 / 空 enum class 开放申请（只生成正文，不自动提交）

空壳不一定是没开放。必须同时满足下列条件，才能判定为"LL 没有开放这个类型"：

- 其他头文件在函数参数、返回值或成员（`TypedStorage<..., ::X>`）中引用了它，说明它实际有内容。`class-info` 会列出这些引用。
- 另一端（server/client）的头文件里有完整定义，或者能从引用的函数和成员推断出它有字段、函数或枚举值。
- 这个类型属于当前端确实会用到的逻辑。例如 `LocalPlayer`、`ClientInstance` 在 server 端是空壳是正常的，因为服务端本来就没有这些逻辑。这种情况应该改用 `LL_PLAT_C`，不要申请。

确认后生成正文：

```powershell
python scripts/mcapi-request.py class-body <完整类名> [...] --justification "Why the mod needs this type, what fields/functions/values it uses"
```

正文会包含模组信息、平台、理由、空壳所在文件、另一端定义和引用证据，保存在 `.cache/mcapi-requests/class-request-*.md`。mcapi-requests 的 CI 会拒绝这类请求，所以脚本**不自动提交**。把正文交给用户，由用户决定发到哪里。
