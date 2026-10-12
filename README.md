# edit_sys_time

Rust HTTP 服务，部署在树莓派上，接收 HTTP 请求修改系统日期和时间。启动时自动禁用 NTP 自动时间同步。

## 功能

- 启动时执行 `timedatectl set-ntp false` 禁用自动时间同步
- 提供 `POST /api/time` 接口，接受 JSON 请求设置系统时钟
- 自动检测系统时区，无时区的时间串按本地时区处理
- 支持在请求中指定时区

## API

### POST /api/time

**请求体** (JSON):

```json
{
  "datetime": "2026-08-06 15:30:00",
  "timezone": "Asia/Shanghai"
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `datetime` | string | 是 | 时间字符串，支持多种格式 |
| `timezone` | string | 否 | IANA 时区名或 UTC 偏移，不传则使用系统时区 |

### timezone 字段

两种指定方式：

- **IANA 时区名**：`Asia/Shanghai`, `America/New_York`, `Europe/London`, `UTC` 等
- **UTC 偏移**：`+08:00`, `-05:00`, `+0800` 等

不传或传空字符串时，自动使用系统检测到的时区（读取 `/etc/timezone` -> `/etc/localtime` -> `$TZ` -> 回退 UTC）。

### datetime 格式

| 格式 | 类型 | 示例 | 说明 |
|---|---|---|---|
| Unix 时间戳 | 绝对 | `"1754488800"` | 秒级，直接设入 `CLOCK_REALTIME`，忽略 timezone |
| RFC 3339 | 绝对 | `"2026-08-06T15:30:00+08:00"` | 自带时区，忽略 timezone 参数 |
| YYYY-MM-DD HH:MM:SS | 相对 | `"2026-08-06 15:30:00"` | 无时区，按 timezone/系统时区 解释后转 UTC 写入 |
| YYYY-MM-DDTHH:MM:SS | 相对 | `"2026-08-06T15:30:00"` | 同上 |
| YYYY/MM/DD HH:MM:SS | 相对 | `"2026/08/06 15:30:00"` | 同上 |
| YYYY-MM-DD HH:MM | 相对 | `"2026-08-06 15:30"` | 同上 |

**时区处理逻辑**：

- **绝对时间**（时间戳 / RFC 3339 带偏移）→ 直接计算 UTC 秒数，写入系统时钟
- **相对时间**（无时区标记的字符串）→ 解释为指定/系统时区的当地时间，自动转换为 UTC 后写入

### 返回

成功：
```json
{"success": true, "message": "Time set to 2026-08-06T07:30:00+00:00 UTC"}
```

失败：
```json
{"success": false, "message": "错误信息"}
```

### POST /api/timezone

单独修改系统时区，不改时间。

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `timezone` | string | 是 | IANA 时区名或 UTC 偏移 |

`timezone` 支持三种格式：

- **IANA 时区名**：`Asia/Shanghai`, `America/New_York`, `UTC` 等
- **偏移格式**：`+08:00`, `+0800`, `-05:00` → 内部映射为 `Etc/GMT∓N`
- **UTC 前缀**：`UTC+08:00`, `UTC+8`, `UTC-05:00` 等

注：仅支持整小时偏移（如 `+08:00`），半/刻钟偏移（如 `+05:30`）请用 IANA 名（如 `Asia/Kolkata`）。

**请求示例**：

```json
{ "timezone": "Asia/Shanghai" }
{ "timezone": "+08:00" }
{ "timezone": "UTC+8" }
```

**返回**：

```json
{"success": true, "message": "Timezone changed to Asia/Shanghai"}
```

## 测试

树莓派上启动服务后：

```bash
# 1. Unix 时间戳
curl -X POST http://localhost:3000/api/time \
  -H "Content-Type: application/json" \
  -d '{"datetime": "1754488800"}'

# 2. RFC 3339 带时区（推荐，无歧义）
curl -X POST http://localhost:3000/api/time \
  -H "Content-Type: application/json" \
  -d '{"datetime": "2026-08-06T15:30:00+08:00"}'

# 3. 无时区字符串 + 系统时区（自动检测）
curl -X POST http://localhost:3000/api/time \
  -H "Content-Type: application/json" \
  -d '{"datetime": "2026-08-06 15:30:00"}'

# 4. 无时区字符串 + 显式指定时区
curl -X POST http://localhost:3000/api/time \
  -H "Content-Type: application/json" \
  -d '{"datetime": "2026-08-06 15:30:00", "timezone": "Asia/Shanghai"}'

# 5. 指定 UTC 偏移
curl -X POST http://localhost:3000/api/time \
  -H "Content-Type: application/json" \
  -d '{"datetime": "2026-08-06 15:30:00", "timezone": "+08:00"}'

# 6. 修改系统时区
curl -X POST http://localhost:3000/api/timezone \
  -H "Content-Type: application/json" \
  -d '{"timezone": "Asia/Shanghai"}'

# 验证
date
```

容器内测试（Docker）：

```bash
# 宿主机 IP 通常是 172.17.0.1 或 host.docker.internal
curl -X POST http://host.docker.internal:3000/api/time \
  -H "Content-Type: application/json" \
  -d '{"datetime": "2026-08-06T15:30:00+08:00"}'
```

## 开发

### 依赖

- Rust 1.80+
- `rustup target add aarch64-unknown-linux-musl`（ARM64 目标）

### 本地编译检查（Windows / macOS / Linux）

```bash
cargo check
cargo clippy
```

### 交叉编译到树莓派（ARM64）

musl 静态链接，rustup 内置工具链，零外部依赖：

```bash
# 添加目标
rustup target add aarch64-unknown-linux-musl

# 编译
cargo build --target aarch64-unknown-linux-musl --release
```

产物：`target/aarch64-unknown-linux-musl/release/edit_sys_time`

> 如果树莓派是 32 位系统，替换目标为 `armv7-unknown-linux-musleabihf`。

### 部署到树莓派

```bash
# 1. 上传二进制
scp target/aarch64-unknown-linux-musl/release/edit_sys_time \
    pi@raspberrypi:/usr/local/bin/

# 2. 添加执行权限
ssh pi@raspberrypi "sudo chmod +x /usr/local/bin/edit_sys_time"

# 3. 安装 systemd 服务
scp deploy/edit_sys_time.service pi@raspberrypi:/tmp/
ssh pi@raspberrypi "sudo mv /tmp/edit_sys_time.service /etc/systemd/system/ && \
    sudo systemctl daemon-reload && \
    sudo systemctl enable --now edit_sys_time"

# 4. 验证
ssh pi@raspberrypi "sudo systemctl status edit_sys_time --no-pager"
```

### 更新部署

```bash
# 重新编译
cargo build --target aarch64-unknown-linux-musl --release

# 停止服务 -> 上传 -> 启动
ssh pi@raspberrypi "sudo systemctl stop edit_sys_time"
scp target/aarch64-unknown-linux-musl/release/edit_sys_time \
    pi@raspberrypi:/usr/local/bin/
ssh pi@raspberrypi "sudo systemctl start edit_sys_time"
```

## 项目结构

```
edit_sys_time/
├── Cargo.toml                 # 依赖配置
├── .cargo/config.toml         # 交叉编译链接器配置
├── deploy/
│   └── edit_sys_time.service  # systemd 服务文件
└── src/
    └── main.rs                # 主程序
```

## 恢复自动时间同步

如需恢复 NTP 自动同步，先停止本服务再重新启用 NTP：

```bash
# 1. 停止本服务
sudo systemctl stop edit_sys_time
sudo systemctl disable edit_sys_time

# 2. 恢复 NTP 自动同步
sudo timedatectl set-ntp true

# 3. 查看 NTP 状态（确认已启用）
timedatectl show -p NTP
# 输出 NTP=yes 表示已恢复
```

## 关于硬件时钟 (RTC)

树莓派默认没有 RTC 电池，关机后时钟停走，开机时从 `/etc/fake-hwclock.data` 恢复上次记录的时间。本服务在每次设置时间后调用 `hwclock --systohc` 写入该文件，确保重启后自动恢复。

行为说明：
- 修改时间为 `15:30:00` → 写入 fake-hwclock
- 关机 2 小时后开机 → 时间恢复为 `15:30:00`（非 `17:30:00`）
- `hwclock --systohc` 同时兼容 RTC 硬件，有 RTC 模块时会写入真实 RTC 芯片

如果安装 RTC 模块（如 DS3231）：
- 启用 I2C 接口并加载对应的设备树覆盖（dtoverlay）
- 内核启动时直接从 RTC 芯片读取时间，不再使用 fake-hwclock
- 关机期间 RTC 芯片由电池供电，时间持续走时
- 本服务的 `hwclock --systohc` 也会直接写入 RTC 芯片

## 注意事项

- 修改系统时钟需要 **root 权限**，systemd 服务已配置 `User=root`
- 服务监听 `0.0.0.0:3000`，仅限内网环境使用，请勿暴露到公网
- musl 静态链接意味着二进制不依赖目标系统的 libc 版本，在所有 ARM64 Linux 上通用
