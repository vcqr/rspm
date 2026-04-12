# RSPM 实现文档

## 项目概述

rspm (Rust Process Manager) 是一个类似 pm2 的进程管理工具，使用 Rust 语言实现。本文档记录了完整的实现过程。

## 一、需求分析

根据 PRD.md，核心功能包括：

1. **后台运行（守护进程）** - 将应用放在后台运行，终端关闭不影响服务
2. **自动重启** - 应用崩溃时自动重启，使用指数退避算法
3. **负载均衡** - 开启多进程充分利用多核 CPU
4. **日志管理** - 统一收集标准输出和错误日志，支持日志分割
5. **监控** - 提供实时的 CPU、内存监控
6. **分布式管理** - 支持通过 gRPC 进行远程管理

## 二、架构设计

### 2.1 整体架构

采用 CLI + Daemon 架构：

```
+----------------+         (gRPC)            +-------------------+
|  CLI (rspm)    | <----------------------> |  Daemon (rspmd)   |
+----------------+                           +-------------------+
       (用户交互)                                  (核心调度引擎)
                                                      |
                                          +-----------+-----------+
                                          |                       |
                                  +---------------+       +---------------+
                                  |  Process A    |       |  Process B    |
                                  | (子进程管理)   |       | (日志收集)    |
                                  +---------------+       +---------------+
```

### 2.2 模块划分

```
rspm/
├── rspm-common/      # 共享库：类型定义、错误处理、工具函数
├── rspm-proto/       # gRPC 协议定义：proto 文件和生成的代码
├── rspm-cli/         # CLI 工具：用户命令行接口
└── rspm-daemon/      # 守护进程：进程管理、监控、日志、gRPC 服务
```

### 2.3 进程状态机

```
STOPPED -> STARTING -> RUNNING -> STOPPING -> ERRORED
                |          |
                v          v
            ERRORED    STOPPING
```

## 三、实现步骤

### 步骤 1：项目结构设计

创建 Rust workspace 项目，包含四个子项目：

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "3"
members = [
    "rspm-common",
    "rspm-proto",
    "rspm-cli",
    "rspm-daemon",
]
```

### 步骤 2：实现共享模块 (rspm-common)

#### 2.1 配置定义 (config.rs)

```rust
pub struct ProcessConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    pub instances: u32,
    pub autorestart: bool,
    pub max_restarts: u32,
    pub max_memory_mb: u32,
    pub watch: bool,
    pub watch_paths: Vec<String>,
    pub log_file: Option<String>,
    pub error_file: Option<String>,
    pub log_max_size: u64,
    pub log_max_files: u32,
}
```

#### 2.2 错误类型 (error.rs)

```rust
#[derive(Error, Debug)]
pub enum RspmError {
    #[error("Process not found: {0}")]
    ProcessNotFound(String),
    #[error("Failed to start process: {0}")]
    StartFailed(String),
    #[error("Daemon not running")]
    DaemonNotRunning,
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    // ... 更多错误类型
}
```

#### 2.3 进程状态和重启策略 (process.rs)

实现指数退避重启策略：

```rust
pub struct RestartPolicy {
    pub max_restarts: u32,
    pub restart_count: u32,
    pub restart_window_secs: u64,
    pub base_delay_ms: u64,      // 基础延迟 1000ms
    pub max_delay_ms: u64,       // 最大延迟 60000ms
    pub restart_times: Vec<Instant>,
}

impl RestartPolicy {
    /// 计算指数退避延迟
    pub fn calculate_delay(&mut self) -> Option<Duration> {
        // 清理过期的重启记录
        let window_ago = now - Duration::from_secs(self.restart_window_secs);
        self.restart_times.retain(|&t| t > window_ago);

        // 检查是否超过最大重启次数
        if self.restart_times.len() >= self.max_restarts as usize {
            return None;
        }

        // 计算指数退避: 1s -> 2s -> 4s -> 8s -> ...
        let delay_ms = self.base_delay_ms * (2u64.pow(count - 1));
        Some(Duration::from_millis(delay_ms.min(self.max_delay_ms)))
    }
}
```

### 步骤 3：定义 gRPC 协议 (rspm-proto)

#### 3.1 Proto 文件定义

```protobuf
syntax = "proto3";
package rspm;

// 进程状态枚举
enum ProcessState {
    STOPPED = 0;
    STARTING = 1;
    RUNNING = 2;
    STOPPING = 3;
    ERRORED = 4;
}

// 进程配置
message ProcessConfig {
    string name = 1;
    string command = 2;
    repeated string args = 3;
    map<string, string> env = 4;
    string cwd = 5;
    int32 instances = 6;
    bool autorestart = 7;
    int32 max_restarts = 8;
    int32 max_memory_mb = 9;
    // ...
}

// 服务定义
service ProcessManager {
    rpc StartProcess(StartProcessRequest) returns (StartProcessResponse);
    rpc StopProcess(StopProcessRequest) returns (StopProcessResponse);
    rpc RestartProcess(RestartProcessRequest) returns (RestartProcessResponse);
    rpc ListProcesses(ListProcessesRequest) returns (ListProcessesResponse);
    rpc StreamLogs(StreamLogsRequest) returns (stream LogEntry);
    // ...
}
```

#### 3.2 构建脚本 (build.rs)

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/rspm.proto"], &["proto"])?;
    Ok(())
}
```

### 步骤 4：实现守护进程核心模块

#### 4.1 进程管理器 (ProcessManager)

核心数据结构：

```rust
pub struct ProcessManager {
    processes: Arc<RwLock<HashMap<String, ManagedProcess>>>,
    state_store: Arc<StateStore>,
    log_writer: Arc<LogWriter>,
    monitor: Arc<Monitor>,
    event_tx: broadcast::Sender<ProcessEvent>,
    start_time: Instant,
}
```

主要方法实现：

```rust
impl ProcessManager {
    /// 启动新进程
    pub async fn start_process(&self, config: ProcessConfig) -> Result<String> {
        // 检查进程是否已存在
        // 为每个实例创建 ManagedProcess
        // 启动进程并捕获 stdout/stderr
        // 保存配置到状态存储
        // 返回进程 ID
    }

    /// 停止进程
    pub async fn stop_process(&self, id: &str, force: bool) -> Result<()> {
        // 发送 SIGTERM (或 SIGKILL if force)
        // 等待进程退出
        // 更新状态
    }

    /// 事件循环 - 监控和自动重启
    fn start_event_loop(&self) {
        tokio::spawn(async move {
            loop {
                // 检查进程是否存活
                // 如果进程死亡且需要重启，计算退避延迟并重启
                // 更新进程统计信息
                // 检查内存限制
            }
        });
    }
}
```

#### 4.2 托管进程 (ManagedProcess)

```rust
pub struct ManagedProcess {
    pub info: ProcessInfo,
    pub config: ProcessConfig,
    pub child: Option<Child>,
    pub restart_policy: RestartPolicy,
    pub start_time: Option<Instant>,
    pub log_writer: Option<Arc<LogWriter>>,
}

impl ManagedProcess {
    pub async fn start(&mut self, log_writer: Option<Arc<LogWriter>>) -> Result<()> {
        // 构建 Command
        // 设置环境变量和工作目录
        // 配置 stdout/stderr 管道
        // 启动进程
        // 启动日志读取任务
    }

    pub async fn stop(&mut self, force: bool) -> Result<()> {
        // 发送终止信号
        // 等待进程退出
        // 清理资源
    }
}
```

#### 4.3 日志系统 (LogWriter)

```rust
pub struct LogWriter {
    base_dir: PathBuf,
    stdout_file: RwLock<Option<File>>,
    stderr_file: RwLock<Option<File>>,
    max_size: RwLock<u64>,
    max_files: RwLock<u32>,
    log_tx: broadcast::Sender<LogEntry>,
}

impl LogWriter {
    pub async fn write_stdout(&self, process_id: &str, line: &str) {
        // 写入日志文件
        // 广播给订阅者
        // 检查是否需要轮转
    }

    fn rotate_stdout(&self) -> std::io::Result<()> {
        // 删除最旧的文件
        // 重命名现有文件 (.1 -> .2)
        // 创建新文件
    }
}
```

#### 4.4 监控模块 (Monitor)

```rust
pub struct Monitor {
    system: RwLock<System>,
}

impl Monitor {
    pub fn get_process_stats(&self, pid: i32) -> Option<ProcessStats> {
        // 刷新进程信息
        // 获取 CPU 使用率
        // 获取内存使用量
    }
}
```

#### 4.5 gRPC 服务器 (RpcServer)

```rust
pub struct RpcServer {
    process_manager: Arc<ProcessManager>,
    start_time: std::time::Instant,
}

impl RpcServer {
    #[cfg(unix)]
    pub async fn serve_unix(self, socket_path: &str) -> Result<()> {
        // 创建 Unix Domain Socket
        // 设置权限
        // 启动 gRPC 服务器
    }
}

#[tonic::async_trait]
impl GrpcProcessManager for RpcServer {
    async fn start_process(&self, request: Request<StartProcessRequest>) 
        -> Result<Response<StartProcessResponse>, Status> {
        // 转换 proto 类型
        // 调用 ProcessManager
        // 返回响应
    }
    // ... 实现其他 RPC 方法
}
```

### 步骤 5：实现 CLI 工具

#### 5.1 命令定义 (main.rs)

使用 clap 定义命令行接口：

```rust
#[derive(Parser, Debug)]
#[command(name = "rspm")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Start { name: Option<String>, command: Option<String>, instances: u32, ... },
    Stop { id: Option<String>, pid: Option<String>, pname: Option<String>, force: bool },
    Restart { id: Option<String>, pid: Option<String>, pname: Option<String> },
    Delete { id: Option<String>, pid: Option<String>, pname: Option<String> },
    List { name: Option<String> },
    Show { id: Option<String>, pid: Option<String>, pname: Option<String> },
    Logs { id: Option<String>, pid: Option<String>, pname: Option<String>, follow: bool, lines: u32 },
    Scale { id: Option<String>, pid: Option<String>, pname: Option<String>, instances: u32 },
    StopAll,
    Status,
    StartDaemon,
    StopDaemon,
}
```

#### 5.2 CLI 参数说明

所有进程操作命令支持三种方式指定进程标识符：

| 方式 | 参数 | 说明 |
|------|------|------|
| 位置参数 | `<ID>` | 最简洁，自动判断是 ID 还是名字 |
| 明确指定 ID | `--id <ID>` | 明确指定数字进程 ID |
| 明确指定名字 | `--name <NAME>` | 明确指定进程名字 |

**示例**：

```bash
# 方式 1: 位置参数（最简洁）
rspm stop 46
rspm restart myapp
rspm show 46

# 方式 2: 使用 --id 参数（明确指定数字 ID）
rspm stop --id 46
rspm restart --id 46

# 方式 3: 使用 --name 参数（明确指定名字）
rspm stop --name myapp
rspm restart --name myapp
```

**参数验证**：
- ❌ 不能同时使用多个标识符（如 `rspm stop 46 --id 46` 会报错）
- ❌ 必须指定至少一个标识符（如 `rspm stop` 会报错）

#### 5.3 gRPC 客户端 (GrpcClient)

```rust
pub struct GrpcClient {
    client: ProcessManagerClient<Channel>,
}

impl GrpcClient {
    #[cfg(unix)]
    pub async fn connect(socket_path: &str) -> Result<Self> {
        // 使用 Unix Domain Socket 连接
        let channel = Endpoint::try_from("unix://unused")?
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                UnixStream::connect(&path)
            }))
            .await?;
        Ok(Self { client: ProcessManagerClient::new(channel) })
    }
}
```

## 四、关键技术点

### 4.1 Unix Domain Socket 通信

客户端连接代码：

```rust
use hyper_util::rt::TokioIo;

let channel = Endpoint::try_from("unix://unused")?
    .connect_with_connector(tower::service_fn(move |_: Uri| {
        let path = path.clone();
        async move {
            let stream = UnixStream::connect(&path).await?;
            Ok::<_, std::io::Error>(TokioIo::new(stream))
        }
    }))
    .await?;
```

### 4.2 异步进程管理

使用 tokio::process 管理子进程：

```rust
let mut cmd = Command::new(&config.command);
cmd.args(&config.args)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

let mut child = cmd.spawn()?;

// 异步读取输出
let stdout = child.stdout.take().unwrap();
tokio::spawn(async move {
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        log_writer.write_stdout(&process_id, &line).await;
    }
});
```

### 4.3 状态持久化

使用 JSON 文件保存进程配置：

```rust
pub struct StateStore {
    path: PathBuf,
    processes: Arc<RwLock<HashMap<String, ProcessConfig>>>,
}

impl StateStore {
    pub async fn load(&self) -> Result<()> {
        let content = fs::read_to_string(&self.path).await?;
        let saved: HashMap<String, ProcessConfig> = serde_json::from_str(&content)?;
        *self.processes.write().await = saved;
        Ok(())
    }

    pub async fn save(&self) -> Result<()> {
        let processes = self.processes.read().await;
        let content = serde_json::to_string_pretty(&*processes)?;
        fs::write(&self.path, content).await?;
        Ok(())
    }
}
```

### 4.4 类型冲突解决

Proto 生成的类型与自定义类型同名，使用类型别名：

```rust
type ProtoProcessConfig = rspm_proto::ProcessConfig;
type ProtoProcessInfo = rspm_proto::ProcessInfo;

fn to_proto_process_info(info: &ProcessInfo) -> ProtoProcessInfo { ... }
fn from_proto_process_config(proto: &ProtoProcessConfig) -> ProcessConfig { ... }
```

## 六、依赖说明

```toml
# 核心依赖
tokio = { version = "1", features = ["full"] }  # 异步运行时
tonic = "0.12"                                   # gRPC 框架
prost = "0.13"                                   # Protocol Buffers
clap = { version = "4", features = ["derive"] }  # CLI 参数解析
sysinfo = "0.33"                                 # 系统监控
chrono = "0.4"                                   # 时间处理
serde = { version = "1", features = ["derive"] } # 序列化
serde_json = "1"                                 # JSON 处理
tracing = "0.1"                                  # 日志
tracing-subscriber = "0.3"                       # 日志订阅
anyhow = "1"                                     # 错误处理
thiserror = "2"                                  # 自定义错误
uuid = "1"                                       # UUID 生成
tabled = "0.17"                                  # 表格输出
colored = "3"                                    # 彩色输出
```

## 七、构建和运行

### 7.1 前置要求

- Rust 1.70+ (edition 2024)
- protoc (Protocol Buffers 编译器)

### 7.2 构建命令

```bash
# 设置 protoc 路径（如果需要）
export PROTOC=/path/to/protoc

# 构建
cargo build

# 发布构建
cargo build --release --features embed-static
```

### 7.3 运行命令

```bash
# 启动守护进程
./target/debug/rspmd

# 使用 CLI（另一个终端）
./target/debug/rspm start --name myapp -- /bin/sleep 100
./target/debug/rspm list
./target/debug/rspm status
./target/debug/rspm stop myapp
```

## 八、后续优化方向

1. **文件监控自动重启** - 使用 notify crate 监控文件变化（已实现配置文件热重载）
2. **开机自启** - 生成 systemd/launchd 服务文件
3. **Web Dashboard** - 添加 Web UI ✅ (已完成，访问 http://127.0.0.1:6681)
4. **集群管理** - 支持远程节点管理
5. **日志流式传输** - 完善 StreamLogs 实现 ✅ (已实现历史日志读取和实时流式传输)
6. **配置文件支持** - 支持 ecosystem.config.js 类似配置 ✅ (已支持 YAML/JSON/TOML)
7. **定时任务管理** - 支持 Cron 表达式、间隔执行、一次性执行 ✅ (已完成)
8. **静态文件服务器** - 快速启动静态文件服务器，支持目录列表 ✅ (已完成)

## 九、Web Dashboard

RSPM 内置了 Web 管理界面，可以方便地查看和管理进程。

### 访问地址

```
http://127.0.0.1:6681
```

### 功能特性

- 📊 **统计面板** - 总进程数、运行中进程、守护进程运行时间、版本
- 📋 **进程列表** - 显示所有进程的状态、CPU、内存、运行时间
- 🎮 **进程操作** - 启动、停止、重启、删除进程
- 📜 **日志查看** - Tab 卡片方式显示 Stdout/Stderr，支持自动滚动
- 🔄 **自动刷新** - 每 5 秒自动更新数据
- 🌓 **主题切换** - 支持亮色/暗黑主题

### 2026-03-01 最新修复

1. **图标显示** - 所有图标路径数据完整，与 Lucide React 一致
2. **日志 Tab** - Stdout/Stderr 改为 Tab 卡片视图，节省空间
3. **自动滚动** - 日志刷新时自动滚动到底部
4. **状态区分** - Auto/Manual、On/Off 按钮颜色明显区分
5. **图标颜色** - 统计卡片背景图标颜色与数值一致
6. **Footer 信息** - 底部显示版本号和运行时间
7. **蒙版修复** - 模态框蒙版覆盖全界面
8. **API 修复** - 启动和删除操作正常工作

### 技术实现

- Axum Web 框架
- REST API
- 现代化深色 UI（单页面应用）
- 静态文件嵌入（rust-embed）

## 十、文件清单

```
rspm/
├── Cargo.toml                              # Workspace 配置
├── CODEBUDDY.md                            # CodeBuddy 指引
├── PRD.md                                  # 需求文档
├── IMPLEMENTATION.md                       # 本实现文档
├── rspm-common/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── config.rs                       # 进程配置
│       ├── config_format.rs                # 配置格式支持
│       ├── config_loader.rs                # 配置加载器
│       ├── process.rs                      # 进程状态和重启策略
│       ├── error.rs                        # 错误类型
│       ├── constants.rs                    # 常量定义和 banner
│       ├── table.rs                        # 表格渲染（原 rspm-table）
│       └── utils.rs                        # 工具函数
├── rspm-proto/
│   ├── Cargo.toml
│   ├── build.rs                            # Proto 构建脚本
│   ├── proto/
│   │   └── rspm.proto                      # gRPC 协议定义
│   └── src/
│       └── lib.rs
├── rspm-cli/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs                         # CLI 入口
│       ├── client/
│       │   ├── mod.rs
│       │   └── grpc_client.rs              # gRPC 客户端
│       └── commands/
│           ├── mod.rs
│           ├── start.rs
│           ├── stop.rs
│           ├── restart.rs
│           ├── delete.rs
│           ├── list.rs
│           ├── show.rs
│           ├── logs.rs
│           ├── scale.rs
│           ├── status.rs
│           ├── daemon.rs
│           ├── load.rs
│           └── serve.rs              # 静态文件服务器命令
└── rspm-daemon/
    ├── Cargo.toml
    └── src/
        ├── main.rs                         # 守护进程入口
        ├── lib.rs
        ├── manager/
        │   ├── mod.rs
        │   ├── process_manager.rs          # 进程管理器
        │   ├── managed_process.rs          # 托管进程
        │   └── state_store.rs              # SQLite 状态存储
        ├── monitor/
        │   ├── mod.rs
        │   └── monitor.rs                  # 系统监控
        ├── log_watcher/
        │   ├── mod.rs
        │   └── log_writer.rs               # 日志写入器和轮转
        ├── config_watcher/
        │   ├── mod.rs
        │   └── watcher.rs                  # 配置文件热重载
        ├── server/
        │   ├── mod.rs
        │   └── grpc.rs                     # gRPC 服务器
        ├── web/
        │   ├── mod.rs
        │   ├── server.rs                   # Web 服务器
        │   └── api.rs                      # REST API
        └── static_server/
            └── mod.rs                      # 静态文件服务器管理
```

## 八、CLI 命令参考

### 进程管理命令

| 命令 | 说明 | 示例 |
|------|------|------|
| `start` | 启动新进程 | `rspm start myapp -- node server.js` |
| `stop` | 停止进程 | `rspm stop 46` 或 `rspm stop --name myapp` |
| `restart` | 重启进程 | `rspm restart 46` 或 `rspm restart --id 46` |
| `delete` | 删除进程 | `rspm delete 46` 或 `rspm delete --name myapp` |
| `delete --all` | 删除所有进程 | `rspm delete --all` |
| `list` | 列出所有进程 | `rspm list` |
| `show` | 显示进程详情 | `rspm show 46` 或 `rspm show --name myapp` |
| `logs` | 查看进程日志 | `rspm logs 46` 或 `rspm logs --name myapp` |
| `logs -n <N>` | 查看最近 N 行历史日志 | `rspm logs 46 -n 100` |
| `scale` | 缩放进程实例数 | `rspm scale 46 3` 或 `rspm scale --name myapp 3` |
| `stop-all` | 停止所有进程 | `rspm stop-all` |

### 定时任务命令 (schedule)

| 命令 | 说明 | 示例 |
|------|------|------|
| `schedule create` | 创建定时任务 | `rspm schedule create -n backup --cron "0 0 2 * * *" --process myapp --action restart` |
| `schedule list` | 列出所有定时任务 | `rspm schedule list` |
| `schedule show` | 显示定时任务详情 | `rspm schedule show <id-or-name>` |
| `schedule delete` | 删除定时任务 | `rspm schedule delete <id-or-name>` |
| `schedule delete --all` | 删除所有定时任务 | `rspm schedule delete --all` |
| `schedule pause` | 暂停定时任务 | `rspm schedule pause <id-or-name>` |
| `schedule resume` | 恢复定时任务 | `rspm schedule resume <id-or-name>` |
| `schedule history` | 查看执行历史 | `rspm schedule history <id-or-name> --limit 50` |

#### schedule create 参数

```bash
# 格式 1: 使用 -- 分隔符（推荐，类似 start 命令）
rspm schedule create -n <name> --interval <seconds> --action execute -- <command> [args...]

# 格式 2: 使用 --command 和 --args
rspm schedule create -n <name> --interval <seconds> --action execute --command <cmd> --args="arg1,arg2,..."
```

**OPTIONS**:
- `-n, --name <NAME>` - 任务名称（必需）
- `--cron <EXPR>` - Cron 表达式（6字段：秒 分 时 天 月 周）
- `--interval <SECONDS>` - 间隔执行（秒）
- `--once <TIME>` - 一次性执行时间（ISO 8601格式）
- `-a, --action <ACTION>` - 操作类型：`start`/`stop`/`restart`/`execute`
- `-p, --process <NAME>` - 进程名称（用于 start/stop/restart 操作）
- `-C, --command <CMD>` - 自定义命令（用于 execute 操作）
- `--args <ARGS>` - 命令参数（逗号分隔）
- `-t, --timezone <TZ>` - 时区（默认：UTC）
- `-m, --max-runs <N>` - 最大执行次数（0=无限）
- `-d, --description <DESC>` - 任务描述
- `--disabled` - 创建时禁用任务

**定时任务示例**：

```bash
# 每30秒执行一次命令（使用 -- 语法）
rspm schedule create -n poc --interval 30 --action execute -- echo 'Hello World'

# 每天凌晨2点重启进程
rspm schedule create -n nightly-backup --cron "0 0 2 * * *" --process myapp --action restart

# 每5分钟执行一次健康检查
rspm schedule create -n health-check --cron "0 */5 * * * *" --process api --action restart

# 执行复杂的 shell 命令
rspm schedule create -n cleanup --cron "0 0 3 * * *" --action execute --command "/bin/bash" --args="-c,rm -rf /tmp/*.log"

# 或者使用 -- 语法执行 shell 命令
rspm schedule create -n cleanup --cron "0 0 3 * * *" --action execute -- /bin/bash -c "rm -rf /tmp/*.log"

# 一次性执行（指定时间）
rspm schedule create -n one-time --once "2026-03-04T10:00:00Z" --process myapp --action stop
```

**Cron 表达式格式（6字段）**：

| 字段 | 范围 | 说明 |
|------|------|------|
| 秒 | 0-59 | 秒 |
| 分 | 0-59 | 分钟 |
| 时 | 0-23 | 小时 |
| 天 | 1-31 | 日期 |
| 月 | 1-12 | 月份 |
| 周 | 0-7 | 星期（0和7都是周日）|

**常用 Cron 示例**：
- `0 0 2 * * *` - 每天凌晨2点
- `0 */5 * * * *` - 每5分钟
- `0 0 0 * * 1` - 每周一
- `0 0 12 * * 1-5` - 工作日中午12点

#### 定时任务日志

定时任务执行日志保存在：
```
~/.rspm/logs/schedules/<task-name>.log
```

日志内容包含每次执行的详细信息（时间、操作、输出、错误等）。

### 静态文件服务器命令 (serve)

| 命令 | 说明 | 示例 |
|------|------|------|
| `serve` | 启动静态文件服务器 | `rspm serve` |
| `serve -p <PORT>` | 指定端口（默认：8080） | `rspm serve -p 3000` |
| `serve -d <DIR>` | 指定目录（默认：当前目录） | `rspm serve -d ./dist` |
| `serve -H <HOST>` | 指定 host（默认：127.0.0.1） | `rspm serve -H 0.0.0.0` |
| `serve -n <NAME>` | 指定服务器名称 | `rspm serve -n my-site` |

**功能特性：**
- 📁 **目录列表** - 自动生成分页目录列表（带文件大小、修改时间）
- 📄 **文件服务** - 正常提供静态文件访问
- 🎨 **美观界面** - 现代化的目录列表页面设计
- ⚡ **高性能** - 使用 Axum + tower-http 提供高性能服务
- 🔌 **守护进程管理** - 由守护进程托管，支持优雅关闭

**示例**：

```bash
# 在当前目录启动静态服务器（默认端口 8080）
rspm serve

# 指定端口和目录
rspm serve -p 3000 -d ./dist

# 指定名称和 host（允许外部访问）
rspm serve -n my-site -H 0.0.0.0 -p 8080 -d ./public
```

**访问**：
- 目录列表：`http://127.0.0.1:8080/`
- 具体文件：`http://127.0.0.1:8080/filename.txt`

### 守护进程命令

| 命令 | 说明 | 示例 |
|------|------|------|
| `status` | 显示守护进程状态 | `rspm status` |
| `start-daemon` | 启动守护进程 | `rspm start-daemon` |
| `stop-daemon` | 停止守护进程 | `rspm stop-daemon` |

### 配置文件命令

| 命令 | 说明 | 示例 |
|------|------|------|
| `load` | 从配置文件加载进程 | `rspm load ecosystem.yaml` |
| `init` | 交互式生成配置文件 | `rspm init` |

### 进程标识符指定方式

所有进程操作命令（stop、restart、delete、show、logs、scale）支持三种方式指定进程：

1. **位置参数**（最简洁）：`rspm stop 46` 或 `rspm stop myapp`
2. **`--id` 参数**（明确指定数字 ID）：`rspm stop --id 46`
3. **`--name` 参数**（明确指定名字）：`rspm stop --name myapp`

**注意**：
- 不能同时使用多个标识符
- 必须指定至少一个标识符

### start 命令参数

```bash
# 格式 1: 位置参数（推荐）
rspm start <name> -- <command> [args...]

# 格式 2: 使用 --name 选项
rspm start --name <name> -- <command> [args...]

# 格式 3: 使用 -C/--command 选项
rspm start --name <name> -C <command> -- <args>...
```

**OPTIONS**:
- `-n, --name <NAME>` - 进程名称
- `-C, --command <COMMAND>` - 要执行的命令
- `-i, --instances <N>` - 实例数量（默认：1）
- `-w, --cwd <DIR>` - 工作目录
- `-e, --env <KEY=VALUE>` - 环境变量（可多次指定）
- `-f, --config <FILE>` - 配置文件路径（.yaml/.yml/.json/.toml）

**示例**：

```bash
# 启动简单命令
rspm start myapp -- /bin/sleep 100

# 启动 Node.js 应用
rspm start myapp -- node server.js

# 使用 npm 脚本
rspm start myapp -- npm run dev

# 使用配置文件
rspm start -f ecosystem.yaml

# 带环境变量和多实例
rspm start api -i 4 -e NODE_ENV=production -e PORT=3000 -- node server.js
```

## 九、总结

本项目成功实现了一个类似 pm2 的进程管理工具，涵盖了：

- ✅ CLI + Daemon 架构
- ✅ gRPC 通信（Unix Domain Socket）
- ✅ 进程生命周期管理
- ✅ 指数退避自动重启
- ✅ 多实例负载均衡
- ✅ 日志收集和轮转
- ✅ CPU/内存监控
- ✅ 状态持久化
- ✅ 静态文件服务器

项目代码量约 4000+ 行，结构清晰，模块职责分明，为后续功能扩展提供了良好的基础。
