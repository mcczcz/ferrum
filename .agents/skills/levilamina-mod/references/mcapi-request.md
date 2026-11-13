# 工作流：MC API 链接错误 → 申请 MCAPI

编译出现 `LNK2019` / `LNK2001` / `undefined symbol`，且符号属于 MC 类型（非 `ll::`、非自己代码）时，按顺序执行：

1. **提取并分类**（会拉取最新 LeviLamina 头文件和 mcapi-requests 的 CI 校验脚本到 `.cache/mcapi-requests/`）：

   ```powershell
   xmake -y > build.log 2>&1
   python scripts/mcapi-request.py check --log build.log [--platform server|client]
   ```

   - lld-link 默认输出 demangle 后的名字，脚本会提示加 `/demangle:no` 重新构建。
   - `--platform` 默认取 `.xmake` 中的 `target_type`，必须与实际编译端一致。

2. **按分类处理**，只有「可申请」才提交：
   - **可申请**：最新头文件中为 `MCNAPI` → 进入第 3 步。
   - **最新 LeviLamina 已提供**：升级 LeviLamina 版本，不要申请。
   - **LeviLamina 符号**：LL 包与头文件版本不一致，检查依赖版本。
   - **无法申请**：头文件里没有或签名不匹配。通常是自己写错了签名、用了对端才有的函数、或调用了内联/模板函数。回头查头文件修代码，不要申请。

3. **提交前确认**，以下任一不满足都会被 bot 关闭：
   - `tooth.json` 的 `tooth` 指向**公开** GitHub 仓库的当前地址（不能是会重定向的旧地址）。
   - 仓库根目录有 `README.md`，且包含 issue 标题中的模组名（默认取 `tooth.json` 的 `info.name`，忽略大小写/空格/连字符；可用 `--mod-name` 指定）。
   - `Justification` 用英文写模组做什么、为什么需要这些符号，≤200 tokens。
   - 每个 issue 的 Classes + Symbols 合计最多 64 行（脚本自动拆分）。

   提交 issue 属于对外公开操作，**先给用户看 `--dry-run` 结果并获得确认再提交**：

   ```powershell
   pip install tiktoken
   python scripts/mcapi-request.py submit --dry-run --justification "My mod ... needs ... to ..."
   python scripts/mcapi-request.py submit --justification "..."        # 需要 GH_TOKEN 或 gh auth login
   ```

   脚本用与 CI 相同的 `check_request.py` 在本地校验，不通过不会提交。

4. **检查 bot 结果**（提交后 1~2 分钟）：

   ```powershell
   python scripts/mcapi-request.py status <issue>
   ```

   - `✅ Request check passed`：完成，等待维护者处理；实现后 bot 会自动关闭 issue，届时升级 LeviLamina。
   - `❌ Request check failed` 且 issue 被关闭：按报告修正（README、仓库地址、Justification、符号），然后修改原 issue，不要重开新 issue：

     ```powershell
     python scripts/mcapi-request.py edit <issue> --justification "..." [--symbol SYM ...] [--class CLS ...]
     ```

     编辑后 CI 重新校验，通过会自动重新打开。重复第 4 步直到通过。

申请期间不要用 Hook 地址硬编码等方式绕过；功能先用其他已导出的 API 实现，或等待申请合入。
