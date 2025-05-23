use crate::traits::*;
use std::collections::HashMap;
use std::env;
use std::fs::read_dir;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;
use wmi::WMIResult;
use wmi::{COMLibrary, Variant, WMIConnection};

use windows::{
    core::PSTR, Win32::System::Power::GetSystemPowerStatus,
    Win32::System::Power::SYSTEM_POWER_STATUS,
    Win32::System::SystemInformation::GetComputerNameExA,
    Win32::System::SystemInformation::GetTickCount64,
    Win32::System::SystemInformation::GlobalMemoryStatusEx,
    Win32::System::SystemInformation::MEMORYSTATUSEX,
    Win32::System::WindowsProgramming::GetUserNameA,
};

impl From<wmi::WMIError> for ReadoutError {
    fn from(e: wmi::WMIError) -> Self {
        ReadoutError::Other(e.to_string())
    }
}

pub struct WindowsBatteryReadout;

impl BatteryReadout for WindowsBatteryReadout {
    fn new() -> Self {
        WindowsBatteryReadout {}
    }

    fn percentage(&self) -> Result<u8, ReadoutError> {
        let power_state = WindowsBatteryReadout::get_power_status()?;

        match power_state.BatteryLifePercent {
            s if s != 255 => Ok(s),
            s => Err(ReadoutError::Warning(format!(
                "Windows reported a battery percentage of {s}, which means there is \
                no battery available. Are you on a desktop system?"
            ))),
        }
    }

    fn status(&self) -> Result<BatteryState, ReadoutError> {
        let power_state = WindowsBatteryReadout::get_power_status()?;

        match power_state.ACLineStatus {
            0 => Ok(BatteryState::Discharging),
            1 => Ok(BatteryState::Charging),
            a => Err(ReadoutError::Other(format!(
                "Unexpected value for ac_line_status from win32 api: {a}"
            ))),
        }
    }

    fn health(&self) -> Result<u8, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }
}

impl WindowsBatteryReadout {
    fn get_power_status() -> Result<SYSTEM_POWER_STATUS, ReadoutError> {
        let mut power_state = SYSTEM_POWER_STATUS::default();

        if unsafe { GetSystemPowerStatus(&mut power_state) }.as_bool() {
            return Ok(power_state);
        }

        Err(ReadoutError::Other(String::from(
            "Call to GetSystemPowerStatus failed.",
        )))
    }
}

pub struct WindowsKernelReadout;

impl KernelReadout for WindowsKernelReadout {
    fn new() -> Self {
        WindowsKernelReadout {}
    }

    fn os_release(&self) -> Result<String, ReadoutError> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let current_windows_not =
            hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")?;

        let nt_build: String = current_windows_not.get_value("CurrentBuild")?;

        Ok(nt_build)
    }

    fn os_type(&self) -> Result<String, ReadoutError> {
        Ok(String::from("Windows NT"))
    }

    fn pretty_kernel(&self) -> Result<String, ReadoutError> {
        Ok(format!("{} {}", self.os_type()?, self.os_release()?))
    }
}

pub struct WindowsMemoryReadout;

impl MemoryReadout for WindowsMemoryReadout {
    fn new() -> Self {
        WindowsMemoryReadout {}
    }

    fn total(&self) -> Result<u64, ReadoutError> {
        let memory_status = WindowsMemoryReadout::get_memory_status()?;
        Ok(memory_status.ullTotalPhys / 1024u64)
    }

    fn free(&self) -> Result<u64, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn buffers(&self) -> Result<u64, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn cached(&self) -> Result<u64, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn reclaimable(&self) -> Result<u64, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn used(&self) -> Result<u64, ReadoutError> {
        let memory_status = WindowsMemoryReadout::get_memory_status()?;
        Ok((memory_status.ullTotalPhys - memory_status.ullAvailPhys) / 1024u64)
    }

    fn swap_total(&self) -> Result<u64, ReadoutError> {
        return Err(ReadoutError::NotImplemented);
    }

    fn swap_free(&self) -> Result<u64, ReadoutError> {
        return Err(ReadoutError::NotImplemented);
    }

    fn swap_used(&self) -> Result<u64, ReadoutError> {
        return Err(ReadoutError::NotImplemented);
    }
}

impl WindowsMemoryReadout {
    fn get_memory_status() -> Result<MEMORYSTATUSEX, ReadoutError> {
        let mut memory_status = MEMORYSTATUSEX::default();
        memory_status.dwLength = std::mem::size_of_val(&memory_status) as u32;

        if !unsafe { GlobalMemoryStatusEx(&mut memory_status) }.as_bool() {
            return Err(ReadoutError::Other(String::from(
                "GlobalMemoryStatusEx returned a zero \
            return \
            code.",
            )));
        }

        Ok(memory_status)
    }
}

thread_local! {
    static COM_LIB: COMLibrary = COMLibrary::new().unwrap();
}

fn wmi_connection() -> WMIResult<WMIConnection> {
    let com_lib = COM_LIB.with(|com| *com);
    WMIConnection::new(com_lib)
}

pub struct WindowsGeneralReadout;

impl GeneralReadout for WindowsGeneralReadout {
    fn new() -> Self {
        WindowsGeneralReadout
    }

    fn backlight(&self) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn resolution(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn username(&self) -> Result<String, ReadoutError> {
        let mut size = 0;
        unsafe { GetUserNameA(PSTR(std::ptr::null_mut()), &mut size) };

        if size == 0 {
            return Err(ReadoutError::Other(
                "Call to \"GetUserNameA\" failed.".to_string(),
            ));
        }

        let mut username = Vec::with_capacity(size as usize);
        if !unsafe { GetUserNameA(PSTR(username.as_mut_ptr()), &mut size) }.as_bool() {
            return Err(ReadoutError::Other(
                "Call to \"GetUserNameA\" failed.".to_string(),
            ));
        }

        unsafe {
            username.set_len(size as usize);
        }

        let mut str = match String::from_utf8(username) {
            Ok(str) => str,
            Err(e) => {
                return Err(ReadoutError::Other(format!(
                    "String from \"GetUserNameA\" \
            was not valid UTF-8: {e}"
                )))
            }
        };

        str.pop(); //remove null terminator from string.

        Ok(str)
    }

    fn hostname(&self) -> Result<String, ReadoutError> {
        use windows::Win32::System::SystemInformation::ComputerNameDnsHostname;

        let mut size = 0;
        unsafe {
            GetComputerNameExA(
                ComputerNameDnsHostname,
                PSTR(std::ptr::null_mut()),
                &mut size,
            )
        };

        if size == 0 {
            return Err(ReadoutError::Other(String::from(
                "Call to \"GetComputerNameExA\" failed.",
            )));
        }

        let mut hostname = Vec::with_capacity(size as usize);
        if unsafe {
            GetComputerNameExA(
                ComputerNameDnsHostname,
                PSTR(hostname.as_mut_ptr()),
                &mut size,
            )
        } == false
        {
            return Err(ReadoutError::Other(String::from(
                "Call to \"GetComputerNameExA\" failed.",
            )));
        }

        unsafe { hostname.set_len(size as usize) };

        let str = match String::from_utf8(hostname) {
            Ok(str) => str,
            Err(e) => {
                return Err(ReadoutError::Other(format!(
                    "String from \"GetComputerNameExA\" \
            was not valid UTF-8: {e}"
                )))
            }
        };

        Ok(str)
    }

    fn distribution(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn desktop_environment(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn session(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn window_manager(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn terminal(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn shell(&self, _shorthand: ShellFormat, _: ShellKind) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn cpu_model_name(&self) -> Result<String, ReadoutError> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let central_processor =
            hklm.open_subkey("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0")?;

        let processor_name: String = central_processor.get_value("ProcessorNameString")?;

        Ok(processor_name)
    }

    fn cpu_usage(&self) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn cpu_physical_cores(&self) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn cpu_cores(&self) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn uptime(&self) -> Result<usize, ReadoutError> {
        let tick_count = unsafe { GetTickCount64() };
        let duration = std::time::Duration::from_millis(tick_count);

        Ok(duration.as_secs() as usize)
    }

    fn machine(&self) -> Result<String, ReadoutError> {
        let product_readout = WindowsProductReadout::new();

        Ok(format!(
            "{} {}",
            product_readout.vendor()?,
            product_readout.product()?
        ))
    }

    fn os_name(&self) -> Result<String, ReadoutError> {
        let wmi_con = wmi_connection()?;

        let results: Vec<HashMap<String, Variant>> =
            wmi_con.raw_query("SELECT Caption FROM Win32_OperatingSystem")?;

        if let Some(os) = results.first() {
            if let Some(Variant::String(caption)) = os.get("Caption") {
                return Ok(caption.to_string());
            }
        }

        Err(ReadoutError::Other(
            "Trying to get the operating system name \
            from WMI failed"
                .to_string(),
        ))
    }

    fn disk_space(&self, path: &Path) -> Result<(u64, u64), ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn gpus(&self) -> Result<Vec<String>, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }
}

pub struct WindowsProductReadout {
    manufacturer: Option<String>,
    model: Option<String>,
}

impl ProductReadout for WindowsProductReadout {
    fn new() -> Self {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let sys_info = hklm
            .open_subkey("SYSTEM\\CurrentControlSet\\Control\\SystemInformation")
            .unwrap();

        WindowsProductReadout {
            manufacturer: sys_info.get_value("SystemManufacturer").ok(),
            model: sys_info.get_value("SystemProductName").ok(),
        }
    }

    fn vendor(&self) -> Result<String, ReadoutError> {
        match &self.manufacturer {
            Some(v) => Ok(v.clone()),
            None => Err(ReadoutError::Other(
                "Trying to get the system manufacturer \
                from the registry failed"
                    .to_string(),
            )),
        }
    }

    fn family(&self) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn product(&self) -> Result<String, ReadoutError> {
        match &self.model {
            Some(v) => Ok(v.clone()),
            None => Err(ReadoutError::Other(
                "Trying to get the system product name \
                from the registry failed"
                    .to_string(),
            )),
        }
    }
}

pub struct WindowsPackageReadout;

impl PackageReadout for WindowsPackageReadout {
    fn new() -> Self {
        WindowsPackageReadout {}
    }

    /// Returns the __number of installed packages__ for the following package managers:
    /// - cargo
    fn count_pkgs(&self) -> Vec<(PackageManager, usize)> {
        let mut packages = Vec::new();
        if let Some(c) = WindowsPackageReadout::count_cargo() {
            packages.push((PackageManager::Cargo, c));
        }
        if let Some(c) = WindowsPackageReadout::count_scoop() {
            packages.push((PackageManager::Scoop, c));
        }
        if let Some(c) = WindowsPackageReadout::count_winget() {
            packages.push((PackageManager::Winget, c));
        }
        if let Some(c) = WindowsPackageReadout::count_chocolatey() {
            packages.push((PackageManager::Chocolatey, c));
        }
        packages
    }
}

impl WindowsPackageReadout {
    fn count_cargo() -> Option<usize> {
        crate::shared::count_cargo()
    }

    fn count_scoop() -> Option<usize> {
        let scoop = match std::env::var("SCOOP") {
            Ok(scoop_var) => PathBuf::from(scoop_var),
            _ => home::home_dir().unwrap().join("scoop"),
        };
        match scoop.join("apps").read_dir() {
            Ok(dir) => Some(dir.count() - 1), // One entry belongs to scoop itself
            _ => None,
        }
    }

    fn count_winget() -> Option<usize> {
        if let Ok(username) = env::var("USERNAME") {
            let db = format!("C:\\Users\\{username}\\AppData\\Local\\Packages\\Microsoft.DesktopAppInstaller_8wekyb3d8bbwe\\LocalState\\Microsoft.Winget.Source_8wekyb3d8bbwe\\installed.db");
            if !Path::new(&db).is_file() {
                return None;
            }
            let connection = sqlite::open(db);
            if let Ok(con) = connection {
                let statement = con.prepare("SELECT COUNT(*) FROM ids");
                if let Ok(mut s) = statement {
                    if s.next().is_ok() {
                        return match s.read::<Option<i64>, _>(0) {
                            Ok(Some(count)) => Some(count as usize),
                            _ => None,
                        };
                    }
                }
            }
        }
        None
    }

    fn count_chocolatey() -> Option<usize> {
        let chocolatey_dir = Path::new("C:\\ProgramData\\chocolatey\\lib");
        if chocolatey_dir.is_dir() {
            if let Ok(read_dir) = read_dir(chocolatey_dir) {
                return Some(read_dir.count());
            }
        }
        None
    }
}

pub struct WindowsNetworkReadout;

impl NetworkReadout for WindowsNetworkReadout {
    fn new() -> Self {
        WindowsNetworkReadout
    }

    fn tx_bytes(&self, _: Option<&str>) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn tx_packets(&self, _: Option<&str>) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn rx_bytes(&self, _: Option<&str>) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn rx_packets(&self, _: Option<&str>) -> Result<usize, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }

    fn logical_address(&self, interface: Option<&str>) -> Result<String, ReadoutError> {
        match interface {
            Some(interface) => {
                if let Ok(addresses) = local_ip_address::list_afinet_netifas() {
                    if let Some((_, ip)) = addresses.iter().find(|(name, _)| name == interface) {
                        return Ok(ip.to_string());
                    }
                }
            }
            None => {
                if let Ok(local_ip) = local_ip_address::local_ip() {
                    return Ok(local_ip.to_string());
                }
            }
        };

        Err(ReadoutError::Other(
            "Unable to get local IP address.".to_string(),
        ))
    }

    fn physical_address(&self, _: Option<&str>) -> Result<String, ReadoutError> {
        Err(ReadoutError::NotImplemented)
    }
}
