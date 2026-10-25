# 设计

目标：一条命令找出本机仍在用 SSH 连 `bitbucket.org` 的地方，预览后改写成 `ssh.bitbucket.org`。默认只读。

## 命令行

```
bbhost-check [--root DIR]... [--no-machine] [--format text|json|markdown] [--fix [--yes]]
```

| 参数 | 含义 |
|---|---|
| `--root DIR` | 要扫的目录，可重复，默认当前目录 |
| `--no-machine` | 不扫机器级配置（`~/.ssh`、全局和系统 gitconfig），只扫 `--root` |
| `--format` | `text`（默认）、`json`、`markdown`（可直接贴进 PR） |
| `--fix` | 打印差异，询问确认后写入；不加就是只读 |
| `--yes` | 跳过确认（只和 `--fix` 一起用） |

退出码：`0` 没有命中；`1` 有命中（`--fix` 之后按剩下的算）；`2` 工具自身出错。CI 里可以直接当检查用。

没有子命令，没有配置文件。

## 扫描范围

### 机器级（`--no-machine` 关闭）

| 文件 | 规则 |
|---|---|
| `$HOME/.gitconfig`、`$XDG_CONFIG_HOME/git/config`（默认 `~/.config/git/config`） | gitconfig 规则 |
| 系统 gitconfig：遵守 `GIT_CONFIG_NOSYSTEM` / `GIT_CONFIG_SYSTEM`；否则用 `git config --system --list --show-origin` 报出的文件，再加上存在的 `/etc/gitconfig` | gitconfig 规则 |
| `$HOME/.ssh/config` | ssh config 规则 |
| `$HOME/.ssh/known_hosts` | known_hosts 规则 |

### `--root` 下递归

跳过 `node_modules`、`target`、`.venv`、`venv` 目录，不跟随符号链接。

| 文件 | 规则 |
|---|---|
| `.git/config`（含 remote 的 `url`、`pushurl`，submodule 的 `url`，`[url "..."]` 段） | gitconfig 规则 |
| `.git/modules/**/config`（子模块自己的 remote） | gitconfig 规则 |
| 裸仓库（目录名以 `.git` 结尾且有 `HEAD`、`config`、`objects/`）的 `config` | gitconfig 规则 |
| `.gitmodules` | URL 规则 |
| `bitbucket-pipelines.yml`、`.github/workflows/*.yml\|yaml`、`.gitlab-ci.yml`、`Jenkinsfile*` | URL 规则 + ssh-keyscan 规则 |
| `package.json` | URL 规则 + npm 简写规则 |
| `package-lock.json`、`npm-shrinkwrap.json`、`yarn.lock`、`pnpm-lock.yaml`、`Cargo.toml`、`Cargo.lock`、`go.mod`、`go.sum`、`Gemfile`、`Gemfile.lock`、`pyproject.toml`、`poetry.lock`、`uv.lock`、`requirements*.txt` | URL 规则 |

说明：
- manifest（`package.json`、`Cargo.toml`、`Gemfile`、`pyproject.toml`、`requirements*.txt`）也扫，因为只改 lockfile 下次 install 会被 manifest 改回去。
- `go.sum` / `go.mod` 里是模块路径（`bitbucket.org/ws/repo`），本身不是 SSH 地址，正常不会命中；Go 走 SSH 靠的是 gitconfig 里的 `insteadOf`，那一处由 gitconfig 规则负责。
- 不扫 README 等文档。

## 规则

所有规则都是按行处理，命中记录 `文件:行号`、原行、改写后的行（不可自动改的为空）。

### URL 规则

在一行里找每个 `bitbucket.org`，同时满足下面条件才算 SSH 地址：

1. 前一个字符是 `@`，或前面是 `//`；再往前不是 `.` 之类的主机名字符（所以 `ssh.bitbucket.org`、`altssh.bitbucket.org`、`mybitbucket.org` 不会命中）。
2. 往回找到这个 URL 的起点（空白、引号、`=`、`(`、`[`、`{`、`,` 为界）。
   - 有 `://`：协议是 `ssh`、`git+ssh`、`ssh+git` 才算；`https`、`http`、`git` 不算。
   - 没有 `://`：只认 scp 写法 `user@bitbucket.org:`，或者 `git@bitbucket.org` 后面直接结束（`ssh -T git@bitbucket.org`）。
3. 后一个字符是 `:`、`/`，或第 2 条里的结束情形。

改写：在 `bitbucket.org` 前插入 `ssh.`。用户名、端口、路径、`#sha` 原样保留。

### gitconfig 规则

URL 规则，外加一条：`insteadOf` / `pushInsteadOf` 这一行的值是“被替换的模式”，连接实际走的是段头里的地址。所以这两种行跳过，只看段头 `[url "..."]`。这样 `[url "git@ssh.bitbucket.org:"] insteadOf = git@bitbucket.org:`（评论区的两行方案）不会被误报，而 `[url "git@bitbucket.org:"] insteadOf = https://bitbucket.org/`（常见的 Go 私有模块配置）会命中段头。

如果在任何被扫的 gitconfig 里发现了“旧 → 新”的 `insteadOf`，汇总里加一句提示：本机 git 已经被改写，剩下的 remote 命中在本机能用，但 CI、容器构建和直接解析 URL 的包管理器不读这份配置。命中照常列出，不隐藏。

### ssh config 规则

按 `Host` / `Match` 分段，关键字不区分大小写，支持 `Key=Value`：

| 情况 | 处理 |
|---|---|
| `Host` 的模式里有 `bitbucket.org`，段内没有 `HostName`，且整个文件没有任何段的模式包含 `ssh.bitbucket.org` | 命中 `Host` 行，改成在模式后追加 ` ssh.bitbucket.org`，让 IdentityFile 等设置在 remote 改名后继续生效 |
| 任何段的 `HostName bitbucket.org` | 命中该行，值改成 `ssh.bitbucket.org` |
| `Host bitbucket.org` 且 `HostName ssh.bitbucket.org` | 已经迁好，不报 |

`Include` 引入的文件不跟进（在已知限制里写明）。

### known_hosts 规则

调用 `ssh-keygen -F <host> -f <file>`（能处理 `HashKnownHosts` 哈希过的条目）。有 `bitbucket.org` 条目、没有 `ssh.bitbucket.org` 条目时命中一条。修复是在文件末尾追加 `ssh.bitbucket.org <keytype> <key>`，公钥取自本机已有的 `bitbucket.org` 条目（公告说主机密钥不变）。不联网，不内置公钥。找不到 `ssh-keygen` 时在 stderr 报错并跳过这一项，汇总里也会写出来。

### ssh-keyscan 规则（CI 文件）

同一行里在 `ssh-keyscan` 后面出现独立的 `bitbucket.org` 记号，改成 `ssh.bitbucket.org`。

### npm 简写规则（package.json）

`"bitbucket:ws/repo"` 只报不改：npm 解析 SSH 时会展开成旧主机名，需要用户自己改成 `git+ssh://git@ssh.bitbucket.org/ws/repo.git#tag`，并重新生成 lockfile。

### GCM 版本检查（机器级，`--no-machine` 关闭）

依据 Bitbucket Cloud changelog 2026-10-13：11-30 起，经 GCM 3.0.0 以下版本、用 OAuth 2.0 认证的 Git 请求会被拦截；GCM 配 PAT / app password 的不受影响。

只在同时满足下面四条时报一条（类别 `credential helper`，只报不改）：

1. 扫描到的仓库配置、`.gitmodules`、gitconfig 或依赖文件里有 `https://bitbucket.org`（`insteadOf = https://bitbucket.org/` 这一行是把 HTTPS 改走 SSH，不算）。
2. 用 `git config --show-origin --get-regexp '^credential\..*helper$'` 按 git 的规则拼出访问 `https://bitbucket.org` 时的 helper 列表：通用的 `credential.helper` 和 `credential.<url>.helper`（url 的主机是 bitbucket.org）按配置顺序排列，空值清空前面的。列表里有 GCM（`manager`、`manager-core`，或以 `git-credential-manager[-core][.exe]` 结尾的路径）。
3. `GCM_BITBUCKET_AUTHMODES`（优先）或 `credential.bitbucketAuthModes` 没设，或包含 `oauth`。
4. 按 git 启动 helper 的方式取版本：裸名字 `x` 跑 `git credential-x --version`，路径直接跑 `<路径> --version`。输出里第一个 `X.Y.Z` 低于 3.0.0。

找不到 GCM（命令不存在、`is not a git command`）当作没装，不报。GCM 在但读不出版本，作为警告输出，退出码 2。只调 `git config` 和 `--version`，不碰 `get`/`store`，不读凭据。

提示里的升级办法按编译平台给出；Windows 要写明 Git for Windows 2.56.0.windows.3 仍捆绑 GCM 2.9.x（git-for-windows/git#6464 截至 10-25 仍 open），需要单独安装 GCM 3.0.1+。

## 输出

- `text`：按文件分组，每条命中给出类别、`路径:行号`、`-` 原行、`+` 新行；最后一行汇总（命中数、可自动修复数、文件数）。
- `json`：`{"findings":[{"kind","path","line","old","new","fixable","note"}],"summary":{...},"notes":[...]}`，`new` 不可修复时为 `null`。
- `markdown`：标题 + 表格（类别 / 位置 / 现在 / 改成），末尾附官方公告链接。

## `--fix`

1. 先完整扫描。
2. 打印每个文件的差异（只有命中行）。
3. 询问 `Apply N change(s) to M file(s)? [y/N]`。标准输入不是终端又没有 `--yes` 时直接报错退出，不会默默写入。
4. 对每个文件：
   - 重新读取，检查每个命中行还和扫描时一样；不一样就跳过整个文件并报错（文件在扫描后被改过）。
   - 先复制一份备份 `<文件>.bbhost-check-<unix 时间戳>.bak`。
   - 只替换命中的那几行（known_hosts 是在末尾追加），保留原来的换行符；写到同目录临时文件再 rename，保留文件权限。
5. 再扫一遍，按剩下的命中算退出码。

只改本地文件。不调 Bitbucket API，不改远端，不碰 HTTPS 地址，不需要任何凭据。

## 代码结构

```
src/main.rs      参数解析、调度、退出码
src/discover.rs  找文件、判断类别
src/rules.rs     URL / gitconfig / ssh-keyscan / npm 简写规则
src/sshconfig.rs ssh config 分段规则
src/knownhosts.rs
src/gcm.rs       GCM 版本检查
src/report.rs    text / json / markdown
src/fix.rs       预览、确认、备份、写入
```

依赖：`clap`、`serde`、`serde_json`、`walkdir`；测试用 `tempfile`。

## 已知限制

- GCM 检查分不清用户实际选的是 OAuth 还是 app password（除非 `bitbucketAuthModes` 写明），也只按全局配置判断一次，不逐个仓库算 helper。
- 不跟进 ssh config 的 `Include`，不解析 `Match` 段。
- scp 写法必须带用户名（`git@...`）；`bitbucket.org:ws/repo` 这种省略用户名、靠 ssh config 补 `User` 的写法不报。
- 不扫 Dockerfile、Terraform、Ansible 等（第一版范围外）；可用 `--format json` 的结果配合 `grep` 自查。
- lockfile 改写后建议重新跑一次 install，让包管理器自己校验。
