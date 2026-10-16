"""创建 GitHub Release 并上传构建产物.

用途: 把已构建的 exe 发布到 GitHub Releases.
认证: 通过 git 的凭据助手读取 token（与 git push 使用同一凭据，无需额外配置）.

前置:
  - 已构建 app/src-tauri/target/release/mechrevo-max.exe
  - 已在 git 里配置好可推送的 GitHub 凭据
  - 对应 tag 已存在（或本脚本会用 target_commitish 创建）

用法:
  python -X utf8 -I tools/publish_release.py <tag> [--title 标题] [--notes-file 文件]
  python -X utf8 -I tools/publish_release.py v0.1.0 --title "v0.1.0 — 首个可用版本" --notes-file CHANGELOG.md

退出码: 0 成功 / 1 失败 / 2 前置不满足
"""
from __future__ import annotations

import json
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "lza6/Mechrevo-Max"
ARTIFACT = ROOT / "app" / "src-tauri" / "target" / "release" / "mechrevo-max.exe"


def get_token() -> str | None:
    """通过 git 凭据助手读取 GitHub token（不落盘、不打印）."""
    try:
        out = subprocess.run(
            ["git", "credential", "fill"],
            input="protocol=https\nhost=github.com\n\n",
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None

    for line in out.stdout.splitlines():
        if line.startswith("password="):
            return line[len("password="):].strip()
    return None


def api(method: str, url: str, token: str, payload: dict | None = None) -> tuple[int, dict]:
    """调用 GitHub API，返回 (状态码, 响应体)."""
    data = json.dumps(payload).encode() if payload else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"token {token}")
    req.add_header("Accept", "application/vnd.github+json")
    if data:
        req.add_header("Content-Type", "application/json")

    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            return resp.status, json.loads(resp.read() or b"{}")
    except urllib.error.HTTPError as e:
        body = e.read()
        try:
            return e.code, json.loads(body)
        except json.JSONDecodeError:
            return e.code, {"message": body.decode("utf-8", "replace")[:500]}
    except urllib.error.URLError as e:
        return 0, {"message": f"网络错误: {e.reason}"}


def upload_asset(upload_url: str, token: str, path: Path) -> tuple[int, dict]:
    """上传 Release 资源（二进制流）."""
    data = path.read_bytes()
    # upload_url 形如 https://uploads.github.com/.../assets{?name,label}
    url = f"{upload_url}?name={urllib.parse.quote(path.name)}"
    req = urllib.request.Request(url, data=data, method="POST")
    req.add_header("Authorization", f"token {token}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("Content-Type", "application/octet-stream")
    req.add_header("Content-Length", str(len(data)))

    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            return resp.status, json.loads(resp.read() or b"{}")
    except urllib.error.HTTPError as e:
        body = e.read()
        try:
            return e.code, json.loads(body)
        except json.JSONDecodeError:
            return e.code, {"message": body.decode("utf-8", "replace")[:500]}


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2

    tag = sys.argv[1]
    args = sys.argv[2:]
    title = tag
    notes_file: Path | None = None
    upload = True

    if "--title" in args:
        i = args.index("--title")
        title = args[i + 1]
    if "--notes-file" in args:
        i = args.index("--notes-file")
        notes_file = Path(args[i + 1])
    if "--no-upload" in args:
        upload = False

    if not ARTIFACT.exists():
        print(f"❌ 找不到构建产物: {ARTIFACT}")
        print("   请先运行: cd app/src-tauri && cargo build --release")
        return 2

    token = get_token()
    if not token:
        print("❌ 无法从 git 凭据助手读取 GitHub token")
        print("   请确认 git push 可用（凭据管理器里应有 git:https://github.com）")
        return 2
    print(f"凭据已就绪 (token {len(token)} 字符)")

    body = ""
    if notes_file and notes_file.exists():
        body = notes_file.read_text(encoding="utf-8")
        print(f"Release 说明: {notes_file} ({len(body)} 字符)")

    # 1) 创建 Release
    print(f"\n创建 Release {tag} …")
    status, resp = api(
        "POST",
        f"https://api.github.com/repos/{REPO}/releases",
        token,
        {
            "tag_name": tag,
            "name": title,
            "body": body,
            "draft": False,
            "prerelease": False,
        },
    )
    if status == 422 and "already_exists" in str(resp):
        print("  Release 已存在，改为查询现有的…")
        status, resp = api(
            "GET", f"https://api.github.com/repos/{REPO}/releases/tags/{tag}", token
        )
    if status not in (200, 201):
        print(f"❌ 创建失败 (HTTP {status}): {resp.get('message')}")
        if resp.get("errors"):
            print(f"   {resp['errors']}")
        return 1

    print(f"✅ Release: {resp.get('html_url')}")
    upload_url = resp["upload_url"].split("{")[0]

    if not upload:
        return 0

    # 2) 上传产物
    size_mb = ARTIFACT.stat().st_size / 1024 / 1024
    print(f"\n上传 {ARTIFACT.name} ({size_mb:.2f} MB) …")
    status, up = upload_asset(upload_url, token, ARTIFACT)
    if status not in (200, 201):
        print(f"❌ 上传失败 (HTTP {status}): {up.get('message')}")
        return 1

    print(f"✅ 产物已上传: {up.get('browser_download_url')}")
    print(f"   大小: {up.get('size', 0) / 1024 / 1024:.2f} MB")
    return 0


if __name__ == "__main__":
    # urllib.parse 是 urllib.request 的隐式依赖，显式导入以通过静态检查
    import urllib.parse  # noqa: E402

    sys.exit(main())
