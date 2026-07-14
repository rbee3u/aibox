# REWRITE.md — 将 aibox 从 Bash 改写为 Rust 的可行性与计划

> **状态:已实施(Phase 0–5 完成)。** 本文最初是待批准的设计文档;计划已获批并
> 落地为单一 `aibox` 二进制(`aibox claude` / `aibox codex` 子命令)。Rust 代码在
> `src/`,Dockerfile/status.sh 内嵌自 `assets/`,CI 在 `.github/workflows/ci.yml`。
> 原 Bash 脚本(`aibox-claude` / `aibox-codex`)暂时保留作回退,待充分测试后再删。
> 下面的计划与决策记录保留,作为改写依据与历史。
>
> 原始目标:评估把 `aibox-claude` / `aibox-codex` 两个 Bash 包装脚本改写为 Rust
> 是否可行,并给出一份足够具体、可以照着执行的改写计划。

---

## 1. 结论(TL;DR)

**可行,且技术上很干净。** 这两个脚本做的所有事情——解析参数、读/合并 env 文件、
生成与 `sync` 模板、浏览 session 转写、拼装并执行 `docker run`——都能一对一映射到
Rust 标准库加少量 crate,没有任何做不到的地方。

**是否值得,取决于一件事:你是否预期功能会继续增长。**
你提到「功能越来越复杂」——如果这个判断成立,Rust 的收益(类型安全、真正的单元测试、
`serde_json` 取代手写 JSON 解析、摆脱 bash 3.2 的所有限制)会随功能增长持续兑现。
如果功能已经基本定型,那现在这套「基本能用的 Bash」重写的净收益有限。

**唯一真正的退步是分发**:现状是「symlink 一下就能用,零依赖零构建」;Rust 版要么要求
宿主机有 Rust 工具链去编译,要么需要分发预编译二进制(见 §7)。这是需要你拍板的核心点。

我的建议:**如果确认要继续加功能,就重写**;把分发方式(§7)和一/二进制布局(§4)这两个
决策先定下来,再进入 §8 的分阶段实施。

---

## 2. 现状盘点(改写要覆盖的全部行为)

两个脚本 + 一个状态脚本 + 两个 Dockerfile。核心逻辑其实不大,大部分体积是注释。

| 能力 | 位置 | 说明 |
| --- | --- | --- |
| 参数解析 | 两脚本顶部 `while` 循环 | `sync` / `session` 子命令 + `-p/-e/-w/-m/--safe/--build`,`--` 后透传 |
| `--help` | `usage()` | **把文件头注释块当帮助文本打印**(`sed` 抽取第 3 行到首个空行) |
| profile 布局 | `$AIBOX_CONFIG_ROOT/<profile>/{base,envs/,home/}` | 每 profile 隔离 |
| env 文件合并 | `awk` 合并 `base` + relay | 后者覆盖前者,`KEY=` 空值可清空 base 默认,保序 |
| 模板生成 | `emit_base_template` / `emit_relay_template` | 带 `# aibox-template: vN` 版本戳 |
| `sync` | `sync_file`(awk) | 把旧文件的真实配置行重新嵌回新模板的示例行下方;孤儿键放到尾部块 |
| 模板版本提示 | `file_template_version` | 文件版本落后就提示 `sync`(不改文件) |
| session 浏览 | `session_*`(awk 手写 JSON 解码) | `list` / `get` / `delete`,按短 id 前缀解析 |
| docker run 拼装 | 两脚本尾部 | 硬化 flag、TTY 探测、Linux uid/gid+host-gateway、挂载、权限旁路 |
| 凭据临时文件 | `mktemp` + `trap ... EXIT INT TERM` | 0600 临时文件,退出/中断时清理(所以**不能 `exec docker`**) |
| 状态行 | `aibox-claude-status.sh` | 首次运行播种到 profile home;**运行在容器内**,保持 Bash |

两个脚本刻意保持结构平行(见 `AGENTS.md`)。差异点集中在:

- **模板内容**(Claude 是 env 变量的模型分层;Codex 是单模型 + reasoning 档位);
- **端点如何抵达 agent**:Claude 全靠环境变量;Codex 靠运行时 `-c key=value` 覆盖注入
  `config.toml`,只有 API key 走环境变量或临时 `auth.json`(两种互斥的 auth 模式);
- **session 磁盘格式与提取器**(Claude 有 `ai-title` + `promptSource:"typed"`;Codex 是
  `rollout-*.jsonl`,首行 `session_meta`,需过滤注入的 wrapper 上下文);
- **Codex 特有**:`--exec` 无头模式、`CODEX_INSTRUCTIONS_FILE` 只读挂载、`query_params`。

---

## 3. 为什么用 Rust(收益)与代价

### 收益
- **`serde_json` 干掉手写 JSON 解码。** 现在 `SESSION_AWK` 里约 130 行手写 UTF-8 /
  `\uXXXX` / 代理对解码(因为 macOS 的 BSD awk 没有 `strtonum`/位运算)——`serde_json`
  原生正确处理,直接删除。
- **摆脱 bash 3.2 的所有约束。** 关联数组、`${var,,}`、空数组守卫 `${arr[@]+…}`——
  这些绕道全部消失,`HashMap`/`IndexMap`/`String::to_lowercase()` 随便用。
- **真正的单元测试。** 现在「除了跑一遍没有测试」(`AGENTS.md`)。合并、`sync`、
  session 解析、参数解析都能做成纯函数 + `#[test]`。
- **编译器强制两 agent 平行。** 现在靠人肉「改一个记得改另一个」;Rust 里差异点收敛为
  一个 trait / enum,共享逻辑只有一份,漏改会编译不过。
- **凭据清理更稳。** `trap` 换成 RAII `Drop`(正常路径)+ 信号处理(中断路径),见 §5。
- **删掉自我定位逻辑。** Dockerfile 无 `COPY`(见 §1),用 `include_str!` 内嵌,
  `readlink` 那段可整段删除。

### 代价
- **分发从「零构建」变成「要构建或要分发二进制」**(§7)——最大的退步。
- **`--help = 文件头注释` 这个巧妙约定消失**,改用 `clap` 生成的帮助(或内嵌帮助字符串)。
- **每平台一个二进制**(macOS arm64/x64、Linux x64/arm64),需要交叉编译或 CI 矩阵。
- 代码行数会变多(Rust 比 Bash 啰嗦),但可读性/可测性换来了。

---

## 4. 目标架构(已定稿)

**决策(已定):** 单一可执行文件 `aibox`,通过**子命令**分发到两个 agent:
`aibox claude …` / `aibox codex …`。子命令下的 flag(`-e/-p/-w/-m/--safe/--build`、
`sync`、`session`)两者共享;Codex 特有的 `--exec` 只挂在 `codex` 子命令上。这取代了本文
早期草案里「两个 `[[bin]]`」的推荐——用户从「装两个脚本」变成「装一个 `aibox`」,
`clap` 的子命令天然支持这种分发。

一个 Cargo package,一个共享库 crate(`[lib]`)+ 一个 `[[bin]] aibox`,差异点用类型系统
(`enum AgentKind` + `trait Agent`)强制平行,共享逻辑只写一份。

```
aibox/
├── Cargo.toml                # package,含 [lib] + 单个 [[bin]] aibox
├── src/
│   ├── lib.rs                # 共享核心,re-export 各模块
│   ├── main.rs               # bin 入口:clap 解析 → 派发到 AgentKind + 子命令
│   ├── cli.rs                # clap 参数定义(顶层 + claude/codex 子命令)
│   ├── agent.rs              # trait Agent + enum AgentKind{Claude,Codex} —— 差异点收口
│   ├── profile.rs            # profile 路径解析、目录创建、home 播种
│   ├── envfile.rs            # env 文件解析 + base/relay 合并(IndexMap 保序、后者覆盖)
│   ├── template.rs           # 模板生成 + TEMPLATE_VERSION + 版本戳读取
│   ├── sync.rs               # sync 合并引擎(取代 sync_file 的 awk)
│   ├── session/
│   │   ├── mod.rs            # 与 agent 无关的 surface/resolve/delete
│   │   ├── claude.rs         # Claude 转写提取器(serde)
│   │   └── codex.rs          # Codex 转写提取器 + wrapper 过滤
│   ├── docker.rs             # docker run/build 参数拼装 + 执行
│   ├── creds.rs              # 凭据临时文件 + RAII 清理 + 信号处理(§5)
│   └── platform.rs           # uid/gid、TTY 探测、OS 判定
├── assets/
│   ├── aibox-claude.Dockerfile   # include_str! 内嵌
│   ├── aibox-codex.Dockerfile
│   └── aibox-claude-status.sh    # 内嵌,首次运行写入 profile home
└── tests/                    # 集成测试(env 合并、sync、session 解析的黄金样例)
```

> 镜像名与 profile 根目录保持不变:`aibox claude` 仍用 `aibox-claude:latest` 镜像和
> `~/.aibox/claude/` 配置根,`aibox codex` 同理。子命令化只改调用入口,不改磁盘布局,
> 现有用户的 profile/镜像零迁移。

### 差异点收口:`trait Agent`

```rust
pub enum AgentKind { Claude, Codex }

pub trait Agent {
    fn image_default(&self) -> &str;              // aibox-claude:latest / aibox-codex:latest
    fn config_root_default(&self) -> PathBuf;     // ~/.aibox/claude | ~/.aibox/codex
    fn dockerfile(&self) -> &'static str;         // include_str! 的内容
    fn base_template(&self, ver: u32) -> String;
    fn relay_template(&self, name: &str, ver: u32) -> String;
    fn container_home(&self) -> &str;             // /home/claude | /home/codex

    /// 把合并后的 relay 配置翻译成 docker run 的挂载/env 以及 agent 命令行。
    /// Claude:注入环境变量;Codex:拼 `-c` 覆盖 + 选择 auth 模式。
    fn build_invocation(&self, cfg: &MergedEnv, opts: &RunOpts) -> Invocation;

    /// session 提取器(list 预览、get 逐条 prompt)。
    fn session_backend(&self) -> Box<dyn SessionBackend>;
}
```

共享逻辑(profile 解析、env 合并、sync、session 的 resolve/delete、硬化 flag)只写一遍;
`AgentKind` 只挑差异。

---

## 5. 逐项映射:Bash 构造 → Rust 做法

| 现在(Bash) | Rust 方案 | 备注 |
| --- | --- | --- |
| `while`+`case` 参数循环 | `clap`(derive) | 或保留手写解析以完全复刻行为;推荐 clap |
| `usage()` 打印头注释 | `clap` 自动帮助 / 内嵌帮助串 | 头注释即帮助的巧妙约定消失 |
| `awk` env 合并(保序+覆盖) | `indexmap::IndexMap<String,String>` | 保序 + 后者覆盖天然 |
| `KEY=` 空值清空 base 默认 | 合并时保留空值行,输出时照写 | 复刻现语义 |
| `sync_file` 的 awk 合并 | `sync.rs`:逐行扫模板,示例行 `#KEY=` 下嵌回真实值,孤儿键入尾部块 | 纯函数,好测 |
| `file_template_version`(sed) | 读首行正则解析 `vN` | |
| `SESSION_AWK`(~130 行手写 JSON) | **`serde_json` 逐行解析** | 最大简化点;代理对/`\u` 免费 |
| Claude session 文件发现 | `<home>/.claude/projects/**/*.jsonl` 遍历 | `walkdir` 或 `std::fs` 递归 |
| Codex session 文件发现 | `<home>/.codex/sessions/**/rollout-*.jsonl` | 同上 |
| Codex wrapper 过滤 `CODEX_SKIP_RE` | `regex` crate 或前缀匹配 | 逐 content item 过滤 |
| 短 id 前缀解析 + 歧义列出 | 收集匹配 + 计数 | 直接照搬逻辑 |
| `mktemp` + `chmod 600` | `tempfile` crate(建时即 0600) | |
| `trap ... EXIT INT TERM` | **`Drop` +信号处理**,见下 | 关键点,注意信号 |
| **不能 `exec docker`** | 天然:`Command::spawn()` + `wait()`,清理在 wait 之后 | Rust 本就是子进程模型 |
| `readlink` 自我定位找 Dockerfile | **删除**;`include_str!` 内嵌 Dockerfile | 因为 Dockerfile 无 COPY(§1) |
| 播种 `status.sh` from SCRIPT_DIR | `include_str!` 内嵌,首次写入 home | 同上 |
| `docker build`(空 context) | `include_str!` 的 Dockerfile 经 `-f <tmp>` 或 stdin,context 用空目录 | Dockerfile 无 COPY,context 无关 |
| `uname` 判 Linux | `cfg!(target_os="linux")` / `std::env::consts::OS` | 每平台各自编译 |
| `id -u`/`id -g` | `rustix::process::{getuid,getgid}`(或 `libc`) | 纯 Rust 首选 rustix |
| `-t 0 && -t 1` TTY 探测 | `std::io::IsTerminal`(std 1.70+) | 无需 crate |
| `~/` 展开(env 文件里的字面量) | 手动替换 `HOME` | 复刻现有手动展开 |
| Codex `-c key=value` 拼装 | 结构化 `Vec<String>` push | 类型清晰 |
| Codex auth 双模式(env_key / auth.json) | enum 二选一,`creds.rs` 分支 | 互斥由类型保证 |

### 需要格外小心:凭据清理与信号(§5 的核心风险)

Bash 的 `trap ... EXIT INT TERM` 会在 **Ctrl-C / kill** 时清理 0600 凭据临时文件。
Rust 的 `Drop` **默认不会在收到 SIGINT/SIGTERM 时运行**——默认信号处理直接终止进程,
不跑析构。所以:

- **正常路径**:用 `tempfile`/自定义 `Drop` guard,`docker` 作为子进程 `wait()` 结束后自动清理。
- **中断路径**:必须额外安装信号处理(`signal-hook` 或 `ctrlc` crate),在 SIGINT/SIGTERM
  时删除已登记的临时凭据文件再退出。**这是唯一一处不能只靠 RAII 蒙混过关的地方**,
  实现时要专门写测试/手动验证 Ctrl-C 场景。

注意:我们本来就把 docker 当子进程 `wait`(不像 bash 要靠不 `exec` 来保住 trap),所以
Rust 这边「不能 exec」的约束天然不存在——但信号下的清理仍要显式处理。

---

## 6. 依赖(crate)清单

刻意保持精简:

| crate | 用途 | 必要性 |
| --- | --- | --- |
| `clap`(derive) | 参数解析 + 帮助 | 推荐(或手写解析,零依赖) |
| `serde` + `serde_json` | session JSON 解析 | **核心收益,必要** |
| `indexmap` | 保序 env 合并 | 必要(或用 `Vec<(K,V)>` 手撸) |
| `tempfile` | 0600 临时凭据文件 | 必要 |
| `signal-hook` 或 `ctrlc` | 中断时清理凭据 | 必要(§5) |
| `rustix` 或 `libc` | getuid/getgid | 必要(或 shell out `id`) |
| `regex` | Codex wrapper 过滤 | 可选(可用前缀匹配替代) |
| `anyhow` | 错误处理 | 可选,便利 |
| `walkdir` | 递归找 session 文件 | 可选(std 也能做) |

`IsTerminal` 走 std,不需要 crate。

---

## 7. 分发 / 安装的变化(已定稿)

现状(README §Install):`ln -s aibox-claude ~/.local/bin/...`,零构建。

**决策(已定):路线 1 起步(`cargo install`),预留路线 2(`cargo-dist` 预编译)做开源发布。**
理由:主要自己用(宿主机已有 rustc 1.97,`cargo install --path .` 一步到位),同时希望开源
惠及他人——`cargo-dist` 的 CI 矩阵可在需要时零改动接上,给不装 Rust 的用户提供 `curl | sh`
安装。两者不冲突:先跑通源码安装,发布时再加 CI。

- **路线 1(现在做)**:`cargo install --path .` 装 `aibox` 到 `~/.cargo/bin`;开发期用
  `cargo build --release` + symlink `target/release/aibox`。
- **路线 2(开源发布时做,Phase 5 的 CI 里预留)**:`cargo-dist` 生成 4 平台
  (macOS arm64/x64、Linux x64/arm64)产物挂 GitHub Release,README 给一行安装脚本。

> 讽刺但真实:项目本身的容器里**已经装了 Rust**,但那是给 `/work` 项目用的,和「宿主机跑
> 包装器」是两回事。包装器是在宿主机上跑的。

---

## 8. 分阶段实施计划

每阶段结束都可编译、可跑、可测。建议按此顺序(后者依赖前者的基础设施):

**Phase 0 — 脚手架**
- `cargo new`,建 package + lib + 两 `[[bin]]`;把两个 Dockerfile 和 status.sh 移到
  `assets/` 并 `include_str!`。
- `platform.rs`(uid/gid、TTY、OS)、`cli.rs`(clap 骨架)。
- 产出:`--help` 与现脚本行为一致;`--build` 能用内嵌 Dockerfile 建镜像(空 context)。
- 验证:`cargo build`;`aibox-claude --build` 成功建出镜像。

**Phase 1 — 配置与运行(先打通 Claude,因为它更简单——纯环境变量)**
- `profile.rs` + `envfile.rs`(合并)+ `template.rs` + home 播种 + relay 必填校验。
- `docker.rs` 拼 `docker run`(硬化 flag、挂载、权限旁路)。
- `creds.rs`:0600 merged env 文件 + `Drop` + 信号清理。
- `agent.rs`:`Agent` trait + `Claude` 实现。
- 产出:`aibox-claude -e <relay>` 端到端可跑,与 Bash 版行为一致。
- 验证:对着真实 relay 跑一次;单测 env 合并的覆盖/清空/保序语义。

**Phase 2 — Codex 实现(差异最大的一块)**
- `Codex` 实现 `Agent`:`-c` 覆盖拼装、auth 双模式(env_key vs auth.json)、
  `--exec`、`CODEX_INSTRUCTIONS_FILE` 只读挂载、`query_params` 拆分。
- 特别复刻:auth.json 预建(避免 virtiofs 嵌套挂载问题,见 codex 脚本 705–719 行注释)。
- 验证:两种 auth 模式各跑一次;`--exec` 无头跑一次。

**Phase 3 — `sync`**
- `sync.rs` 合并引擎 + `sync one/all/--dry-run`。
- 验证:用「旧文件」黄金样例测合并(示例下回填、孤儿键入尾部块、chmod 600)。

**Phase 4 — session 浏览**
- `session/`:发现、resolve(前缀+歧义)、`list`/`get`/`delete`,serde 提取器。
- **注意**:codex 脚本注释坦承其磁盘格式是「照 `rollout` crate 规格构造的合成样例验证,
  未对真实 codex 文件验证过」——改写时**用真实 codex 生成的文件做一次校验**,顺手修正现有偏差。
- 验证:黄金 `.jsonl` 样例测 list/get 输出;delete 的确认流程。

**Phase 5 — 收尾**
- 更新 `README.md`(安装/分发变化)、`AGENTS.md`(约束改为 Rust:去掉 bash 3.2 段,
  新增「两 bin 靠 trait 平行」「凭据靠 Drop+信号」「模板改版记得 bump TEMPLATE_VERSION」)。
- CI:`cargo build/test/clippy/fmt`;若走分发路线 2 加 `cargo-dist`。
- 决定 Bash 脚本去留:建议保留一个 release tag 作为回退点后再删。

`status.sh` **保持 Bash**——它跑在容器内,依赖容器里的 `jq`,不该 Rust 化,只是被内嵌播种。

---

## 9. 测试策略(相对现状的净提升)

现在「除了跑没有测试」。Rust 版目标:

- **纯函数单测**:env 合并、sync 回填、模板版本解析、短 id 前缀解析、Codex `-c` 拼装。
- **黄金样例集成测**:`tests/fixtures/` 放真实(脱敏)Claude / Codex `.jsonl`,断言
  `list`/`get` 输出;放「旧 env 文件」断言 `sync` 结果。
- **手动验证清单**(自动化难覆盖的):Ctrl-C 中途凭据文件是否被清;两种 auth 模式;
  macOS + Linux 各跑一遍(uid/gid、host-gateway、TTY 分支)。

---

## 10. 风险与开放问题

- **[高] 分发方式**(§7)——最大的用户可感变化,需先定。
- **[中] 信号下的凭据清理**(§5)——Rust 的 Drop 不覆盖 SIGINT/SIGTERM,必须显式处理并验证。
- **[中] Codex session 格式**——现有实现未对真实文件验证过;改写时正好校准。
- **[低] 帮助文本**——「头注释即帮助」的约定消失,换 clap;可接受。
- **[低] 跨平台二进制**——4 目标,`cargo-dist` 可自动化(仅走分发路线 2 时才需要)。
- **[低] 行为完全等价**——建议改写期间保留 Bash 版,逐命令做输出对拍(diff)。

## 11. 决策记录(已定稿)

1. **重写?** ✅ 是——功能会继续增长。
2. **分发路线** ✅ 路线 1(`cargo install`)起步,预留路线 2(`cargo-dist`)做开源发布。
3. **二进制形态** ✅ **单一 `aibox` 二进制 + 子命令**(`aibox claude` / `aibox codex`),
   取代原先「两个二进制」的建议。
4. **`clap`** ✅ 用——子命令分发本就是 clap 强项;放弃「头注释即帮助」的约定。
5. **Bash 版去留** ✅ 暂留;等测试稳定后再删(保留 tag 回退点)。

以上已定,从 Phase 0 开始实施。
