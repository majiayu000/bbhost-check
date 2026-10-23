# 发布步骤（待用户确认，尚未执行）

建议 **10-27 前公开**：11-12 第一次 brownout 前留两周，让搜 “bitbucket ssh.bitbucket.org” 的人能找到。

1. 在 GitHub 建 `majiayu000/bbhost-check`（public），push `main`。
2. 加 CI（`cargo test`、`cargo clippy -- -D warnings`、MSRV 1.88，Linux + macOS），绿了再继续。
3. 打 `v0.1.0` tag，Release 附 Linux x86_64、macOS arm64/x86_64 二进制和 sha256；README 安装段补上下载方式。
4. `cargo publish`（crates.io 上 `bbhost-check` 这个名字需要发布前再确认；要用户自己 `cargo login`）。
5. 仓库 topics：bitbucket、bitbucket-cloud、ssh、git、migration、known-hosts、devops、rust、cli。
6. 推广（每一条都要用户确认后再发）：
   - Atlassian 公告帖下回复一条，说明工具做什么、只读默认、不需要 token。
   - HN 上已有的 Ask HN（https://news.ycombinator.com/item?id=50150824）回复。
   - 11-12 第一次 brownout 当天考虑 Show HN / r/devops。

## 需要用户决定

- README 要不要写 12-10 这个最终移除日期。本轮已经在公告正文（10-21 更新）里读到 “Dec 10, 2026 Final removal”，但任务说明要求不写；README 目前只写 “11-12 起开始拒绝，之后按公告的 brownout 安排到最终移除”。
- 是否在 v0.1.0 加 GitHub Action / Bitbucket Pipe（目前没做，`--format markdown` 和退出码 1 已经能在 CI 里用）。
- 是否扩到 Dockerfile、Terraform、Ansible（设计里列为已知限制）。
- crates.io 是否发布。
