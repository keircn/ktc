use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static RUNNING: AtomicBool = AtomicBool::new(true);
static CHILDREN: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub fn request_shutdown() {
    RUNNING.store(false, Ordering::SeqCst);
}

pub fn register_child(pid: u32) {
    if let Ok(mut children) = CHILDREN.lock() {
        children.push(pid);
    }
}

fn terminate_children() {
    if let Ok(children) = CHILDREN.lock() {
        for &pid in children.iter() {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }
    
    std::thread::sleep(std::time::Duration::from_millis(100));
    
    if let Ok(children) = CHILDREN.lock() {
        for &pid in children.iter() {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
    }
}

pub struct Session {
    tty_fd: RawFd,
    old_kd_mode: i32,
    old_kb_mode: i32,
    vt_num: i32,
}

impl Session {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        setup_signal_handlers()?;
        
        let tty_fd = open_tty()?;
        let vt_num = get_vt_num(tty_fd)?;
        
        log::info!("Session starting on VT{}", vt_num);
        
        let old_kd_mode = get_kd_mode(tty_fd)?;
        let old_kb_mode = get_kb_mode(tty_fd)?;
        
        set_kd_mode(tty_fd, KD_GRAPHICS)?;
        set_kb_mode(tty_fd, K_OFF)?;
        
        log::info!("TTY configured: KD_GRAPHICS mode, keyboard raw mode");
        
        Ok(Session {
            tty_fd,
            old_kd_mode,
            old_kb_mode,
            vt_num,
        })
    }
    
    pub fn vt_num(&self) -> i32 {
        self.vt_num
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        log::info!("Session cleanup starting");
        
        terminate_children();
        
        if let Err(e) = set_kb_mode(self.tty_fd, self.old_kb_mode) {
            log::error!("Failed to restore keyboard mode: {}", e);
        }
        
        if let Err(e) = set_kd_mode(self.tty_fd, self.old_kd_mode) {
            log::error!("Failed to restore KD mode: {}", e);
        }
        
        unsafe {
            libc::ioctl(self.tty_fd, VT_ACTIVATE, self.vt_num);
            libc::ioctl(self.tty_fd, VT_WAITACTIVE, self.vt_num);
            
            libc::close(self.tty_fd);
        }
        
        log::info!("Session cleanup complete, TTY restored");
    }
}

fn setup_signal_handlers() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as usize;
        sa.sa_flags = libc::SA_RESTART;
        
        libc::sigemptyset(&mut sa.sa_mask);
        
        if libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut()) < 0 {
            return Err("Failed to set SIGINT handler".into());
        }
        if libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut()) < 0 {
            return Err("Failed to set SIGTERM handler".into());
        }
        if libc::sigaction(libc::SIGHUP, &sa, std::ptr::null_mut()) < 0 {
            return Err("Failed to set SIGHUP handler".into());
        }
    }
    
    Ok(())
}

extern "C" fn signal_handler(sig: i32) {
    log::info!("Received signal {}, initiating shutdown", sig);
    request_shutdown();
}

fn open_tty() -> Result<RawFd, Box<dyn std::error::Error>> {
    let tty_path = std::fs::read_to_string("/sys/class/tty/tty0/active")
        .ok()
        .and_then(|s| {
            let name = s.trim();
            if !name.is_empty() {
                Some(format!("/dev/{}", name))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "/dev/tty".to_string());
    
    log::info!("Opening TTY: {}", tty_path);
    
    let fd = unsafe {
        libc::open(
            std::ffi::CString::new(tty_path.clone())?.as_ptr(),
            libc::O_RDWR | libc::O_CLOEXEC,
        )
    };
    
    if fd < 0 {
        return Err(format!("Failed to open TTY {}: {}", tty_path, std::io::Error::last_os_error()).into());
    }
    
    Ok(fd)
}

fn get_vt_num(fd: RawFd) -> Result<i32, Box<dyn std::error::Error>> {
    #[repr(C)]
    struct VtStat {
        v_active: u16,
        v_signal: u16,
        v_state: u16,
    }
    
    let mut stat: VtStat = unsafe { std::mem::zeroed() };
    
    if unsafe { libc::ioctl(fd, VT_GETSTATE, &mut stat) } < 0 {
        return Err("Failed to get VT state".into());
    }
    
    Ok(stat.v_active as i32)
}

fn get_kd_mode(fd: RawFd) -> Result<i32, Box<dyn std::error::Error>> {
    let mut mode: i32 = 0;
    
    if unsafe { libc::ioctl(fd, KDGETMODE, &mut mode) } < 0 {
        return Err("Failed to get KD mode".into());
    }
    
    Ok(mode)
}

fn set_kd_mode(fd: RawFd, mode: i32) -> Result<(), Box<dyn std::error::Error>> {
    if unsafe { libc::ioctl(fd, KDSETMODE, mode) } < 0 {
        return Err(format!("Failed to set KD mode to {}: {}", mode, std::io::Error::last_os_error()).into());
    }
    
    Ok(())
}

fn get_kb_mode(fd: RawFd) -> Result<i32, Box<dyn std::error::Error>> {
    let mut mode: i32 = 0;
    
    if unsafe { libc::ioctl(fd, KDGKBMODE, &mut mode) } < 0 {
        return Err("Failed to get keyboard mode".into());
    }
    
    Ok(mode)
}

fn set_kb_mode(fd: RawFd, mode: i32) -> Result<(), Box<dyn std::error::Error>> {
    if unsafe { libc::ioctl(fd, KDSKBMODE, mode) } < 0 {
        return Err(format!("Failed to set keyboard mode to {}: {}", mode, std::io::Error::last_os_error()).into());
    }
    
    Ok(())
}

const KDSETMODE: libc::c_ulong = 0x4B3A;
const KDGETMODE: libc::c_ulong = 0x4B3B;
const KDGKBMODE: libc::c_ulong = 0x4B44;
const KDSKBMODE: libc::c_ulong = 0x4B45;

const KD_TEXT: i32 = 0x00;
const KD_GRAPHICS: i32 = 0x01;

const K_RAW: i32 = 0x00;
const K_XLATE: i32 = 0x01;
const K_MEDIUMRAW: i32 = 0x02;
const K_UNICODE: i32 = 0x03;
const K_OFF: i32 = 0x04;

const VT_GETSTATE: libc::c_ulong = 0x5603;
const VT_ACTIVATE: libc::c_ulong = 0x5606;
const VT_WAITACTIVE: libc::c_ulong = 0x5607;
