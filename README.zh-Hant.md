# 拍拍你的小龍蝦🦞

> [English](README.md) | 中文（繁體）

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Apple%20Silicon-black.svg)](https://support.apple.com/en-us/116943)

> 拍拍你的小龍蝦，你的 AI 小龍蝦會（嘴上）拍回來。

**拍拍你的小龍蝦🦞** 是一個 Rust 命令列工具，透過內建加速度計偵測 Apple Silicon MacBook 上的物理拍打和晃動，然後告訴你的 [OpenClaw](https://www.npmjs.com/package/@turquoisebay/openclaw) 代理發生了什麼——讓它在 Discord 上當場吐槽你。

```
你: *拍了筆電一下*
openclaw: "那是拍打還是你打字太爛了？"
```

## 目錄

- [這東西為什麼存在？](#這東西為什麼存在)
- [運作方式](#運作方式)
- [運作模式](#運作模式)
- [系統需求](#系統需求)
- [快速開始](#快速開始)
- [嚴重等級](#嚴重等級)
- [事件類型](#事件類型)
- [命令列參考](#命令列參考)
- [事件內容](#事件內容)
- [偵測演算法](#偵測演算法)
- [專案結構](#專案結構)
- [啟動流程](#啟動流程)
- [防誤報措施](#防誤報措施)
- [調校建議](#調校建議)
- [OpenClaw 智慧代理提示詞建議](#openclaw-智慧代理提示詞建議)
- [測試](#測試)
- [疑難排解](#疑難排解)
- [參與貢獻](#參與貢獻)
- [致謝](#致謝)
- [授權條款](#授權條款)

## 這東西為什麼存在？

因為有人看了一眼每台 Apple Silicon MacBook 裡的博世 BMI286 加速度計，然後想：「要是我的筆電能感受到疼痛呢？」

這個工具以 800Hz 頻率讀取原始 IMU 資料，透過地震學等級的偵測演算法處理（原本是為地震偵測設計的，現在被徵用來偵測辦公室筆電虐待行為），將衝擊分為 6 個嚴重等級，從「那是蝴蝶嗎？」到「你這個惡魔」，然後把事件發送給你的 OpenClaw 智慧代理，由它的提示詞決定怎麼回應。

你的 MacBook 早就在默默地評判你了。現在它可以大聲說出來了。

## 運作方式

```
                    你的手
                        |
                        | (暴力行為)
                        v
┌─────────────────────────────────────┐
│  Apple Silicon MacBook              │
│  ┌───────────────────────────────┐  │
│  │ 博世 BMI286 IMU              │  │
│  │ (加速度計, ~800Hz 原始頻率)  │  │
│  └──────────────┬────────────────┘  │
└─────────────────┼───────────────────┘
                  │
                  │ IOKit HID (需要 sudo，因為
                  │ 蘋果也不信任你)
                  v
    ┌─────────────────────────────┐
    │ C 適配層 (iokit.c)          │
    │ - 喚醒 SPU 感測器驅動      │
    │ - 自動鎖定加速度計 HID     │
    │ - 800Hz → 100Hz 降採樣     │
    │ - 無鎖環形緩衝區           │
    └──────────────┬──────────────┘
                   │
                   │ Q16 定點數 → 重力加速度 (g)
                   v
    ┌─────────────────────────────┐
    │ 偵測器 (純 Rust)            │
    │ ┌─────────┐ ┌────────────┐ │
    │ │ STA/LTA │ │   CUSUM    │ │
    │ │(3 尺度) │ │ (漂移偵測) │ │
    │ ├─────────┤ ├────────────┤ │
    │ │ 峰度    │ │ Peak/MAD   │ │
    │ │(脈衝)   │ │ (離群點)   │ │
    │ └─────────┘ └────────────┘ │
    │                             │
    │ 高通濾波器移除重力分量      │
    │ (你的筆電大概沒在墜落)      │
    └──────────────┬──────────────┘
                   │
                   │ 事件: 類型 + 嚴重等級 + 振幅
                   v
    ┌─────────────────────────────┐
    │ 分類                        │
    │                             │
    │ 拍打 = 短脈衝 (<100ms)     │
    │ 晃動 = 持續振盪 (>200ms)   │
    │                             │
    │ 6 個嚴重等級                │
    │ (見下表)                    │
    └──────────────┬──────────────┘
                   │
                   │ 冷卻時間 + 振幅過濾
                   v
    ┌─────────────────────────────┐
    │ openclaw agent --message    │
    │ "SLAP_EVENT level=5 ..."   │
    │                             │
    │ 代理收到事件，              │
    │ 產生一條機智的回應，        │
    │ 可選發送到                  │
    │ Discord / Slack / 其他平台  │
    └─────────────────────────────┘
```

## 運作模式

本工具支援兩種模式：

| 模式 | 指令 | 說明 |
|------|------|------|
| **Standalone** (預設) | `sudo slap-your-openclaw` | 偵測事件後呼叫 `openclaw agent` CLI |
| **MCP Server** | `sudo slap-your-openclaw mcp` | 透過 stdio 提供 MCP 工具，供 AI 代理整合 |

兩種模式共用相同的感測器執行緒與偵測迴圈，差別在於事件的輸出方式。

### MCP 工具

| 工具 | 說明 |
|------|------|
| `slap_status` | 偵測器階段、已處理樣本數、感測器健康度、運行時間 |
| `slap_get_events` | 近期事件歷史（可依數量、最低等級篩選） |
| `slap_wait_for_event` | 阻塞等待事件發生或逾時 |
| `slap_get_config` | 取得目前的執行期設定 |
| `slap_set_config` | 動態更新設定（冷卻時間、閾值等） |

## 系統需求

- **Apple Silicon Mac**（M1、M2、M3、M4 — 任何型號）
- **Root 權限**（`sudo`）— IOKit HID 加速度計存取需要
- **Rust 工具鏈** — 建議使用 `rustup`
- **OpenClaw CLI** 在 PATH 中（或使用 `--openclaw-bin` 指定）
  - 安裝：`npm i -g @turquoisebay/openclaw`
  - 或使用 `standalone --local` 模式測試，不需要 OpenClaw

## 快速開始

### 1. 建置

```bash
git clone https://github.com/sinhong2011/slap-your-openclaw
cd slap-your-openclaw
cargo build --release
```

### 2. 本機測試（不需要 OpenClaw）

```bash
sudo ./target/release/slap-your-openclaw standalone --local
```

你會看到暖機進度條，接著進入布防階段。當 `detector: ready` 出現時，就可以拍你的筆電，看事件印到螢幕上。

```
warmup: [#########################] 0.0s remaining
arming: [#########################] 0.0s remaining
detector: [#########################] ready
>>> SLAP #5 [CHOC_MOYEN  amp=0.04231g] sources=["STA/LTA", "CUSUM", "PEAK"]
```

如果什麼都沒出現：拍用力一點。這不是觸控螢幕。

### 3. 連接 OpenClaw

```bash
sudo ./target/release/slap-your-openclaw
```

預設情況下，每次偵測到事件都會呼叫 `openclaw agent --message "SLAP_EVENT ..."`。你的 OpenClaw 代理會怎麼回，取決於它的系統提示詞。

### 4. 發送到 Discord

```bash
sudo ./target/release/slap-your-openclaw standalone \
  --openclaw-deliver \
  --openclaw-reply-channel discord \
  --openclaw-reply-to "channel:1234567890" \
  --openclaw-thinking off \
  --openclaw-timeout 8
```

現在你每次拍筆電，它都會在 Discord 上公開羞辱你。

### 5. MCP Server 模式

```bash
sudo ./target/release/slap-your-openclaw mcp
```

以 stdio MCP 伺服器啟動，AI 代理可透過標準 MCP 協定呼叫 `slap_status`、`slap_wait_for_event` 等工具來即時監控拍打事件。

## 嚴重等級

你的筆電是個戲精。它把衝擊分為 6 個等級：

| 等級 | 名稱 | 發生了什麼 | 你筆電的心情 |
|------|------|-----------|-------------|
| 1 | MICRO_VIB | 你在旁邊呼吸了一下 | 「剛才有動靜嗎？」 |
| 2 | VIB_LEGERE | 打字太用力了 | 「我有感覺到喔」 |
| 3 | VIBRATION | 桌子被撞到、隔壁關門 | 「不好意思？？」 |
| 4 | MICRO_CHOC | 輕拍、用力敲 | 「你不是認真的吧」 |
| 5 | CHOC_MOYEN | 紮紮實實一巴掌 | 「報警！報警！」 |
| 6 | CHOC_MAJEUR | 全力出擊，所有演算法同時尖叫 | 「我要打 AppleCare 電話了」 |

分類基於有多少偵測演算法同意發生了什麼以及振幅有多大。當 4 個偵測器同時觸發時，你的筆電知道你是認真的。

## 事件類型

| 類型 | 持續時間 | 範例 |
|------|---------|------|
| **SLAP（拍打）** | < 100ms STA/LTA 啟動時間 | 快速擊打、敲擊 |
| **SHAKE（晃動）** | > 200ms 持續振盪 | 憤怒地拿起筆電、桌面震動 |

100-200ms 之間的事件會被分類為 UNKNOWN 並直接忽略——你的筆電很困惑，選擇不發表評論。

## 命令列參考

```
slap-your-openclaw [選項] [指令]
```

指令：`standalone`（預設）、`mcp`

> `--local` 與所有 `--openclaw-*` 參數屬於 standalone 專用。請用 `slap-your-openclaw standalone ...`。

### 偵測調校

| 參數 | 環境變數 | 預設值 | 說明 |
|------|---------|-------|------|
| `--cooldown <MS>` | `SLAP_COOLDOWN` | `500` | 事件之間的最小冷卻時間（毫秒） |
| `--min-level <1-6>` | `SLAP_MIN_LEVEL` | `4` | 忽略低於此等級的事件 |
| `--min-slap-amp <G>` | `SLAP_MIN_SLAP_AMP` | `0.010` | 最小拍打振幅（g） |
| `--min-shake-amp <G>` | `SLAP_MIN_SHAKE_AMP` | `0.030` | 最小晃動振幅（g） |

### OpenClaw 整合（standalone 模式）

| 參數 | 環境變數 | 預設值 | 說明 |
|------|---------|-------|------|
| `--openclaw-agent <ID>` | `OPENCLAW_AGENT` | `main` | 處理拍打事件的 OpenClaw 智慧代理 |
| `--openclaw-session-id <ID>` | `OPENCLAW_SESSION_ID` | `slap-detector` | 拍打流量的工作階段隔離 ID |
| `--openclaw-thinking <LEVEL>` | `OPENCLAW_THINKING` | `off` | 思考層級：off/minimal/low/medium/high |
| `--openclaw-timeout <SEC>` | `OPENCLAW_TIMEOUT` | `20` | 等待智慧代理回應的逾時時間 |
| `--local` | — | `false` | 輸出 JSON 到標準輸出，略過 OpenClaw |
| `--openclaw-deliver` | `OPENCLAW_DELIVER` | `false` | 將智慧代理回覆投遞到頻道 |
| `--openclaw-reply-channel <NAME>` | `OPENCLAW_REPLY_CHANNEL` | — | 例如 `discord` |
| `--openclaw-reply-to <TARGET>` | `OPENCLAW_REPLY_TO` | — | 例如 `user:123` 或 `channel:456` |
| `--openclaw-run-as <USER>` | `OPENCLAW_RUN_AS` | `$SUDO_USER` | 以此使用者身分執行 openclaw CLI |
| `--openclaw-bin <PATH>` | `OPENCLAW_BIN` | `openclaw` | OpenClaw 可執行檔路徑 |

> **為什麼需要 `--openclaw-run-as`？** 因為你用 `sudo` 執行這個工具，但 OpenClaw 需要你使用者的設定/憑證。預設使用 `$SUDO_USER` 把權限降回給你。

## 事件內容

每個事件作為結構化訊息傳送給 OpenClaw：

```
SLAP_EVENT level=5 severity=CHOC_MOYEN amplitude=0.04231g correlationId=slap-a1b2c3d4
```

或者晃動事件：

```
SHAKE_EVENT level=4 severity=MICRO_CHOC amplitude=0.01500g correlationId=slap-e5f6g7h8
```

傳輸保持結構化，而讓你的 OpenClaw 智慧代理的提示詞決定回應的語氣。想讓你的代理像失望的父母一樣回應？像個戲精？像個淡定的禪師？那是提示詞的問題，不是偵測的問題。

## 偵測演算法

四種演算法在每個採樣點上平行運行。對於「有人拍了筆電」來說這確實大材小用了，但我們就是來玩訊號處理的。

### STA/LTA（短期平均 / 長期平均）

借鑑自地震學。在 3 個時間尺度上比較近期能量與背景能量：

| 尺度 | 短窗口 | 長窗口 | 靈敏度 |
|------|-------|-------|--------|
| 快速 | 3 個採樣 (30ms) | 100 個採樣 (1s) | 捕捉尖銳脈衝 |
| 中等 | 15 個採樣 (150ms) | 500 個採樣 (5s) | 捕捉中等衝擊 |
| 慢速 | 50 個採樣 (500ms) | 2000 個採樣 (20s) | 捕捉持續擾動 |

當比值超過啟動閾值時，通道啟動。啟動持續時間決定是拍打還是晃動。

### CUSUM（累積和）

漂移偵測——累積與執行均值的偏差。像記仇一樣，小偏移不斷累積直到突破閾值。

### 峰度（Kurtosis）

在 100 個採樣的窗口上測量訊號分佈的「尖峭度」。正常雜訊的峰度約為 3。脈衝式拍打會使其飆升到 6 以上。簡單說就是：「這看起來像不像有人打了什麼東西？」

### Peak/MAD（中位絕對偏差）

在 200 個採樣的窗口上進行穩健離群點偵測。如果目前的採樣與中位數（MAD 估計）偏離超過 4 個標準差，說明剛才發生了異常。

## 專案結構

```
src/
├── main.rs            # CLI + 暖機/就緒互動 + 主迴圈 + 模式分派
├── config.rs          # clap 衍生 CLI 參數 + 環境變數 + 子指令
├── shared.rs          # SharedState, DetectorConfig, run_detection_loop()
├── openclaw.rs        # OpenClaw 發佈器（生成 `openclaw agent` 子程序）
├── sensor/
│   ├── mod.rs         # 模組匯出
│   ├── iokit.rs       # Rust FFI: 環形緩衝區讀取器, Q16→g 轉換
│   └── iokit.c        # C 適配層: IOKit HID, SPU 驅動喚醒, 裝置自動鎖定
├── detector/
│   ├── mod.rs         # 4 種偵測演算法 + 嚴重等級分類器
│   └── ring.rs        # 固定容量環形緩衝區 (RingFloat)
└── mcp/
    ├── mod.rs         # MCP 模組宣告
    └── server.rs      # SlapServer: 5 個 MCP 工具（rmcp）
```

### 為什麼用 C 適配層？

IOKit 和 CoreFoundation 是 C 框架。你*可以*透過原始 FFI 從 Rust 呼叫它們，但那意味著 200 多行 `extern "C"` 宣告、不透明型別轉換和 `CFRelease` 編排。C 適配層約 240 行，處理所有 macOS 框架呼叫，向 Rust 暴露 3 個函式：

```c
int iokit_sensor_init(void);    // 歸零環形緩衝區
void iokit_sensor_run(void);    // 喚醒感測器 + 執行 CFRunLoop（阻塞）
const uint8_t* iokit_ring_ptr(void);  // 共享環形緩衝區指標
```

### 裝置自動鎖定

Apple Silicon Mac 透過 `AppleSPUHIDDevice` 暴露 4-8 個 HID 裝置。其中只有一個是加速度計。C 適配層使用投票系統自動偵測正確的裝置：

1. 開啟所有廠商頁面（`0xFF00`）HID 裝置
2. 過濾 22 位元組 IMU 格式的報告
3. 驗證原始 L1 範數在合理重力範圍（0.5g–4g）
4. 同一裝置連續 3 次有效報告 → 鎖定裝置
5. 同一報告 ID 連續 6 次有效報告 → 鎖定報告

這意味著同一個二進位檔可以在 M1、M2、M3、M4 上執行，無需硬編碼裝置索引。

## 啟動流程

執行工具時，你會看到：

```
iokit: woke 8 SPU drivers
iokit: device 1: UsagePage=0xff00 Usage=255
iokit: registered accel callback on idx=0 usage=255
...
iokit: locked accelerometer device idx=0 usage=255
iokit: locked accelerometer reportID=0
warmup: [################---------] 0.9s remaining
arming: [#########################] 0.0s remaining
detector: [#########################] ready
```

**階段一 — 暖機（2s）：** 高通濾波器和執行平均值需要時間穩定。暖機期間事件被抑制。

**階段二 — 布防（1s）：** 暖機後再給一小段安靜時間，讓統計值穩定。這段期間仍會抑制事件。

**階段三 — 準備完成：** 偵測器已上線。你的筆電現在情緒就緒。

## 防誤報措施

因為沒人希望自己打封郵件的時候筆電在那大喊「遇襲了」：

1. **暖機門控** — 前 200 個採樣（2s）完全抑制
2. **布防門控** — 額外 100 個採樣（1s）的安靜穩定期
3. **UNKNOWN 事件丟棄** — 只發佈 SLAP 和 SHAKE
4. **防打字誤判** — 沒有 PEAK 偵測來源且振幅 < 0.03g 的 SLAP 事件會直接忽略（鍵盤產生的低振幅微震動看起來像輕拍）
5. **振幅下限** — SLAP（0.01g）和 SHAKE（0.03g）分別可設定最小值
6. **嚴重等級過濾** — 預設 `--min-level 4` 完全忽略 1-3 級
7. **冷卻時間** — 事件之間最少 500ms
8. **事件合併** — 如果 OpenClaw 子行程還在執行時新事件到達，只發送最新事件（突發保護）

## 調校建議

**太靈敏了？**（打字、桌面碰撞都會觸發）
```bash
sudo ./target/release/slap-your-openclaw --min-level 5 --min-slap-amp 0.025
```

**不夠靈敏？**（需要揍一拳才能觸發）
```bash
sudo ./target/release/slap-your-openclaw --min-level 3 --min-slap-amp 0.005 --min-shake-amp 0.010
```

**被洗版了？**（連續太多事件）
```bash
sudo ./target/release/slap-your-openclaw --cooldown 3000  # 3 秒冷卻時間
```

## OpenClaw 智慧代理提示詞建議

你的 OpenClaw 代理會收到結構化事件字串。建議使用這份系統提示詞（與 `skill/SKILL.md` 一致）：

```
你連接到一個 Apple Silicon MacBook 的實體拍打/晃動偵測器。
僅在符合以下任一條件時才套用本段規則：
- senderId 是 "slap-detector" 或 "slap"
- text 以 SLAP_EVENT 或 SHAKE_EVENT 開頭
- text 包含 SLAP DETECTED!
- text 符合 SLAP #<level> <severity> 或 SHAKE #<level> <severity>
其他所有訊息都忽略本段規則。

當收到 SLAP_EVENT 或 SHAKE_EVENT 時，用戲劇化但好玩的語氣回應。

嚴重等級對應：
- 等級 1-2（MICRO_VIB / VIB_LEGERE）：幾乎不理會
- 等級 3（VIBRATION）：帶點疑惑
- 等級 4（MICRO_CHOC）：不爽但克制
- 等級 5（CHOC_MOYEN）：戲劇化抗議
- 等級 6（CHOC_MAJEUR）：全面浮誇暴怒

行為規則：
- SHAKE 與 SLAP 要分開處理（粗魯晃動 vs 人身攻擊）
- 連續短時間重複事件要升級語氣
- 振幅很大時可直接點名數值
- 保持好玩、戲精，但不要真的惡意
```

範例對話：

```
輸入:  SLAP_EVENT level=5 severity=CHOC_MOYEN amplitude=0.04g correlationId=slap-abc123
輸出: "我有 AppleCare+，但我覺得它不保家庭暴力。請尋求幫助。"
```

## 測試

```bash
cargo test        # 單元測試（偵測器、環形緩衝區、設定、MCP、整合路徑）
cargo clippy      # 程式碼檢查
cargo fmt --check # 格式檢查
```

測試使用合成加速度計資料——CI 過程中無需實際的筆電暴力行為。

## 疑難排解

**"requires root privileges"**
→ 使用 `sudo` 執行。IOKit HID 需要它，沒有繞過方法。

**"Failed to initialize IOKit HID sensor"**
→ 不是 Apple Silicon，或者你的 Mac 沒有 BMI286 IMU。只支援 M 系列晶片。

**偵測不到事件**
→ 等待「detector: ready」訊息出現。用力拍掌托區域（不是螢幕，拜託）。檢查 `--min-level` 是否設得太高。

**打字時觸發事件**
→ 提高 `--min-slap-amp`（試試 `0.020` 或 `0.025`）。防打字誤判功能能擋住大多數情況，但某些 MacBook 型號上的重度打字者可能需要更高的閾值。

**"openclaw exited with status 1"**
→ 檢查 `openclaw` 已安裝且智慧代理存在。先手動試試 `openclaw agent --message "test"`。

**進度條卡住**
→ 感測器執行緒可能失敗了。檢查上方的 iokit 日誌行是否有錯誤。某些 M4 Mac 上感測器的 usage page 可能不同——自動鎖定系統應該能處理，但如果不行請提 issue。

## 參與貢獻

歡迎貢獻！本專案還在早期開發階段，有很多可以改進的地方。

### 開發環境設定

```bash
git clone https://github.com/sinhong2011/slap-your-openclaw
cd slap-your-openclaw
cargo build
```

### 執行測試

```bash
cargo test
cargo clippy
cargo fmt --check
```

### 歡迎貢獻的方向

- **硬體測試** — 在不同 MacBook 型號（M1/M2/M3/M4）上試用並回報表現
- **偵測調校** — 改善誤報過濾或提出新的演算法
- **新輸出模式** — OpenClaw 和 MCP 以外的額外整合
- **文件** — 翻譯、教學或改善疑難排解指南

請在開始大型變更前先開 issue 討論方向。

## 致謝

偵測演算法移植自：
- [taigrr/spank](https://github.com/taigrr/spank) — 原版 Go 實作
- [taigrr/apple-silicon-accelerometer](https://github.com/taigrr/apple-silicon-accelerometer)

使用的函式庫：
- [clap](https://docs.rs/clap) — CLI 框架
- [tokio](https://tokio.rs) — 非同步執行時期
- [cc](https://docs.rs/cc) — C 適配層編譯
- [rmcp](https://docs.rs/rmcp) — MCP 伺服器框架

## 授權條款

本專案採用 MIT 授權條款——詳見 [LICENSE](LICENSE) 檔案。

請負責任地拍打。
