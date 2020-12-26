// Copyright (C) 2020  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

//! Async wrapper for `I2cdev` - runs I2cDevice in a separate thread and forwards
//! requests from async tasks.

use ii_logging::macros::*;

use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::executor::block_on;
use futures::stream::StreamExt;
use ii_async_compat::{futures, tokio};
use tokio::task;

/// For devmem
use nix::sys::mman::{MapFlags, ProtFlags};
use std::fs::OpenOptions;
use std::os::unix::prelude::AsRawFd;

use std::thread;
use std::time::Duration;

use embedded_hal::blocking::i2c::{Read, Write};
use linux_embedded_hal::I2cdev;

use thiserror::Error;

/// Delay constant for I2C controller
const I2C_CONTROLLER_RESET_DELAY: Duration = Duration::from_millis(850);

#[derive(Error, Debug)]
pub enum Error {
    #[error("I2C error")]
    I2cError {
        #[from]
        source: linux_embedded_hal::i2cdev::linux::LinuxI2CError,
    },
}

pub type Result<T> = std::result::Result<T, self::Error>;

/// Utility function to get raw access to registers of I2C controller
fn devmem_write_u32(address: usize, value: u32) {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mem")
        .expect("BUG: cannot open /dev/mem");

    let page_size = 4096;
    let raw_ptr = unsafe {
        nix::sys::mman::mmap(
            0 as *mut libc::c_void,
            page_size,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED,
            f.as_raw_fd(),
            (address & !(page_size - 1)) as libc::off_t,
        )
        .expect("BUG: failed to mmap /dev/mem")
    };
    let data_ptr = (raw_ptr as usize + (address & (page_size - 1))) as *mut libc::c_void;
    let data_ptr = unsafe { &mut *(data_ptr as *mut u32) };
    *data_ptr = value;
    unsafe { nix::sys::mman::munmap(raw_ptr, page_size) }.expect("BUG: munmap failed");
}

/// Reset I2C controller #0
fn i2c_reset(reset_enable: bool) {
    devmem_write_u32(0xF8000224, if reset_enable { 1 } else { 0 });
}

enum Request {
    Read {
        address: u8,
        num_bytes: usize,
        /// Channel used to send back result
        reply: oneshot::Sender<Result<Vec<u8>>>,
    },
    Write {
        address: u8,
        bytes: Vec<u8>,
        /// Channel used to send back result
        reply: oneshot::Sender<Result<()>>,
    },
    ResetI2cController {
        reply: oneshot::Sender<Result<()>>,
    },
}

/// Server for I2C read/write requests
/// Runs in separate thread.
/// Terminates when all request sender sides are dropped.
fn serve_requests(
    path: String,
    mut i2c_device: I2cdev,
    mut request_rx: mpsc::UnboundedReceiver<Request>,
) -> Result<()> {
    while let Some(request) = block_on(request_rx.next()) {
        match request {
            Request::Read {
                address,
                num_bytes,
                reply,
            } => {
                let mut bytes = vec![0; num_bytes];
                let result = i2c_device
                    .read(address, &mut bytes)
                    .map(|_| bytes)
                    .map_err(|e| e.into());
                if reply.send(result).is_err() {
                    warn!("AsyncI2c reply send failed - remote side may have ended");
                }
            }
            Request::Write {
                address,
                bytes,
                reply,
            } => {
                let result = i2c_device.write(address, &bytes).map_err(|e| e.into());
                if reply.send(result).is_err() {
                    warn!("AsyncI2c reply send failed - remote side may have ended");
                }
            }
            Request::ResetI2cController { reply } => {
                // Close I2C device
                drop(i2c_device);

                // Reset the controller
                i2c_reset(true);
                thread::sleep(I2C_CONTROLLER_RESET_DELAY);
                i2c_reset(false);
                thread::sleep(I2C_CONTROLLER_RESET_DELAY);

                // Open the device again (kernel will re-initialize the registers)
                i2c_device = I2cdev::new(&path).expect("BUG: failed to re-open i2c device");

                if reply.send(Ok(())).is_err() {
                    warn!("AsyncI2c reply send failed - remote side may have ended");
                }
            }
        }
    }
    info!("I2C device exiting");
    Ok(())
}

/// Clonable async I2C device. I2cDevice is closed when last sender channel is dropped.
pub struct AsyncI2cDev {
    request_tx: mpsc::UnboundedSender<Request>,
}

/// TODO: Make this into a trait, then implement different backends.
/// TODO: Write tests for this and for power controller (fake async I2C with power controller,
/// check power initialization goes as expected etc., maybe reuse I2C bus from sensors?).
/// TODO: Reuse traits from `i2c/i2c.rs`
impl AsyncI2cDev {
    /// Open I2C device
    /// Although this function is not async, it has to be called from within Tokio context
    /// because it spawns task in a separate thread that serves the (blocking) I2C requests.
    pub fn open(path: String, clear_reset: bool) -> Result<Self> {
        if clear_reset {
            info!("Clearing reset on I2C controller...");
            i2c_reset(false);
        }
        let i2c_device = I2cdev::new(&path)?;
        let (request_tx, request_rx) = mpsc::unbounded();

        // Spawn the future in a separate blocking pool (for blocking operations)
        // so that this doesn't block the regular threadpool.
        task::spawn_blocking(move || {
            if let Err(e) = serve_requests(path, i2c_device, request_rx) {
                error!("{}", e);
            }
        });

        Ok(Self { request_tx })
    }

    pub async fn read(&self, address: u8, num_bytes: usize) -> Result<Vec<u8>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let request = Request::Read {
            address,
            num_bytes,
            reply: reply_tx,
        };
        self.request_tx
            .unbounded_send(request)
            .expect("BUG: I2C request failed");
        reply_rx.await.expect("BUG: failed to receive I2C reply")
    }

    pub async fn write(&self, address: u8, bytes: Vec<u8>) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let request = Request::Write {
            address,
            bytes,
            reply: reply_tx,
        };
        self.request_tx
            .unbounded_send(request)
            .expect("BUG: I2C request failed");
        reply_rx.await.expect("BUG: failed to receive I2C reply")
    }

    pub async fn reset_i2c_controller(&self) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let request = Request::ResetI2cController { reply: reply_tx };
        self.request_tx
            .unbounded_send(request)
            .expect("BUG: I2C request failed");
        reply_rx.await.expect("BUG: failed to receive I2C reply")
    }
}

// Please somebody write tests here
