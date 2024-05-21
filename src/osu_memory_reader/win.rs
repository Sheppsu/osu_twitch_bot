use windows::Win32::System::Diagnostics::ToolHelp::{ CreateToolhelp32Snapshot, Process32First, Process32Next, TH32CS_SNAPPROCESS, PROCESSENTRY32 };
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::Foundation::{ CloseHandle, MAX_PATH, HANDLE, HMODULE };
use windows::Win32::System::Threading::{ OpenProcess, PROCESS_VM_READ, PROCESS_QUERY_INFORMATION };
use windows::Win32::System::ProcessStatus::{ EnumProcessModules, MODULEINFO, GetModuleFileNameExA, GetModuleInformation };
use windows::Win32::System::Memory::{ VirtualQueryEx, MEMORY_BASIC_INFORMATION };

use std::cmp::Ordering;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::slice;
use std::str;
use std::str::Utf8Error;

unsafe fn path_as_str(chars: &[u8; MAX_PATH as usize]) -> Result<&str, Utf8Error> {
    let mut len: usize = 0;
    for c in chars {
        if *c == 0 {
            break;
        }
        len += 1;
    }
    str::from_utf8(slice::from_raw_parts(chars.as_ptr(), len))
}

pub unsafe fn close_handle(handle: HANDLE) -> Result<(), String> {
    if let Err(e) = CloseHandle(handle) {
        return Err(format!("Failed to close snapshot handle: {}", e.message()));
    }
    Ok(())
}

pub unsafe fn open_process(pid: u32) -> Result<HANDLE, String> {
    let hproc = OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, None, pid);
    if let Err(e) = hproc {
        return Err(format!("Failed to open process from pid: {}", e.message()));
    }
    Ok(hproc.unwrap())
}

pub unsafe fn find_proc(proc_name: &str) -> Result<u32, String> {
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if let Err(e) = snapshot {
        return Err(format!("Received invalid snapshot handle: {}", e.message()));
    }
    let snapshot = snapshot.unwrap();

    let mut pe: PROCESSENTRY32 = Default::default();
    pe.dwSize = size_of::<PROCESSENTRY32>() as u32;
    let mut result = Process32First(snapshot, &mut pe);

    while let Ok(()) = result {
        let file_name = match path_as_str(&pe.szExeFile) {
            Ok(s) => s,
            Err(e) => return Err(format!("Failed to parse szExeFile: {}", e))
        };
        if let Ordering::Equal = proc_name.cmp(&file_name) {
            return match close_handle(snapshot) {
                Err(e) => Err(e),
                Ok(()) => Ok(pe.th32ProcessID)
            };
        }
        result = Process32Next(snapshot, &mut pe);
    }

    match close_handle(snapshot) {
        Err(e) => Err(e),
        Ok(()) => {
            match result {
                Ok(()) => Err(String::from("Failed to find process")),
                Err(e) => Err(format!("Failed to iterate processes: {}", e.message()))
            }
        }
    }
}

unsafe fn get_module_info(hproc: HANDLE, h_mod: HMODULE) -> Result<MODULEINFO, String> {
    let mut module_info: MODULEINFO = Default::default();
    if let Err(e) = GetModuleInformation(hproc, h_mod, &mut module_info, size_of::<MODULEINFO>() as u32) {
        return Err(format!("Failed to get module information: {}", e.message()));
    }
    Ok(module_info)
}

pub unsafe fn get_proc_info(hproc: HANDLE) -> Result<(String, MODULEINFO), String> {
    let mut lpcb_needed: u32 = 0;
    if let Err(e) = EnumProcessModules(hproc, null_mut(), 0, &mut lpcb_needed) {
        return Err(format!("Failed to get lpcb_needed: {}", e.message()));
    }

    let mut modules: Vec<HMODULE> = Vec::with_capacity((lpcb_needed as usize)/size_of::<HMODULE>());
    if let Err(e) = EnumProcessModules(hproc, modules.as_mut_ptr(), lpcb_needed, &mut lpcb_needed) {
        return Err(format!("Failed to get modules: {}", e.message()));
    }
    modules.set_len(modules.capacity());

    let mut file_name = [0 as u8; MAX_PATH as usize];
    let mut proc_file_name = [0 as u8; MAX_PATH as usize];
    GetModuleFileNameExA(hproc, None, &mut proc_file_name);
    for module in modules {
        GetModuleFileNameExA(hproc, module, &mut file_name);
        if let Ordering::Equal = proc_file_name.cmp(&file_name) {
            return Ok((String::from_utf8(Vec::from(file_name.as_slice())).unwrap(), get_module_info(hproc, module)?));
        }
    }

    Err(String::from("Unable to find module"))
}

pub unsafe fn query_page(hproc: HANDLE, addr: usize) -> Option<MEMORY_BASIC_INFORMATION> {
    let mut info = MEMORY_BASIC_INFORMATION::default();
    if VirtualQueryEx(hproc, Some(addr as *const _), &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
        return None;
    }
    Some(info)
}

pub unsafe fn read_address(hproc: HANDLE, addr: usize, buf: &mut [u8], size: usize) -> Result<(), String> {
    if let Err(e) = ReadProcessMemory(hproc, addr as *const c_void, buf.as_mut_ptr().cast(), size, None) {
        return Err(format!("Failed to read address {:X}: {}", addr, e.message()));
    }
    Ok(())
}