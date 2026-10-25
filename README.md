# VRCRecorder

VRCRecorder 是一款用于快速方便的录制 VRChat 相机画面的桌面程序。

## VR 操作指南

### 前提条件

- Windows 11
- PC 版 VRChat
- SteamVR

使用前需完成以下配置：

1. 在 VRChat 的 Action Menu 中进入 Options → OSC，启用 OSC。
2. 在 VRChat 内手动启动相机。
3. 确认 VRCRecorder 的 OSC 参数与 VRChat 一致。默认监听端口为 `9001`，发送端口为 `9000`；如 VRChat 使用其他端口，需在程序的 VRChat OSC 设置中同步修改。
4. 保持「录制时自动开启 Spout」和「停止时自动关闭 Spout」开启。两项默认均为开启状态。

> 在 OSC 配置正确且相机已手动启动的前提下，开始录制会自动触发相机的直播模式（Streaming）和 Spout 输出，停止录制后自动关闭。

### 控制器绑定

1. 启动 SteamVR 和 VRCRecorder。启动顺序不限，程序会自动重试 SteamVR 初始化。
2. 打开 SteamVR Dashboard，进入 VRCRecorder 面板。面板显示 `READY` 表示待机，显示 `RECORDING` 表示正在录制。
3. 在 VRCRecorder 桌面窗口中点击「配置控制器绑定」，在 SteamVR 绑定界面为 `toggle_record` 动作设置按键。

默认绑定如下：

| 手柄 | 默认按键 |
| --- | --- |
| Valve Index | 右手 A |
| Oculus Touch | 左手摇杆按下 |
| HTC Vive 手柄 | 右手 B |
| Pico 4 | 左摇杆按下 |

### 开始与停止录制

在 VRChat 内手动启动相机后，按下绑定的按键开始录制。VRCRecorder 会通过 OSC 通知 VRChat 将相机切换至直播模式并开启 Spout 输出，待检测到画面后开始写入文件。再次按下同一按键停止录制，程序会结束采集并关闭 VRChat 相机的 Spout 输出，随后在后台完成编码与音频合并，面板恢复为 `READY`。

录制文件默认保存在「视频\VRCRecorder」，命名格式为 `VRCRecorder_YYYY-MM-DD_HH-MM-SS.mp4`。

Dashboard 面板仅用于显示录制状态，不提供点击控制；录制开关由手柄绑定动作触发。

## 桌面端操作

未启用 SteamVR 时，桌面端的录制功能不受影响。

### 初始配置

1. 按上文启用 VRChat 和 VRCRecorder 的 OSC。
2. 在 VRChat 内手动启动相机。
3. 启动 VRCRecorder。程序会自动扫描本机 Spout 发送端，并连接上次使用的源。
4. 若暂时未检测到发送端，请确认相机已在 VRChat 内启动，并检查 OSC 端口设置。开始录制后，程序会请求开启直播模式和 Spout 输出；发送端出现后会自动连接，也可以手动选择。
5. 按需在 Video、Audio 和 Storage 区域配置码率、帧率、画面方向、麦克风及保存目录。

### 录制

点击界面中的录制按钮开始，再次点击停止。录制前需在 VRChat 内手动启动相机。启用「录制时自动开启 Spout」后，程序会通过 OSC 将已启动的相机切换至直播模式并开启 Spout 输出；检测到画面后开始写入文件。

录制文件保存在 Storage 指定的目录中，默认路径为「视频\VRCRecorder」。录制完成后，界面状态区域会显示文件完整路径。

系统音频与麦克风可独立缺失：仅有一路可用时保留该路音频；两路均不可用时仍会保存视频文件。

关闭 OSC 后，程序不会自动切换直播模式或 Spout 输出，需要在 VRChat 中手动开启。

## 功能概述

VRCRecorder 会自动扫描本机 Spout 发送端，并连接上次使用的源；发送端中断后支持自动重连。预览为可选功能，录制开始后会自动关闭，以减少资源占用。

视频以 H.264 编码保存为 MP4，支持 NVENC、AMF、QSV 等硬件编码器。码率可选 4 / 10 / 20 / 40 Mbps，帧率可选 30 / 60 fps；画面支持水平镜像、竖屏旋转和垂直翻转。

系统音频与麦克风分别采集后混合，麦克风可关闭并指定输入设备。

启用 VRChat OSC 后，录制开始时会自动开启已启动相机的 Spout 输出，停止时自动关闭；相机切换横竖屏时，画面方向同步调整。SteamVR Dashboard 面板显示 READY / RECORDING 状态，录制开关通过手柄动作绑定触发。界面提供简体中文、英文和日文，关闭主窗口后程序保留在托盘。

## 补充说明

- 仅支持 Windows，且只接收本机 Spout 发送端。
- 录制内容为 VRChat 相机的输出，不包含桌面或其他窗口。
- 配置文件位于 `%APPDATA%\vrc-recorder\config.json`。如遇源连接或录制异常，可查看 `%LOCALAPPDATA%\vrc-recorder\vrc-recorder.log`。

## 从源码构建

构建环境要求：Windows 11、Rust（edition 2024）、Visual Studio 2022 的 C++ 桌面开发工作负载，以及 CMake。

1. 初始化并编译 Spout2：

   ```powershell
   powershell -File scripts/setup_spout.ps1
   ```

2. 准备 FFmpeg。构建时会将 FFmpeg 与 `Spout.dll` 一并嵌入程序，默认从 `C:\Program Files\Virtual Desktop Streamer\ffmpeg.exe` 读取；安装 Virtual Desktop Streamer 即可满足该要求。

3. 执行编译：

   ```bash
   cargo build --release
   ```

4. 构建产物位于 `target/release/vrc-recorder.exe`。

Spout 相关环境变量已在 `.cargo/config.toml` 中配置，无需手动设置。

## 技术实现

后端使用 Rust 与 Tauri v2，前端为原生 HTML/JavaScript。视频通过 Spout SDK 接收，经 FFmpeg 编码；音频由 cpal 采集；SteamVR 集成通过 OpenVR 接口实现。
