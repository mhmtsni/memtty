use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

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

    // --- Thread: Read from PTY, send to UI ---
    tokio::task::spawn_blocking(move || {
        let mut read_buf = [0u8; 1024];
        let mut batch = Vec::with_capacity(32768);
        let mut last_flush = Instant::now();
        let flush_interval = Duration::from_millis(8);
        let max_batch_size = 8192;
        loop {
            match reader.read(&mut read_buf) {
                Ok(0) => {
                    if !batch.is_empty() {
                        let _ = tx_ui.blocking_send(std::mem::take(&mut batch));
                    }
                    break;
                }
                Ok(n) => {
                    batch.extend_from_slice(&read_buf[..n]);

                    let should_flush =
                        batch.len() >= max_batch_size || last_flush.elapsed() >= flush_interval;

                    if should_flush {
                        let _ = tx_ui.blocking_send(std::mem::take(&mut batch));
                        last_flush = Instant::now();
                    }
                }
                Err(_) => break,
            }

            if !batch.is_empty() && last_flush.elapsed() >= flush_interval {
                let _ = tx_ui.blocking_send(std::mem::take(&mut batch));
                last_flush = Instant::now();
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
