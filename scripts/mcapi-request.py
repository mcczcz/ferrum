#!/usr/bin/env python3
"""从链接错误中提取 MC 符号，对照最新 LeviLamina 头文件分类，并向 LiteLDev/mcapi-requests 申请 MCNAPI。

校验逻辑直接复用 mcapi-requests 仓库 CI 的 check_request.py，本地结果与 bot 一致。

用法:
  python scripts/mcapi-request.py check  [--log build.log] [--platform server|client]
  python scripts/mcapi-request.py submit --justification "..." [--dry-run] [--symbol SYM ...] [--class CLS ...]
  python scripts/mcapi-request.py status <issue>
  python scripts/mcapi-request.py edit   <issue> --justification "..." [--symbol SYM ...] [--class CLS ...]
  python scripts/mcapi-request.py class-info <Class> [...] [--platform server|client]
  python scripts/mcapi-request.py class-body <Class> [...] --justification "..."

类申请：
  - 含 ll::UntypedStorage 未翻译成员的类 → submit --class 提交到 mcapi-requests
  - 空壳类 / 空 enum class（未开放）→ class-body 只生成正文，不自动提交（mcapi-requests CI 会拒绝）

submit / edit 需要 GitHub token：环境变量 GH_TOKEN / GITHUB_TOKEN，或已登录的 gh CLI。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / ".cache" / "mcapi-requests"
LL_DIR = CACHE / "LeviLamina"
LAST = CACHE / "last.json"
REQ_REPO = "LiteLDev/mcapi-requests"
RAW = f"https://raw.githubusercontent.com/{REQ_REPO}/HEAD/.github/scripts/"
CI_SCRIPTS = ("check_request.py", "msvc_demangle.py")
PLATFORM_NAMES = {"server": "Server for Windows", "client": "Client for Windows"}
MAX_LINES = 64

# link.exe: ... (__imp_?foo@Bar@@QEAAXXZ) ...；lld-link /demangle:no: undefined symbol: __imp_?foo@...
LINK_RE = re.compile(r"LNK20(?:01|19).*?\(((?:__imp_)?\?[^\s()]+)\)")
LLD_RE = re.compile(r"undefined symbol: ((?:__imp_)?\?\S+)")
LLD_DEMANGLED_RE = re.compile(r"undefined symbol: (?!(?:__imp_)?\?)")

sys.stdout.reconfigure(encoding="utf-8")
sys.stderr.reconfigure(encoding="utf-8")


def die(msg: str) -> None:
    print(msg, file=sys.stderr)
    sys.exit(1)


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, check=True, **kw)


def decode(data: bytes) -> str:
    for enc in ("utf-8", "mbcs" if os.name == "nt" else "latin-1"):
        try:
            return data.decode(enc)
        except (UnicodeDecodeError, LookupError):
            pass
    return data.decode("utf-8", "replace")


# ---------------------------------------------------------------------------
# 准备：CI 脚本 + 最新 LeviLamina 头文件
# ---------------------------------------------------------------------------


def prepare() -> None:
    CACHE.mkdir(parents=True, exist_ok=True)
    for name in CI_SCRIPTS:
        with urllib.request.urlopen(RAW + name, timeout=30) as resp:
            (CACHE / name).write_bytes(resp.read())

    sparse = ["src/mc", "src-client/mc", "src-server/mc"]
    if not (LL_DIR / ".git").is_dir():
        print("首次克隆 LeviLamina 头文件（sparse），稍等...")
        run(["git", "clone", "--depth", "1", "--filter=blob:none", "--sparse",
             "https://github.com/LiteLDev/LeviLamina", str(LL_DIR)])  # fmt: skip
        run(["git", "-C", str(LL_DIR), "sparse-checkout", "set", *sparse])
    else:
        run(["git", "-C", str(LL_DIR), "fetch", "--depth", "1", "origin", "HEAD"])
        run(["git", "-C", str(LL_DIR), "reset", "--hard", "FETCH_HEAD"])
    commit = run(["git", "-C", str(LL_DIR), "rev-parse", "--short", "HEAD"], capture_output=True, text=True)
    print(f"LeviLamina 头文件: {commit.stdout.strip()}")

    if str(CACHE) not in sys.path:
        sys.path.insert(0, str(CACHE))


# ---------------------------------------------------------------------------
# 符号提取与分类
# ---------------------------------------------------------------------------


def detect_platform() -> str:
    conf = ROOT / ".xmake" / "windows" / "x64" / "xmake.conf"
    if conf.is_file():
        m = re.search(r'target_type\s*=\s*"(\w+)"', conf.read_text(encoding="utf-8", errors="replace"))
        if m and m.group(1) in PLATFORM_NAMES:
            return m.group(1)
    return "server"


def read_log(path: str | None) -> str:
    if path:
        return decode(Path(path).read_bytes())
    print("未指定 --log，执行 xmake -y 获取链接输出...")
    proc = subprocess.run(["xmake", "-y"], cwd=ROOT, capture_output=True)
    return decode(proc.stdout + proc.stderr)


def extract_symbols(log: str) -> list[str]:
    found = LINK_RE.findall(log) + LLD_RE.findall(log)
    if not found and LLD_DEMANGLED_RE.search(log):
        die(
            "lld-link 输出的是 demangle 后的名字，无法提取原始符号。\n"
            '请加 /demangle:no 后重新构建，例如: xmake f --ldflags="/demangle:no" --shflags="/demangle:no" -y && xmake -y'
        )
    seen, out = set(), []
    for sym in found:
        sym = sym.removeprefix("__imp_")
        if sym not in seen:
            seen.add(sym)
            out.append(sym)
    return out


def classify(symbols: list[str], platform: str) -> dict[str, list[dict]]:
    import check_request as cr
    from msvc_demangle import DemangleError, demangle

    index = cr.build_index(LL_DIR, PLATFORM_NAMES[platform])
    groups: dict[str, list[dict]] = {"request": [], "implemented": [], "ll": [], "invalid": []}
    for sym in symbols:
        try:
            if demangle(sym).scope[:1] == ["ll"]:
                groups["ll"].append({"symbol": sym, "message": "LeviLamina 自身的符号"})
                continue
        except DemangleError:
            pass
        problem = cr.check_symbol(sym, index)
        if problem is None:
            groups["request"].append({"symbol": sym, "message": "MCNAPI，可申请"})
        elif problem.implemented:
            groups["implemented"].append({"symbol": sym, "message": problem.message})
        else:
            groups["invalid"].append({"symbol": sym, "message": problem.message})
    return groups


def print_groups(groups: dict[str, list[dict]]) -> None:
    titles = {
        "request": "可申请（最新头文件中为 MCNAPI）",
        "implemented": "最新 LeviLamina 已提供（升级 LeviLamina 即可，勿申请）",
        "ll": "LeviLamina 符号（LL 版本与头文件不匹配，检查依赖版本）",
        "invalid": "无法申请（头文件中不存在或签名不匹配，检查自己的代码 / 目标端）",
    }
    for key, title in titles.items():
        items = groups[key]
        print(f"\n== {title}: {len(items)}")
        for item in items:
            print(f"  {item['symbol']}\n    {item['message']}")


# ---------------------------------------------------------------------------
# Issue
# ---------------------------------------------------------------------------


def mod_info() -> tuple[str, str]:
    tooth = json.loads((ROOT / "tooth.json").read_text(encoding="utf-8"))
    return "https://" + tooth["tooth"], tooth["info"]["name"]


def field_block(lines: list[str]) -> str:
    return "```text\n" + "\n".join(lines) + "\n```" if lines else "_No response_"


def build_body(url: str, platform: str, justification: str, classes: list[str], symbols: list[str]) -> str:
    return (
        f"### Mod\n\n{url}\n\n"
        f"### Platform\n\n{PLATFORM_NAMES[platform]}\n\n"
        f"### Justification\n\n{justification.strip()}\n\n"
        f"### Classes\n\n{field_block(classes)}\n\n"
        f"### Symbols\n\n{field_block(symbols)}\n"
    )


def split_request(classes: list[str], symbols: list[str]) -> list[tuple[list[str], list[str]]]:
    """每个 issue 的 Classes + Symbols 合计不超过 MAX_LINES 行。"""
    items = [("c", c) for c in classes] + [("s", s) for s in symbols]
    chunks = [items[i : i + MAX_LINES] for i in range(0, len(items), MAX_LINES)]
    return [([v for k, v in ch if k == "c"], [v for k, v in ch if k == "s"]) for ch in chunks]


def local_check(title: str, body: str) -> bool:
    """用 CI 同一脚本在本地校验，结果与 bot 一致。"""
    try:
        import tiktoken  # noqa: F401
    except ImportError:
        die("本地校验需要 tiktoken: pip install tiktoken")
    env = {**os.environ, "ISSUE_TITLE": title, "ISSUE_BODY": body, "PYTHONIOENCODING": "utf-8"}
    if token := get_token(required=False):
        env["GH_TOKEN"] = token
    proc = subprocess.run(
        [sys.executable, str(CACHE / "check_request.py"), str(LL_DIR), str(CACHE / "report.md")],
        cwd=CACHE, env=env, capture_output=True, text=True, encoding="utf-8",
    )  # fmt: skip
    print(proc.stdout or proc.stderr)
    return proc.returncode == 0


def get_token(required: bool = True) -> str | None:
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token and shutil.which("gh"):
        token = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True).stdout.strip() or None
    if required and not token:
        die("需要 GitHub token：设置 GH_TOKEN / GITHUB_TOKEN，或执行 gh auth login。")
    return token


def github(method: str, path: str, data: dict | None = None):
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "mcapi-request-script"}
    if token := get_token(required=method != "GET"):
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(
        f"https://api.github.com/{path}",
        data=json.dumps(data).encode() if data is not None else None,
        headers=headers,
        method=method,
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read() or b"null")
    except urllib.error.HTTPError as e:
        die(f"GitHub API {method} {path} 失败: HTTP {e.code} {e.read().decode(errors='replace')}")


def requested_items(args) -> tuple[list[str], list[str]]:
    """返回 (classes, symbols)。指定了 --class / --symbol 时只用指定的，否则用 check 的可申请结果。"""
    classes = list(dict.fromkeys(args.cls or []))
    if args.symbol:
        symbols = list(dict.fromkeys(s.removeprefix("__imp_") for s in args.symbol))
    elif classes:
        symbols = []
    elif LAST.is_file():
        symbols = [item["symbol"] for item in json.loads(LAST.read_text(encoding="utf-8"))["groups"]["request"]]
    else:
        die("没有可申请的内容：先运行 check，或用 --class / --symbol 指定。")
    return classes, symbols


# ---------------------------------------------------------------------------
# 类检查：未翻译成员 (UntypedStorage) / 未开放的空壳类、空 enum
# ---------------------------------------------------------------------------

PLATFORM_DIRS = {"server": ("src/mc", "src-server/mc"), "client": ("src/mc", "src-client/mc")}
COMMENT_RE = re.compile(r"//[^\n]*|/\*.*?\*/", re.S)


def header_files(platform: str) -> list[Path]:
    files = []
    for d in PLATFORM_DIRS[platform]:
        files += sorted((LL_DIR / d).rglob("*.h"))
    return files


def matching_brace(text: str, start: int) -> int:
    depth = 0
    for i in range(start, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return i
    return len(text)


DEF_HEAD_RE = re.compile(
    r"^\s*(?:template\s*<.*>\s*)?(enum\s+class|enum\s+struct|enum|class|struct|union)\s+(?:\[\[[^\]]*\]\]\s*)?"
    r"([\w:]+)\b[^;{()]*$",
    re.S,
)
NS_HEAD_RE = re.compile(r"^\s*(?:inline\s+)?namespace\s+([\w:]+)\s*$")


def find_definition(name: str, platform: str) -> dict | None:
    """按完整限定名查找类 / enum 定义（区分命名空间与外层类），返回文件、种类、类体。"""
    name = name.strip().removeprefix("::")
    last = name.split("::")[-1]
    for path in header_files(platform):
        raw = path.read_text(encoding="utf-8", errors="replace")
        if last not in raw:
            continue
        text = COMMENT_RE.sub(" ", raw)
        text = re.sub(r"^[ \t]*#[^\n]*", " ", text, flags=re.M)
        stack: list[str | None] = []  # 命名空间/类名；None 表示函数体等无名块
        pos = 0
        for m in re.finditer(r"[{};]", text):
            stmt = re.sub(r"^\s*(?:(?:public|protected|private)\s*:(?!:)\s*)+", "", text[pos : m.start()])
            pos = m.end()
            if m.group() == "}":
                if stack:
                    stack.pop()
                continue
            if m.group() == ";":
                continue
            if None in stack:
                stack.append(None)
                continue
            if ns := NS_HEAD_RE.match(stmt):
                stack.append(ns.group(1))
                continue
            rec = DEF_HEAD_RE.match(stmt)
            if not rec:
                stack.append(None)
                continue
            full = "::".join([*stack, rec.group(2)])
            if full == name:
                end = matching_brace(text, m.start())
                return {
                    "file": path.relative_to(LL_DIR).as_posix(),
                    "kind": re.sub(r"\s+", " ", rec.group(1)),
                    "body": text[m.end() : end],
                }
            stack.append(rec.group(2))
    return None


def analyze_body(kind: str, body: str) -> dict:
    untyped = re.findall(r"::ll::UntypedStorage<\s*\d+\s*,\s*\d+\s*>\s*\w+", body)
    if kind.startswith("enum"):
        return {"stub": not body.strip(), "untyped": [], "members": len(re.findall(r"\w+\s*=", body))}
    # 去掉内部类型的声明 / 空定义后，看是否还有函数、成员或基类信息
    rest = re.sub(r"\b(?:class|struct|union|enum(?:\s+class)?)\s+[\w:]+\s*(?::\s*[\w:]+\s*)?(?:\{\s*\}\s*)?;", "", body)
    rest = re.sub(r"\b(?:public|protected|private)\s*:", "", rest)
    rest = re.sub(r"\busing\s+[^;]*;", "", rest)
    has_decl = bool(re.search(r"\(|Storage<|\w+\s+m\w+\s*[;{=]", rest))
    return {"stub": not has_decl, "untyped": [re.sub(r"\s+", " ", u) for u in untyped], "members": None}


def find_references(name: str, platform: str, limit: int = 8) -> tuple[int, list[str]]:
    """统计其他头文件中把该类型用作参数、返回值、成员的位置，作为“它不是空的”的证据。"""
    last = name.split("::")[-1]
    use_re = re.compile(rf"::{re.escape(name if '::' in name else last)}\b(?!\s*\{{)")
    count, samples = 0, []
    for path in header_files(platform):
        raw = path.read_text(encoding="utf-8", errors="replace")
        if last not in raw:
            continue
        for line in raw.splitlines():
            s = line.strip()
            if use_re.search(s) and re.search(r"MC\w*API|Storage<|\(", s) and not s.startswith(("//", "#", "class ", "struct ")):
                count += 1
                if len(samples) < limit:
                    samples.append(f"{path.relative_to(LL_DIR).as_posix()}: {s[:160]}")
    return count, samples


def inspect_class(name: str, platform: str) -> dict:
    other = "client" if platform == "server" else "server"
    d = find_definition(name, platform)
    info: dict = {"name": name, "platform": platform, "definition": d}
    if d is None:
        info["category"] = "missing"
        return info
    info.update(analyze_body(d["kind"], d["body"]))
    if info["untyped"]:
        info["category"] = "untyped"
    elif info["stub"]:
        info["category"] = "stub"
        od = find_definition(name, other)
        info["other_platform"] = (
            {"file": od["file"], **analyze_body(od["kind"], od["body"])} if od else None
        )
        info["ref_count"], info["references"] = find_references(name, platform)
    else:
        info["category"] = "complete"
    return info


def print_class_info(info: dict) -> None:
    name, d = info["name"], info["definition"]
    print(f"\n== {name} ({PLATFORM_NAMES[info['platform']]})")
    if d is None:
        print("  头文件中找不到定义：检查拼写、命名空间，或该类型本身只有前置声明。")
        return
    print(f"  {d['kind']} @ {d['file']}")
    cat = info["category"]
    if cat == "untyped":
        print(f"  未翻译成员 {len(info['untyped'])} 个 → 可用 submit --class {name} 申请（mcapi-requests）：")
        for u in info["untyped"][:20]:
            print(f"    {u}")
        if len(info["untyped"]) > 20:
            print(f"    ... 共 {len(info['untyped'])} 个")
    elif cat == "complete":
        print("  成员已全部翻译，无需申请。")
    else:
        print("  空壳：没有函数和成员（enum 无枚举值）。")
        op = info.get("other_platform")
        if op and not op["stub"]:
            print(f"  另一端有完整定义 ({op['file']})，说明该类型在本端未开放或本端确实不用。")
        print(f"  其他头文件引用 {info['ref_count']} 处（证明它实际有内容）：")
        for r in info["references"]:
            print(f"    {r}")
        print(f"  → 确认确实有内容后，用 class-body {name} --justification \"...\" 生成开放申请正文。")


def cmd_class_info(args) -> None:
    prepare()
    platform = args.platform or detect_platform()
    for name in args.names:
        print_class_info(inspect_class(name, platform))


def cmd_class_body(args) -> None:
    prepare()
    platform = args.platform or detect_platform()
    url, mod_name = mod_info()
    infos = [inspect_class(n, platform) for n in args.names]
    bad = [i["name"] for i in infos if i["category"] != "stub"]
    if bad:
        for i in infos:
            if i["category"] != "stub":
                print_class_info(i)
        die(f"以下类型不是空壳，不适用开放申请: {', '.join(bad)}（有 UntypedStorage 的用 submit --class）")

    lines = [
        f"# Request to expose {', '.join(f'`{i['name']}`' for i in infos)}",
        "",
        f"- **Mod**: {url} ({args.mod_name or mod_name})",
        f"- **Platform**: {PLATFORM_NAMES[platform]}",
        "",
        "## Justification",
        "",
        args.justification.strip(),
        "",
        "## Details",
    ]
    for i in infos:
        d = i["definition"]
        lines += ["", f"### `{i['name']}`", "", f"Currently an empty {d['kind']} stub in `{d['file']}`."]
        op = i.get("other_platform")
        if op and not op["stub"]:
            lines.append(f"It is fully defined on the other platform in `{op['file']}`.")
        if i["references"]:
            lines += [
                f"It is referenced {i['ref_count']} times in other headers, so it is not actually empty, e.g.:",
                "",
                "```cpp",
                *[r.split(": ", 1)[1] for r in i["references"]],
                "```",
            ]
    text = "\n".join(lines) + "\n"
    out = CACHE / f"class-request-{'_'.join(re.sub(r'\W+', '_', i['name']) for i in infos)}.md"
    out.write_text(text, encoding="utf-8")
    print(text)
    print(f"已保存到 {out.relative_to(ROOT)}（未提交，由用户决定发往何处）")


def last_platform(args) -> str:
    if args.platform:
        return args.platform
    if LAST.is_file():
        return json.loads(LAST.read_text(encoding="utf-8"))["platform"]
    return detect_platform()


# ---------------------------------------------------------------------------
# 子命令
# ---------------------------------------------------------------------------


def cmd_check(args) -> None:
    platform = args.platform or detect_platform()
    symbols = extract_symbols(read_log(args.log))
    if not symbols:
        die("日志中没有找到未解析符号。可用 xmake -r -y > build.log 2>&1 保存完整输出后用 --log 指定。")
    prepare()
    print(f"平台: {PLATFORM_NAMES[platform]}，未解析符号: {len(symbols)}")
    groups = classify(symbols, platform)
    print_groups(groups)
    LAST.write_text(json.dumps({"platform": platform, "groups": groups}, ensure_ascii=False, indent=2), "utf-8")
    print(f"\n结果已保存到 {LAST.relative_to(ROOT)}")


def cmd_submit(args) -> None:
    classes, symbols = requested_items(args)
    if not classes and not symbols:
        die("没有需要申请的类或符号。")
    prepare()
    platform = last_platform(args)
    url, name = mod_info()
    title = f"Request symbols for {args.mod_name or name}"
    bodies = [build_body(url, platform, args.justification, c, s) for c, s in split_request(classes, symbols)]

    for i, body in enumerate(bodies, 1):
        print(f"\n--- Issue {i}/{len(bodies)}: {title}\n{body}")
        if not local_check(title, body):
            die("本地 CI 校验未通过，按上方报告修改后重试（不会提交）。")
    if args.dry_run:
        print("--dry-run：校验通过，未提交。")
        return
    for body in bodies:
        issue = github("POST", f"repos/{REQ_REPO}/issues", {"title": title, "body": body})
        print(f"已提交 #{issue['number']}: {issue['html_url']}")
    print("bot 会在 1~2 分钟内评论检查结果，用 status <issue> 查看。")


def cmd_status(args) -> None:
    issue = github("GET", f"repos/{REQ_REPO}/issues/{args.issue}")
    print(f"#{args.issue} {issue['title']}\n状态: {issue['state']} ({issue.get('state_reason')})\n{issue['html_url']}")
    comments = github("GET", f"repos/{REQ_REPO}/issues/{args.issue}/comments?per_page=100")
    bot = [c for c in comments if c["user"]["login"] == "github-actions[bot]"]
    print("\n" + (bot[-1]["body"] if bot else "bot 尚未评论，稍后再查。"))


def cmd_edit(args) -> None:
    classes, symbols = requested_items(args)
    prepare()
    issue = github("GET", f"repos/{REQ_REPO}/issues/{args.issue}")
    url, name = mod_info()
    title = f"Request symbols for {args.mod_name or name}"
    chunks = split_request(classes, symbols)
    if len(chunks) > 1:
        die(f"Classes + Symbols 超过 {MAX_LINES} 行，一个 issue 放不下，请拆分。")
    body = build_body(url, last_platform(args), args.justification, *chunks[0])
    print(body)
    if not local_check(title, body):
        die("本地 CI 校验未通过，按上方报告修改后重试（不会提交）。")
    github("PATCH", f"repos/{REQ_REPO}/issues/{args.issue}", {"title": title, "body": body})
    print(f"已更新 {issue['html_url']}，bot 校验通过后会自动重新打开被它关闭的 issue。")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("check", help="提取未解析符号并分类")
    p.add_argument("--log", help="构建日志文件；不指定则执行 xmake -y")
    p.add_argument("--platform", choices=PLATFORM_NAMES)
    p.set_defaults(func=cmd_check)

    for name, func in (("submit", cmd_submit), ("edit", cmd_edit)):
        p = sub.add_parser(name, help="提交申请" if name == "submit" else "修改已提交的 issue")
        if name == "edit":
            p.add_argument("issue", type=int)
        p.add_argument("--justification", required=True, help="模组用途及需要这些符号的原因，英文，≤200 tokens")
        p.add_argument("--symbol", action="append", help="手动指定符号（可多次），默认使用 check 的可申请结果")
        p.add_argument("--class", dest="cls", action="append", help="申请翻译 UntypedStorage 成员的类（可多次）")
        p.add_argument("--platform", choices=PLATFORM_NAMES)
        p.add_argument("--mod-name", help="标题中的模组名，默认 tooth.json info.name；须出现在 README.md 中")
        if name == "submit":
            p.add_argument("--dry-run", action="store_true", help="只做本地校验，不提交")
        p.set_defaults(func=func)

    p = sub.add_parser("class-info", help="检查类型：未翻译成员 / 空壳类或空 enum / 已完整")
    p.add_argument("names", nargs="+", help="完整类名，如 Player、Actor::BuiltInComponents")
    p.add_argument("--platform", choices=PLATFORM_NAMES)
    p.set_defaults(func=cmd_class_info)

    p = sub.add_parser("class-body", help="为空壳类 / 空 enum 生成开放申请正文（不提交）")
    p.add_argument("names", nargs="+")
    p.add_argument("--justification", required=True, help="为什么需要这个类型，英文")
    p.add_argument("--platform", choices=PLATFORM_NAMES)
    p.add_argument("--mod-name")
    p.set_defaults(func=cmd_class_body)

    p = sub.add_parser("status", help="查看 issue 状态和 bot 校验结果")
    p.add_argument("issue", type=int)
    p.set_defaults(func=cmd_status)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
