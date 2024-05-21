#[cfg(target_os = "windows")]
use crate::osu_memory_reader::win::read_address;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;

use core::convert::From;
use paste::paste;

macro_rules! read_bytes {
    ($hproc:expr, $addr:expr, $b:literal) => {
        {
            let mut buf: [u8; $b] = [0; $b];
            read_address($hproc, $addr, &mut buf, $b)?;
            buf
        }
    };
    ($hproc:expr, $addr:expr, $buf:expr) => {
        {
            let len = $buf.len();
            read_address($hproc, $addr, &mut $buf, len)?;
        }
    }
}

macro_rules! primitive_read_fn {
    ($t:ident) => {
        paste! {
            unsafe fn [<read_ $t>](&self, addr: usize) -> Result<$t, String> {
                if addr == 0 {
                    return Ok($t::default());
                }
    
                const SIZE: usize = core::mem::size_of::<$t>();
                let mut buf: [u8; SIZE] = [0; SIZE];
                read_address(self.handle(), addr, &mut buf, SIZE)?;
                Ok($t::from_le_bytes(buf))
            }
        }
    };
}

macro_rules! impl_primitive_from {
    ($t:ty) => {
        impl FromBytes for $t {
            fn from_bytes(bytes: &[u8]) -> Self {
                const SIZE: usize = core::mem::size_of::<$t>();
                let mut buf: [u8; SIZE] = [0; SIZE];
                for i in 0..SIZE {
                    buf[i] = bytes[i];
                }
                <$t>::from_le_bytes(buf)
            }
        }
    };
}

pub trait FromBytes {
    fn from_bytes(bytes: &[u8]) -> Self;
}

impl_primitive_from!(u16);
impl_primitive_from!(i32);

pub trait MemoryReader {
    fn handle(&self) -> HANDLE;

    unsafe fn read_raw(&self, addr: usize, len: usize) -> Result<Vec<u8>, String> {
        let mut buf: Vec<u8> = vec![0; len];
        read_bytes!(self.handle(), addr, buf);
        Ok(buf)
    }

    primitive_read_fn!(u32);
    primitive_read_fn!(i8);
    primitive_read_fn!(i16);
    primitive_read_fn!(i32);
    primitive_read_fn!(f32);
    primitive_read_fn!(f64);

    unsafe fn read_ptr(&self, addr: usize) -> Result<usize, String> {
        let ptr = self.read_u32(addr)? as usize;
        if ptr == 0 {
            return Err(String::from("Read null pointer"));
        }
        Ok(ptr)
    }

    unsafe fn read_array<'a, T>(&self, addr: usize) -> Result<Vec<T>, String>
    where
        T: Default,
        T: Clone,
        T: FromBytes
    {
        if addr == 0 {
            return Ok(Vec::default());
        }

        let len = self.read_u32(addr+4)? as usize;
        if len == 0 {
            return Ok(Vec::default());
        }
        let size = core::mem::size_of::<T>();
        let buf = self.read_raw(addr+8, len*size)?;
        let mut t_buf = vec![T::default(); len];

        for i in 0..len {
            t_buf[i] = T::from_bytes(&buf[i*size..(i+1)*size]);
        }

        Ok(t_buf)
    }

    unsafe fn read_str(&self, addr: usize) -> Result<String, String> {
        if addr == 0 {
            return Ok(String::default());
        }

        let buf = self.read_array::<u16>(addr)?;

        match String::from_utf16(&buf) {
            Ok(s) => Ok(s),
            Err(e) => Err(format!("Unicode decoding failed: {}", e.to_string()))
        }
    }
}