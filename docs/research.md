# 调研（2026-10-23）

## 1. 官方公告原文要点

来源：Atlassian Community，Apurva Paralkar，《Upcoming change to Bitbucket Cloud SSH access: move from bitbucket.org to ssh.bitbucket.org》
https://community.atlassian.com/forums/Bitbucket-articles/Upcoming-change-to-Bitbucket-Cloud-SSH-access-move-from/ba-p/3234032
（本轮 2026-10-23 23:08 重新抓取页面全文，下面是对原文的概括，不是逐字引用。）

| 问题 | 公告怎么说 |
|---|---|
| 改什么 | `bitbucket.org` 以后只服务 HTTPS（网页和 API），所有 Git over SSH 改走 `ssh.bitbucket.org` |
| 原定截止 | 2026-11-12 之后 SSH 连接 `bitbucket.org` 会被拒绝 |
| 10-21 更新 | 页面顶部写着 “Update as of October 21, 2026”：从 11-12 起改为宽限期，期间按工作日做 brownout（每天 UTC 00/06/12/18 四个窗口，时长 15 分钟 → 30 分钟 → 1 小时 → 2 小时 → 3 小时，跳过周末和节假日），表格最后一行是 “Dec 10, 2026 Final removal”。brownout 期间会临时屏蔽 `bitbucket.org` 的 SSH 并返回明确的报错 |
| 写法 | 原文给的旧写法是 `git@bitbucket.org:<workspace>/<repo>.git`，新写法 `git@ssh.bitbucket.org:<workspace>/<repo>.git`；让大家在配置里搜 `git@bitbucket.org:` 和 `ssh://git@bitbucket.org` |
| 端口 | 防火墙需放行 `ssh.bitbucket.org` 的 22 端口；22 被挡时按官方 “Port 22 is blocked on local network” 文档用备用主机名和端口（即 `altssh.bitbucket.org:443`，公告没有说 altssh 是否受影响，评论区有人问，官方未回复） |
| 主机密钥 | 新主机名沿用原来的主机密钥，公告列出了三个 SHA256 指纹（ECDSA `FC73VB6C…`、ED25519 `ybgmFkzw…`、RSA `46OSHA1R…`） |
| known_hosts | 第一次连接会提示确认指纹；自动化环境可以预先写入，公告给的命令是 `curl -s https://bitbucket.org/site/ssh \| sed 's/bitbucket.org/ssh.bitbucket.org/' >> ~/.ssh/known_hosts` |
| HTTPS | 明确写 “HTTPS access is not affected”，`https://bitbucket.org/<workspace>/<repo>.git` 照常可用 |
| 要改的地方 | 本地 remote、CI/CD、部署脚本、构建机、镜像同步任务、所有跑 `git clone/fetch/push` 的内部工具 |

关于 12-10：前一轮复核（`20261023-1914-revalidate`）把 “12-10 最终移除” 记为未核实。本轮直接从公告页面正文读到了 10-21 的更新和 brownout 表，表的最后一行就是 12-10 Final removal。所以 12-10 现在有官方出处，但日期已经改过一次，文档里一律写“按官方公告（10-21 更新）”并附链接。是否在 README 里写 12-10 交给用户决定，见交接说明。

### 已知问题的当前状态（Jira REST API，2026-10-23 查）

| Issue | 标题 | 状态 |
|---|---|---|
| [BAM-26573](https://jira.atlassian.com/browse/BAM-26573) | Bamboo does not use new Bitbucket Cloud SSH URL | Closed / Fixed，修复版本 13.0.1、12.1.12、10.2.24（10-12 更新）。老版本 Bamboo 仍然不行，需要升级 |
| [BCLOUD-24132](https://jira.atlassian.com/browse/BCLOUD-24132) | Pipelines 默认 known_hosts 加入 ssh.bitbucket.org | Closed / Fixed（10-22 更新） |

任务说明里写的“Bamboo 还不支持新地址”已经过时：只有未升级到上述版本的 Bamboo 才有问题。

## 2. 迁移范围（工具据此设计）

需要改的是 **SSH 方式** 访问 `bitbucket.org` 的地方：

- scp 写法：`git@bitbucket.org:ws/repo.git`
- URL 写法：`ssh://git@bitbucket.org/ws/repo.git`、带端口 `ssh://git@bitbucket.org:22/...`、npm/pip 的 `git+ssh://git@bitbucket.org/...`
- `~/.ssh/config` 里 `Host bitbucket.org` 段：remote 改成新主机名后，这一段的 `IdentityFile` 等设置不再生效（评论区 Anders Harder、Jason Rumney 都踩到了 “Permission denied (publickey)”）；`HostName bitbucket.org` 的别名段也要改
- `known_hosts`：主机密钥不变，但条目按主机名匹配，要为 `ssh.bitbucket.org` 再加一份
- CI 里的 `ssh-keyscan bitbucket.org`（预置 known_hosts 的常见写法）

不需要改的：`https://bitbucket.org/...`（包括 `https://user@bitbucket.org/...`）、`altssh.bitbucket.org`（公告未提，不动）、已经是 `ssh.bitbucket.org` 的。

## 3. 查重（GitHub 仓库搜索，拆词取交集，2026-10-23）

每组分别查“不限时间”和 `created:>2026-10-15`：

| 关键词 | 不限时间 | 10-15 以后 |
|---|---|---|
| `bitbucket ssh migrate` | 1：mshassium/bitbucket-repo-migrator（0★，2025-10，搬仓库，不相关） | 0 |
| `ssh.bitbucket.org` | 16：都是 2012–2020 年碰巧带这个字符串的旧仓库 | 0 |
| `bitbucket hostname ssh` | 0 | 0 |
| `bitbucket remote rewrite` | 0 | 0 |
| `bitbucket ssh scanner` | 0 | 0 |
| `bitbucket known_hosts` | 2：2014、2016 年的 Ansible role | 0 |

网页搜索 “ssh.bitbucket.org migration tool script scan repositories” 只返回 Bitbucket → GitHub 的搬迁工具。结论：到 10-23 为止仍然没有同类工具。另外 GitHub 代码搜索 `"ssh.bitbucket.org" insteadOf` 有 24 个文件，说明已经有人在 dotfiles 里用全局 `insteadOf` 绕过去了。

## 4. 用户在评论区贴的迁移办法

| 来源 | 做法 | 覆盖 | 没覆盖 |
|---|---|---|---|
| Matthias Gaiser | `git remote get-url origin` → `sed -E` 替换 → `git remote set-url origin` | 单个仓库的 `origin`；正则同时处理 scp 和 `//` 写法 | 其他 remote、`pushurl`、子模块、多个仓库 |
| Ian Hunter | 两行全局 `git config --global url."git@ssh.bitbucket.org:".insteadOf "git@bitbucket.org:"`（以及 `ssh://` 版本） | 本机所有走 git 的操作，包括子模块 | CI、Docker 构建、别人的机器；npm/cargo 等直接解析 URL 的工具不一定读 gitconfig；lockfile 里的地址还是旧的 |
| ldaughenbaugh | bash 脚本：`find -name .git`，遍历每个仓库的所有 remote，只改 `git@bitbucket.org:` 和 `ssh://git@bitbucket.org/` 开头的，带 `--dry-run` | 多仓库、多 remote，有预览 | `pushurl`、`.gitmodules`、`.git/modules/*/config`、ssh config、known_hosts、CI、lockfile；没有备份 |
| matthewrhamilton / istvan.verhas | `~/.ssh/config` 里 `Host bitbucket.org` 加 `HostName ssh.bitbucket.org` | 本机所有 SSH 连接，不改任何仓库 | 同全局 insteadOf，只管本机 |
| Anders Harder | 把 `Host bitbucket.org` 段复制一份为 `Host ssh.bitbucket.org` | 改 remote 后 IdentityFile 还能用 | 需要手工做 |
| 官方 known_hosts 命令 | `curl .../site/ssh \| sed ...` | 自动化环境 | Nathan Phillip Brink 指出：等官方把 `/site/ssh` 改成新主机名后，这个 sed 会产出 `ssh.ssh.bitbucket.org` |

评论区还提到的坑（工具的扫描项都来自这里）：
- npm `bitbucket:` 简写会被展开成 `git+ssh://git@bitbucket.org/...` 写进 `package-lock.json`（Falk Köppe、Gregor Sondermeier）。
- 子模块写死了旧地址，老 tag 重新构建会坏（mjkaldin、martonmiklosqdev）。
- “Clone this repository” 对话框和官方文档仍给旧地址（多人反映，截至评论时）。
- Jenkins 上百个 job、GitKraken 按主机名选密钥。
- Pipelines 提供的 `BITBUCKET_GIT_SSH_ORIGIN` 变量会不会更新，官方未回复。

## 5. 对工具的启示

1. 本机问题两行 `insteadOf` 就能挡住，工具的价值在于**找全**：CI、lockfile、子模块、ssh config、known_hosts 这些 `insteadOf` 管不到的地方。README 要老实说明这一点。
2. 已经设了 `insteadOf` 或 ssh config 跳转的人，扫描结果会有很多 remote 命中。不把这些当成“已解决”隐藏掉（CI 和别人的机器不读你的 gitconfig），但在汇总里提示一句。
3. `insteadOf = git@bitbucket.org:` 这一行本身就是解决方案，不能当成命中，否则会把用户的修复改坏。
4. 匹配必须区分 HTTPS：`https://x@bitbucket.org/` 不能动。
5. known_hosts 不联网、不硬编码公钥：主机密钥不变，直接复用本机已信任的 `bitbucket.org` 公钥，给 `ssh.bitbucket.org` 追加一份。
