# Sanma Mahjong AI

A 3-player (sanma) mahjong AI training pipeline and gameplay assistant, built on top of
[Mortal](https://github.com/Equim-chan/Mortal),
[mjai-reviewer](https://github.com/Equim-chan/mjai-reviewer), and
[MahjongCopilot](https://github.com/latorc/MahjongCopilot).

## Repository Layout

| Directory | Description |
|-----------|-------------|
| `Mortal/` | Offline RL training framework (libriichi Rust engine + Python trainer). Modified for sanma v5. |
| `mjai-reviewer/` | Rust tool for reviewing game logs with a Mortal AI engine. |
| `MahjongCopilot/` | Python GUI copilot for Majsoul — shows step-by-step AI guidance during a live game. |
| `tenhou_dl/` | Rust tool for downloading sanma game logs from Tenhou archives. |
| `models/` | Model weights (`*.pth`). Not tracked by git — created on first training run. |
| `data/` | Training data in mjai JSONL format. Not tracked by git — you supply this. |
| `runs/` | TensorBoard logs. Not tracked by git. |
| `template_log/` | Sample Tenhou/Majsoul log files for testing the reviewer. |

## Prerequisites

- **Conda** (`conda` or `mamba`)
- **Rust** toolchain (stable) — install via [rustup](https://rustup.rs)
- Python 3.11+

## Quick Start

### 1. Create the conda environment

```bash
conda env create -f Mortal/environment.yml
conda activate mortal
```

This installs Python, PyTorch, maturin, and all other dependencies.

### 2. Build libriichi

`libriichi` is a Rust extension used by the trainer, MahjongCopilot, and the reviewer.

**macOS / Linux:**
```bash
cd Mortal/libriichi
RUSTFLAGS="" maturin develop --release
cd ../..
```
> `RUSTFLAGS=""` clears any pyenv linker flags that conflict with conda's Python. Omit it if you are not using pyenv.

**Windows (PowerShell):**
```powershell
cd Mortal\libriichi
maturin develop --release
cd ..\..
```

### 3. Build tools

```bash
cd mjai-reviewer && cargo build --release && cd ..
cd tenhou_dl    && cargo build --release && cd ..
```

### 4. Create the data and models directories

```bash
mkdir -p models data runs        # macOS / Linux
mkdir models data runs           # Windows (cmd)
```

## Preparing Training Data

Training requires mjai-format JSONL files in `data/`. The full pipeline to get them from Tenhou:

### Step 1 — Download Tenhou monthly archives

Tenhou publishes monthly gz archives of all game logs. Download the sanma (三鳳) gz files from:
```
https://tenhou.net/sc/raw/
```
Save them to a local directory, e.g. `tenhou_gz/`.

### Step 2 — Download game JSONs from archives

`tenhou_dl` reads the gz index files, extracts sanma game IDs, and downloads the logs as JSON.

```bash
# macOS / Linux
./tenhou_dl/target/release/tenhou_dl \
  --format gz --mode 3 \
  --input tenhou_gz/ \
  --output tenhou_json/

# Windows
.\tenhou_dl\target\release\tenhou_dl.exe `
  --format gz --mode 3 `
  --input tenhou_gz\ `
  --output tenhou_json\
```

`--mode 3` selects 三鳳南喰赤 (sanma phoenix south, red dora). Concurrent downloads default to 10; tune with `--download N`.

### Step 3 — Convert Tenhou JSON → mjai JSONL

`convlog` (part of mjai-reviewer) converts the Tenhou JSON files to mjai JSONL format using all CPU cores.

```bash
# macOS / Linux
cd mjai-reviewer
cargo run --example convert_dir --release -- ../tenhou_json/ ../data/
cd ..

# Windows
cd mjai-reviewer
cargo run --example convert_dir --release -- ..\tenhou_json\ ..\data\
cd ..
```

Each input `.json` becomes one `.jsonl` file in `data/`. The trainer picks up all `data/*.jsonl`.

## Training

All scripts run from `Mortal/mortal/` with `config.sanma.toml`. Paths in the config are relative to that directory.

### Stage 1 — GRP (Global Reward Predictor)

Train this first — the main trainer depends on it for reward shaping.

```bash
cd Mortal/mortal
$env:MORTAL_CFG=".\config.sanma.toml"; python train_grp.py   # Windows
MORTAL_CFG=./config.sanma.toml python train_grp.py           # macOS/Linux
```

Saves to `models/grp_sanma.pth`. Watch `val_acc` in TensorBoard — stop when it plateaus (typically 45–55%).

### Stage 1 — Offline RL (main model)

Starts from random weights if `models/mortal_sanma.pth` does not exist.

```bash
$env:MORTAL_CFG=".\config.sanma.toml"; python train.py   # Windows
MORTAL_CFG=./config.sanma.toml python train.py           # macOS/Linux
```

Checkpoints saved every 400 steps. Watch `dqn_loss` in TensorBoard — stop when it plateaus.

### Stage 2 — Online self-play

After behavior cloning converges, switch to online self-play. Set `online = true` in `config.sanma.toml`, then launch three processes in separate terminals from `Mortal/mortal/`:

```powershell
# Terminal 1 — replay buffer server (start this first)
$env:MORTAL_CFG=".\config.sanma.toml"; python server.py

# Terminal 2 — self-play worker (run multiple for more throughput)
$env:MORTAL_CFG=".\config.sanma.toml"; python client.py

# Terminal 3 — trainer
$env:MORTAL_CFG=".\config.sanma.toml"; python train.py
```

The trainer pushes updated weights to the server every `submit_every` steps; workers pull them and generate new games.

### Evaluation (1v2)

Pits the challenger model against two copies of the champion to measure improvement.

```bash
$env:MORTAL_CFG=".\config.sanma.toml"; python one_vs_two.py   # Windows
MORTAL_CFG=./config.sanma.toml python one_vs_two.py           # macOS/Linux
```

## Reviewing a Game Log

**macOS / Linux:**
```bash
cd mjai-reviewer
conda run -n mortal \
  ./target/release/mjai-reviewer \
    -e mortal \
    --mortal-exe ../Mortal/mortal/mortal \
    --mortal-cfg ../Mortal/mortal/config.toml \
    -i path/to/game.json \
    -a 0
```

**Windows (PowerShell):**
```powershell
cd mjai-reviewer
conda run -n mortal `
  .\target\release\mjai-reviewer.exe `
    -e mortal `
    --mortal-exe ..\Mortal\mortal\mortal.bat `
    --mortal-cfg ..\Mortal\mortal\config.toml `
    -i path\to\game.json `
    -a 0
```

`-a` is the player seat to review (0–2). The output HTML report opens automatically.

## MahjongCopilot (Live Assistant)

```bash
cd MahjongCopilot
pip install -r requirements.txt
playwright install chromium
python main.py
```

In the settings UI, select **Local 3P** model type and point it at `models/mortal_sanma.pth`.

## Model Architecture (v5 sanma)

| Parameter | Value |
|-----------|-------|
| Input channels (obs) | 774 |
| ResNet blocks | 40 |
| Conv channels | 192 |
| Action space | 44 |
| GRP hidden size | 64 × 2 layers |

> **GRP** stands for **Global Reward Predictor** — a GRU-based model trained to predict final rank distributions from game state sequences. It is used during training for reward shaping and during review mode to display win-rate estimates.

## License

Each subdirectory retains its original license:
- `Mortal/` — [AGPL-3.0-or-later](Mortal/LICENSE) — Copyright (C) 2021-2022 Equim
- `mjai-reviewer/` — [Apache-2.0](mjai-reviewer/LICENSE)
- `MahjongCopilot/` — [GPL-3.0](MahjongCopilot/LICENSE)
- `tenhou_dl/` — MIT
