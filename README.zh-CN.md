# async-bash-mcp

[![License](https://img.shields.io/badge/license-MIT-blue)](https://github.com/vincent-chang/async-bash-mcp/blob/main/LICENSE)
[![Test](https://github.com/vincent-chang/async-bash-mcp/actions/workflows/test.yaml/badge.svg)](https://github.com/vincent-chang/async-bash-mcp/actions/workflows/test.yaml)

[English](README.md) | 中文

一个用于异步启动和管理 bash 命令的 MCP 服务器。支持并行运行多个 shell 命令，并独立检查各命令的执行进度。

## 背景

在被 opencode 的 bash 执行超时问题困扰时，我发现了原项目 [async-bash-mcp](https://github.com/xhuw/async-bash-mcp)。原项目确实能有效解决 opencode 的 bash 执行超时问题，在下载和安装项目依赖等耗时操作场景下尤为实用。本项目是对其进行的 Rust 改写，使用 [opencode](https://opencode.ai/) + [oh-my-opencode](https://github.com/code-yeongyu/oh-my-opencode) 在 [OpenChamber](https://github.com/btriapitsyn/openchamber) 中完成。作为一个 local 类型的 MCP 服务器，Rust 的启动开销比 Python 更小，更适合每次会话都需要启动的工具场景。

## 在 opencode 中使用

在 `opencode.json` 配置中添加以下内容，用 async-bash-mcp 替换内置的 bash 工具：

```json
{
  "$schema": "https://opencode.ai/config.json",
  "tools": {
    "bash": false
  },
  "mcp": {
    "async-bash": {
      "type": "local",
      "command": ["/path/to/async-bash-mcp"],
      "enabled": true
    }
  }
}
```
从 [GitHub Releases](https://github.com/vincent-chang/async-bash-mcp/releases) 下载最新的预编译二进制文件，或通过 `cargo build --release` 从源码构建。

然后可以使用类似以下的指令：
- "在后台启动一个长时间运行的构建任务"
- "并行运行测试并展示结果"
- "启动一个服务器，准备好后通知我"

## 为什么选择异步 bash？

在处理构建、测试或服务器等长时间运行的命令时，AI 代理需要：
- 增量监控进度，而无需预先设定固定的超时时间
- 并行运行多个命令，并独立检查每个命令的状态
- 根据部分输出结果决定是继续还是终止
- 在命令产生输出时实时处理反馈

本工具为代理提供更充分的信息用于决策，从而加快任务完成速度，减少混乱的响应。

**相比内置 bash 工具的关键优势：**
- **更好的决策能力**：代理可以查看部分输出，做出更明智的继续或终止选择
- **并行执行**：同时运行多个命令
- **无需猜测超时**：通过增量检查进度，替代预先设定超时
- **更快的迭代**：错误已经可见时，无需等待超时结束

本工具旨在替代 opencode 的内置 bash 工具，适用于任何涉及长时间运行命令的场景，为代理提供更好的决策信息，节省您的时间。

## 工具

**spawn** - 异步启动一个 bash 命令
- `command` (str)：要执行的 bash 命令
- `cwd` (str, 可选)：工作目录路径
- 返回一个用于跟踪的进程 ID

**list_processes** - 显示所有正在运行/最近完成的进程
- 无参数
- 返回数组 `{"ID": int, "command": str, "done": bool}`

**poll** - 检查已启动进程的进度
- `process_id` (int)：spawn 命令返回的进程 ID
- `wait` (int)：等待时间（毫秒）
- `terminate` (bool, 可选)：返回结果前终止进程
- 返回 `{"stdout": str, "stderr": str, "elapsedTime": float, "finished": bool, "exitCode": int}`

## 安装

从 [GitHub Releases](https://github.com/vincent-chang/async-bash-mcp/releases) 下载预编译二进制文件，或从源码构建：

```bash
cargo build --release
# 二进制文件位于：target/release/async-bash-mcp
```
