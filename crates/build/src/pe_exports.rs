// Exported data of a PE file (the Windows app's guest library), read from the file: on any host, without loading it.
// lift_overlay reads the base library's function tables this way, so a Mac can lift a mod set against a library made
// for Windows. Only constant data is read (no relocations are applied): the tables hold guest addresses, not pointers.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub struct PeFile {
    file: File,
    path: PathBuf,
    /// (virtual address, virtual size, raw size, raw offset) of each section.
    sections: Vec<(u32, u32, u32, u32)>,
    /// Export directory: (RVA, size).
    exports: (u32, u32),
}

impl PeFile {
    pub fn open(path: &Path) -> Result<PeFile, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut pe = PeFile { file, path: path.to_path_buf(), sections: Vec::new(), exports: (0, 0) };
        let lfanew = pe.u32_at(0x3c)? as u64;
        if pe.bytes_at(lfanew, 4)? != b"PE\0\0" {
            return Err(pe.err("not a PE file"));
        }
        let coff = lfanew + 4;
        let sections = pe.u16_at(coff + 2)? as u64;
        let optional_size = pe.u16_at(coff + 16)? as u64;
        let optional = coff + 20;
        if pe.u16_at(optional)? != 0x20b {
            return Err(pe.err("not a 64-bit PE file"));
        }
        // PE32+: the data directories start at +112; the export table is the first.
        if pe.u32_at(optional + 108)? > 0 {
            pe.exports = (pe.u32_at(optional + 112)?, pe.u32_at(optional + 116)?);
        }
        let table = optional + optional_size;
        for i in 0..sections {
            let s = table + i * 40;
            let section = (pe.u32_at(s + 12)?, pe.u32_at(s + 8)?, pe.u32_at(s + 16)?, pe.u32_at(s + 20)?);
            pe.sections.push(section);
        }
        Ok(pe)
    }

    /// The `len` bytes at exported symbol `name`, if it is exported and its bytes are in the file.
    pub fn symbol_bytes(&mut self, name: &str, len: usize) -> Result<Option<Vec<u8>>, String> {
        let Some(rva) = self.export_rva(name)? else { return Ok(None) };
        let Some(off) = self.file_offset(rva, len as u32) else { return Ok(None) };
        self.bytes_at(off, len).map(Some)
    }

    /// The u64 at exported symbol `name`.
    pub fn symbol_u64(&mut self, name: &str) -> Result<Option<u64>, String> {
        Ok(self.symbol_bytes(name, 8)?.map(|b| u64::from_le_bytes(b.try_into().unwrap())))
    }

    fn export_rva(&mut self, name: &str) -> Result<Option<u32>, String> {
        let (dir, size) = self.exports;
        if size == 0 {
            return Ok(None);
        }
        let dir = self.file_offset(dir, 40).ok_or_else(|| self.err("export directory outside the file"))?;
        let names = self.u32_at(dir + 24)?;
        let (functions, name_rvas, ordinals) = (self.u32_at(dir + 28)?, self.u32_at(dir + 32)?, self.u32_at(dir + 36)?);
        let at = |pe: &PeFile, rva: u32, len: u32| pe.file_offset(rva, len).ok_or_else(|| pe.err("export table outside the file"));
        let (name_off, ordinal_off) = (at(self, name_rvas, names * 4)?, at(self, ordinals, names * 2)?);
        let name_table = self.bytes_at(name_off, names as usize * 4)?;
        let ordinal_table = self.bytes_at(ordinal_off, names as usize * 2)?;
        for i in 0..names as usize {
            let rva = u32::from_le_bytes(name_table[i * 4..i * 4 + 4].try_into().unwrap());
            let off = at(self, rva, name.len() as u32 + 1)?;
            let candidate = self.bytes_at(off, name.len() + 1)?;
            if &candidate[..name.len()] == name.as_bytes() && candidate[name.len()] == 0 {
                let ordinal = u16::from_le_bytes(ordinal_table[i * 2..i * 2 + 2].try_into().unwrap()) as u32;
                let f = at(self, functions + ordinal * 4, 4)?;
                return self.u32_at(f).map(Some);
            }
        }
        Ok(None)
    }

    /// The file offset of `len` bytes at `rva`, when they are all in one section's data in the file.
    fn file_offset(&self, rva: u32, len: u32) -> Option<u64> {
        self.sections.iter().find_map(|&(va, _, raw_size, raw_off)| {
            let rel = rva.checked_sub(va)?;
            (rel as u64 + len as u64 <= raw_size as u64).then_some(raw_off as u64 + rel as u64)
        })
    }

    fn bytes_at(&mut self, off: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut b = vec![0; len];
        self.file.seek(SeekFrom::Start(off)).and_then(|_| self.file.read_exact(&mut b)).map_err(|e| self.err(&e.to_string()))?;
        Ok(b)
    }

    fn u16_at(&mut self, off: u64) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes_at(off, 2)?.try_into().unwrap()))
    }

    fn u32_at(&mut self, off: u64) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes_at(off, 4)?.try_into().unwrap()))
    }

    fn err(&self, what: &str) -> String {
        format!("{}: {what}", self.path.display())
    }
}

#[cfg(test)]
mod tests {
    /// The tables read from the file are those the loaded library holds. Needs a guest library: NXPORT_TEST_GUEST_DLL.
    #[test]
    #[cfg(windows)]
    fn tables_match_the_loaded_library() {
        let Some(path) = std::env::var_os("NXPORT_TEST_GUEST_DLL").map(std::path::PathBuf::from) else { return };
        let modules = std::fs::read_to_string(path.with_file_name(crate::lift::MODULES_FILE)).unwrap();
        let mut pe = super::PeFile::open(&path).unwrap();
        let lib = nxport::os::Library::open(&path).unwrap();
        for m in modules.lines() {
            let count = pe.symbol_u64(&format!("{m}_fn_count")).unwrap().unwrap();
            let addrs = pe.symbol_bytes(&format!("{m}_fn_addrs"), count as usize * 8).unwrap().unwrap();
            let loaded_count = unsafe { *(lib.symbol(&format!("{m}_fn_count")) as *const u64) };
            let loaded = unsafe { std::slice::from_raw_parts(lib.symbol(&format!("{m}_fn_addrs")) as *const u8, count as usize * 8) };
            assert_eq!(count, loaded_count, "{m}");
            assert!(addrs == loaded, "{m}: tables differ");
            println!("{m}: {count} functions, same");
        }
        assert_eq!(pe.symbol_u64("no_such_symbol").unwrap(), None);
    }
}
