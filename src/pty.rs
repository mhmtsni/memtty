use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use tokio::sync::mpsc;
use tokio::time::{Duration, MissedTickBehavior};

pub enum PtyInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

pub async fn run(
    tx_ui: mpsc::Sender<Vec<u8>>,
    mut rx_ui: mpsc::Receiver<PtyInput>, // Changed from Vec<u8> to PtyInput
) -> Result<(), Box<dyn std::error::Error>> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // Spawn the shell
    pair.slave.spawn_command(CommandBuilder::new("zsh"))?;

    // We must drop the slave after spawning, or the master read will never EOF
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;
    let master = pair.master;

    let (tx_chunks, mut rx_chunks) = mpsc::channel::<Vec<u8>>(64);

    // --- Thread: Read from PTY, send to UI ---
    tokio::task::spawn_blocking(move || {
        let mut read_buf = [0u8; 1024];
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

    // Batch chunks and flush periodically so output does not wait for a new read.
    tokio::spawn(async move {
        let mut batch = Vec::with_capacity(8192);
        let mut ticker = tokio::time::interval(Duration::from_millis(8));
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
                writer.flush()?;
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
