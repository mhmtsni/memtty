use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
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
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = tx_ui.blocking_send(buf[..n].to_vec());
                }
                Err(_) => break,
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
