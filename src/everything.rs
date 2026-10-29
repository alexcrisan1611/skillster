//! Asking a running Everything (voidtools) for every folder named `skills`, through its
//! window-message IPC (the protocol `es.exe` uses), so nothing else needs installing.
//! Windows only.

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, HWND_MESSAGE, MSG, PM_REMOVE,
    PeekMessageW, RegisterClassW, SendMessageW, WM_COPYDATA, WNDCLASSW,
};

/// IPC windows of Everything 1.5 (which runs as instance "1.5a") and 1.4.
const IPC_WINDOWS: &[&str] = &["EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)", "EVERYTHING_TASKBAR_NOTIFICATION"];
const QUERY: &str = "folder: wfn:skills";

// From Everything's `everything_ipc.h`.
const COPYDATA_QUERY2W: usize = 18;
const REQUEST_FULL_PATH_AND_NAME: u32 = 0x4;
const SORT_NAME_ASCENDING: u32 = 1;

/// Our id for the reply, echoed back by Everything in `COPYDATASTRUCT::dwData`.
const REPLY_ID: u32 = 0x534B_5359;
const TIMEOUT: Duration = Duration::from_secs(30);

/// `EVERYTHING_IPC_QUERY2`, followed in the message by the NUL-terminated UTF-16 search.
#[repr(C)]
struct Query2 {
    reply_hwnd: u32,
    reply_copydata_message: u32,
    search_flags: u32,
    offset: u32,
    max_results: u32,
    request_flags: u32,
    sort_type: u32,
}

thread_local! {
    static REPLY: RefCell<Option<Vec<PathBuf>>> = const { RefCell::new(None) };
}

/// Every folder named exactly `skills` in Everything's index.
pub fn skills_folders() -> Result<Vec<PathBuf>, String> {
    let everything = IPC_WINDOWS
        .iter()
        .map(|class| unsafe { FindWindowW(wide(class).as_ptr(), std::ptr::null()) })
        .find(|h| !h.is_null())
        .ok_or("Everything is not running.")?;

    let reply_window = ReplyWindow::new()?;
    let query = Query2 {
        reply_hwnd: reply_window.0 as usize as u32,
        reply_copydata_message: REPLY_ID,
        search_flags: 0,
        offset: 0,
        max_results: u32::MAX,
        request_flags: REQUEST_FULL_PATH_AND_NAME,
        sort_type: SORT_NAME_ASCENDING,
    };
    let mut message: Vec<u8> = unsafe {
        std::slice::from_raw_parts((&raw const query).cast::<u8>(), size_of::<Query2>()).to_vec()
    };
    message.extend(wide(QUERY).iter().flat_map(|u| u.to_le_bytes()));
    let data = COPYDATASTRUCT { dwData: COPYDATA_QUERY2W, cbData: message.len() as u32, lpData: message.as_mut_ptr().cast() };

    REPLY.with(|r| r.borrow_mut().take());
    let accepted = unsafe { SendMessageW(everything, WM_COPYDATA, reply_window.0 as WPARAM, (&raw const data) as LPARAM) };
    if accepted == 0 {
        return Err("Everything did not accept the search.".into());
    }

    // The answer arrives as a WM_COPYDATA sent to our window, delivered while we pump messages.
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe { DispatchMessageW(&msg) };
        }
        if let Some(found) = REPLY.with(|r| r.borrow_mut().take()) {
            return Ok(found);
        }
        if Instant::now() > deadline {
            return Err("Everything did not answer in time.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A message-only window that receives Everything's reply; destroyed on drop.
struct ReplyWindow(HWND);

impl ReplyWindow {
    fn new() -> Result<Self, String> {
        let class = wide("skills-sync-everything-reply");
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(on_message),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        // Fails harmlessly if the class is already registered.
        unsafe { RegisterClassW(&wc) };
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() { Err("cannot create a window for Everything's reply.".into()) } else { Ok(Self(hwnd)) }
    }
}

impl Drop for ReplyWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

unsafe extern "system" fn on_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_COPYDATA {
        let data = unsafe { &*(lparam as *const COPYDATASTRUCT) };
        if data.dwData == REPLY_ID as usize {
            // The data is only valid during this call, so parse it now.
            let bytes = unsafe { std::slice::from_raw_parts(data.lpData.cast::<u8>(), data.cbData as usize) };
            let found = parse_list2(bytes);
            REPLY.with(|r| *r.borrow_mut() = Some(found));
            return 1;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Reads an `EVERYTHING_IPC_LIST2` holding only full paths: a 20-byte header
/// (`totitems, numitems, offset, request_flags, sort_type`), then `numitems` items of
/// (`flags, data_offset`); each item's data is a character count and that many UTF-16
/// units. Anything out of bounds is skipped.
fn parse_list2(bytes: &[u8]) -> Vec<PathBuf> {
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let Some(count) = u32_at(4) else { return Vec::new() };
    (0..count as usize)
        .filter_map(|i| {
            let data = u32_at(20 + i * 8 + 4)? as usize;
            let len = u32_at(data)? as usize;
            let text = bytes.get(data + 4..data + 4 + len * 2)?;
            let units: Vec<u16> = text.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
            Some(PathBuf::from(String::from_utf16_lossy(&units)))
        })
        .collect()
}

/// NUL-terminated UTF-16.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reply() {
        let paths = ["C:\\a\\skills", "D:\\ș\\skills"];
        let mut bytes: Vec<u8> = [0, paths.len() as u32, 0, REQUEST_FULL_PATH_AND_NAME, 1]
            .iter()
            .flat_map(|n: &u32| n.to_le_bytes())
            .collect();
        let mut data_at = 20 + paths.len() * 8;
        let mut data = Vec::new();
        for p in paths {
            bytes.extend(1u32.to_le_bytes()); // EVERYTHING_IPC_FOLDER
            bytes.extend((data_at as u32).to_le_bytes());
            let units: Vec<u16> = p.encode_utf16().chain(Some(0)).collect();
            data.extend(((units.len() - 1) as u32).to_le_bytes());
            data.extend(units.iter().flat_map(|u| u.to_le_bytes()));
            data_at += 4 + units.len() * 2;
        }
        bytes.extend(data);
        assert_eq!(parse_list2(&bytes), paths.map(PathBuf::from));
        assert!(parse_list2(&bytes[..30]).is_empty(), "truncated replies are skipped, not misread");
    }
}
