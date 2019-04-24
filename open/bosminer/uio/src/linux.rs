use fs2::FileExt;
use libc;
use nix::sys::mman::{MapFlags, ProtFlags};
use std::error::Error;
use std::fmt;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::prelude::*;
use std::num::ParseIntError;
use std::os::unix::prelude::AsRawFd;
use std::time::{Duration, Instant};
use timeout_readwrite::TimeoutReader;

const PAGESIZE: usize = 4096;

#[derive(Debug)]
pub enum UioError {
    Io(io::Error),
    Map(nix::Error),
    Parse,
}

impl From<io::Error> for UioError {
    fn from(e: io::Error) -> Self {
        UioError::Io(e)
    }
}

impl From<ParseIntError> for UioError {
    fn from(_: ParseIntError) -> Self {
        UioError::Parse
    }
}

impl From<nix::Error> for UioError {
    fn from(e: nix::Error) -> Self {
        UioError::Map(e)
    }
}

impl fmt::Display for UioError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            UioError::Parse => write!(f, "integer conversion error"),
            UioError::Io(ref e) => write!(f, "{}", e),
            UioError::Map(ref e) => write!(f, "{}", e),
        }
    }
}

impl Error for UioError {
    fn description(&self) -> &str {
        match self {
            UioError::Io(ref e) => e.description(),
            UioError::Map(ref e) => e.description(),
            UioError::Parse => "integer conversion error",
        }
    }

    fn cause(&self) -> Option<&Error> {
        match self {
            UioError::Io(ref e) => Some(e),
            UioError::Map(ref e) => Some(e),
            UioError::Parse => None,
        }
    }
}

/// This structure represents memory mapping as performed by `mmap()` syscall.
/// Lifetime of this structure is directly tied to the mapping and once the
/// structure goes out of the scope, the mapping is `unmmap()`-ed.
///
/// The inner pointer `ptr` is public, but the responsibility to not use it
/// after UioMapping structure had got out of the scope is on the caller.
pub struct UioMapping {
    pub ptr: *mut libc::c_void,
    length: usize,
}

impl Drop for UioMapping {
    fn drop(&mut self) {
        unsafe { nix::sys::mman::munmap(self.ptr, self.length) }.expect("munmap is successful");
    }
}

pub struct UioDevice {
    uio_num: usize,
    //path: &'static str,
    devfile: File,
}

impl UioDevice {
    /// Creates a new UIO device for Linux.
    ///
    /// # Arguments
    ///  * uio_num - UIO index of device (i.e., 1 for /dev/uio1)
    pub fn new(uio_num: usize) -> io::Result<UioDevice> {
        let path = format!("/dev/uio{}", uio_num);
        let devfile = OpenOptions::new().read(true).write(true).open(path)?;
        devfile.lock_exclusive()?;
        Ok(UioDevice { uio_num, devfile })
    }

    /// Go through all UIO devices in /sys and try to find one with
    /// matching name.
    ///
    /// # Arguments
    ///  * uio_name - name of the uio device (must match the one in sysfs)
    pub fn open_by_name(uio_name: &String) -> io::Result<UioDevice> {
        let mut i = 0;
        loop {
            let path = format!("/sys/class/uio/uio{}/name", i);
            let name = fs::read_to_string(path)?;
            if name.trim() == uio_name {
                return Ok(UioDevice::new(i)?);
            }
            i = i + 1;
        }
    }

    /// Return a vector of mappable resources (i.e., PCI bars) including their size.
    pub fn get_resource_info(&mut self) -> Result<Vec<(String, u64)>, UioError> {
        let paths = fs::read_dir(format!("/sys/class/uio/uio{}/device/", self.uio_num))?;

        let mut bars = Vec::new();
        for p in paths {
            let path = p?;
            let file_name = path
                .file_name()
                .into_string()
                .expect("Is valid UTF-8 string.");

            if file_name.starts_with("resource") && file_name.len() > "resource".len() {
                let metadata = fs::metadata(path.path())?;
                bars.push((file_name, metadata.len()));
            }
        }

        Ok(bars)
    }

    /// Maps a given resource into the virtual address space of the process.
    ///
    /// Returns UioMapping structure, which represents the mapping. Lifetime
    /// of the structure is directly tied to the mapping.
    ///
    /// # Arguments
    ///   * bar_nr: The index to the given resource (i.e., 1 for /sys/class/uio/uioX/device/resource1)
    pub fn map_resource(&self, bar_nr: usize) -> Result<UioMapping, UioError> {
        let filename = format!(
            "/sys/class/uio/uio{}/device/resource{}",
            self.uio_num, bar_nr
        );
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filename.to_string())?;
        let metadata = fs::metadata(filename.clone())?;
        let fd = f.as_raw_fd();
        let length = metadata.len() as usize;

        let res = unsafe {
            nix::sys::mman::mmap(
                0 as *mut libc::c_void,
                length,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                fd,
                0 as libc::off_t,
            )
        };
        match res {
            Ok(m) => Ok(UioMapping { ptr: m, length }),
            Err(e) => Err(UioError::from(e)),
        }
    }

    fn read_file(&self, path: String) -> Result<String, UioError> {
        let mut file = File::open(path)?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)?;
        Ok(buffer.trim().to_string())
    }

    /// The amount of events.
    pub fn get_event_count(&self) -> Result<u32, UioError> {
        let filename = format!("/sys/class/uio/uio{}/event", self.uio_num);
        let buffer = self.read_file(filename)?;
        match u32::from_str_radix(&buffer, 10) {
            Ok(v) => Ok(v),
            Err(e) => Err(UioError::from(e)),
        }
    }

    /// The name of the UIO device.
    pub fn get_name(&self) -> Result<String, UioError> {
        let filename = format!("/sys/class/uio/uio{}/name", self.uio_num);
        self.read_file(filename)
    }

    /// The version of the UIO driver.
    pub fn get_version(&self) -> Result<String, UioError> {
        let filename = format!("/sys/class/uio/uio{}/version", self.uio_num);
        self.read_file(filename)
    }

    /// The size of a given mapping.
    ///
    /// # Arguments
    ///  * mapping: The given index of the mapping (i.e., 1 for /sys/class/uio/uioX/maps/map1)
    pub fn map_size(&self, mapping: usize) -> Result<usize, UioError> {
        let filename = format!(
            "/sys/class/uio/uio{}/maps/map{}/size",
            self.uio_num, mapping
        );
        let buffer = self.read_file(filename)?;
        match usize::from_str_radix(&buffer[2..], 16) {
            Ok(v) => Ok(v),
            Err(e) => Err(UioError::from(e)),
        }
    }

    /// The address of a given mapping.
    ///
    /// # Arguments
    ///  * mapping: The given index of the mapping (i.e., 1 for /sys/class/uio/uioX/maps/map1)
    pub fn map_addr(&self, mapping: usize) -> Result<usize, UioError> {
        let filename = format!(
            "/sys/class/uio/uio{}/maps/map{}/addr",
            self.uio_num, mapping
        );
        let buffer = self.read_file(filename)?;
        match usize::from_str_radix(&buffer[2..], 16) {
            Ok(v) => Ok(v),
            Err(e) => Err(UioError::from(e)),
        }
    }

    /// Return a list of all possible memory mappings.
    pub fn get_map_info(&mut self) -> Result<Vec<String>, UioError> {
        let paths = fs::read_dir(format!("/sys/class/uio/uio{}/maps/", self.uio_num))?;

        let mut map = Vec::new();
        for p in paths {
            let path = p?;
            let file_name = path
                .file_name()
                .into_string()
                .expect("Is valid UTF-8 string.");

            if file_name.starts_with("map") && file_name.len() > "map".len() {
                map.push(file_name);
            }
        }

        Ok(map)
    }

    /// Map an available memory mapping.
    ///
    /// Returns UioMapping structure, which represents the mapping. Lifetime
    /// of the structure is directly tied to the mapping.
    ///
    /// # Arguments
    ///  * mapping: The given index of the mapping (i.e., 1 for /sys/class/uio/uioX/maps/map1)
    pub fn map_mapping(&self, mapping: usize) -> Result<UioMapping, UioError> {
        let offset = mapping * PAGESIZE;
        let fd = self.devfile.as_raw_fd();
        let map_size = self.map_size(mapping).unwrap() as usize; // TODO

        let res = unsafe {
            nix::sys::mman::mmap(
                0 as *mut libc::c_void,
                map_size,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                fd,
                offset as libc::off_t,
            )
        };
        match res {
            Ok(m) => Ok(UioMapping {
                ptr: m,
                length: map_size,
            }),
            Err(e) => Err(UioError::from(e)),
        }
    }

    /// Enable interrupt
    pub fn irq_enable(&self) -> io::Result<()> {
        let bytes = 1u32.to_ne_bytes();
        self.devfile.try_clone()?.write(&bytes)?;
        Ok(())
    }

    /// Disable interrupt
    pub fn irq_disable(&self) -> io::Result<()> {
        let bytes = 0u32.to_ne_bytes();
        self.devfile.try_clone()?.write(&bytes)?;
        Ok(())
    }

    /// Wait for interrupt
    pub fn irq_wait(&self) -> io::Result<u32> {
        let mut bytes = [0u8; 4];
        self.devfile.try_clone()?.read(&mut bytes)?;
        Ok(u32::from_ne_bytes(bytes))
    }

    pub fn irq_wait_timeout(&self, timeout: Duration) -> io::Result<Option<u32>> {
        let mut rdr = TimeoutReader::new(self.devfile.try_clone()?, timeout);
        let mut bytes = [0u8; 4];
        let res = rdr.read_exact(&mut bytes);

        // Handle timeout, because it's not an error condition
        if let Err(e) = res {
            if e.kind() == io::ErrorKind::TimedOut {
                Ok(None)
            } else {
                Err(e)
            }
        } else {
            Ok(Some(u32::from_ne_bytes(bytes)))
        }
    }

    pub fn irq_wait_cond<T>(&self, cond: T, timeout: Option<Duration>) -> io::Result<Option<()>>
    where
        T: Fn() -> bool,
    {
        let start = Instant::now();

        while !cond() {
            self.irq_enable()?;
            if cond() {
                // this check is to cover the window between `cond()`
                // in while head and `irq_enable()` that follows (it is
                // relevant only for edge-sensitive interrupts though)
                break;
            }
            if let Some(timeout) = timeout {
                let passed = start.elapsed();
                if passed >= timeout {
                    return Ok(None);
                }
                self.irq_wait_timeout(timeout - passed)?;
            } else {
                self.irq_wait()?;
            }
        }
        Ok(Some(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open() {
        let res = ::linux::UioDevice::new(0);
        match res {
            Err(e) => {
                panic!("Can not open device /dev/uio0: {}", e);
            }
            Ok(_f) => (),
        }
    }

    #[test]
    fn print_info() {
        let res = ::linux::UioDevice::new(0).unwrap();
        let name = res.get_name().expect("Can't get name");
        let version = res.get_version().expect("Can't get version");
        let event_count = res.get_event_count().expect("Can't get event count");
        assert_eq!(name, "uio_pci_generic");
        assert_eq!(version, "0.01.0");
        assert_eq!(event_count, 0);
    }

    #[test]
    fn map() {
        let res = ::linux::UioDevice::new(0).unwrap();
        let bars = res.map_resource(5);
        match bars {
            Err(e) => {
                panic!("Can not map PCI stuff: {:?}", e);
            }
            Ok(_f) => (),
        }
    }

    #[test]
    fn bar_info() {
        let mut res = ::linux::UioDevice::new(0).unwrap();
        let bars = res.get_resource_info();
        match bars {
            Err(e) => {
                panic!("Can not map PCI stuff: {:?}", e);
            }
            Ok(_f) => (),
        }
    }
}