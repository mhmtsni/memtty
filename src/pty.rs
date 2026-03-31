use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use tokio::sync::mpsc;
use tokio::time::{Duration, MissedTickBehavior};

pub enum PtyInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

const BUFF_CAPACITY: usize = 1096;
const BATCH_LENGTH: usize = 8192;
const FLUSH_INTERVAL: u64 = 2;

pub async fn run(
    tx_ui: mpsc::Sender<Vec<u8>>,
    mut rx_ui: mpsc::UnboundedReceiver<PtyInput>,
) -> Result<(), Box<dyn std::error::Error>> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // Spawn the shell
    let mut cmd = CommandBuilder::new("zsh");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "custom-terminal");
    cmd.env("TERM_PROGRAM_VERSION", "0.1.0");

    pair.slave.spawn_command(cmd)?;

    // We must drop the slave after spawning, or the master read will never EOF
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let master = pair.master;

    let (tx_chunks, mut rx_chunks) = mpsc::channel::<Vec<u8>>(64);

    // --- Thread: Read from PTY, send to UI ---
    tokio::task::spawn_blocking(move || {
        let mut read_buf = [0u8; BUFF_CAPACITY];
        loop {
            match reader.read(&mut read_buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx_chunks.blocking_send(read_buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    tokio::spawn(async move {
        let mut batch = Vec::with_capacity(BATCH_LENGTH);
        let mut ticker = tokio::time::interval(Duration::from_millis(FLUSH_INTERVAL));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_chunk = rx_chunks.recv() => {
                    match maybe_chunk {
                        Some(chunk) => {
                            batch.extend_from_slice(&chunk);
                            if batch.len() >= 8192 {
                                if tx_ui.send(std::mem::take(&mut batch)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        None => {
                            if !batch.is_empty() {
                                let _ = tx_ui.send(std::mem::take(&mut batch)).await;
                            }
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !batch.is_empty() {
                        if tx_ui.send(std::mem::take(&mut batch)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    while let Some(input) = rx_ui.recv().await {
        match input {
            PtyInput::Data(bytes) => {
                writer.write_all(&bytes)?;
            }
            PtyInput::Resize { cols, rows } => {
                master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })?;
            }
        }
    }

    Ok(())
}
